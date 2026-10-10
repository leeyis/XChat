use super::*;
use crate::web_server::AppState;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as UrlPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use tokio::io::AsyncWriteExt;

fn response<T: Serialize>(result: std::result::Result<T, String>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":error})),
        )
            .into_response(),
    }
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/backups", get(list).post(start))
        .route(
            "/api/backups/import",
            post(upload).layer(DefaultBodyLimit::max(
                (8 * 1024 * 1024 * 1024u64).min(usize::MAX as u64) as usize,
            )),
        )
        .route("/api/backups/:id/download", get(download))
        .route("/api/backups/:id/preview", post(preview))
        .route("/api/backups/jobs/:id", get(status))
        .route("/api/backups/jobs/:id/cancel", post(cancel_job))
        .route("/api/backups/jobs/:id/restore", post(merge))
}
async fn list(State(state): State<Arc<AppState>>) -> Response {
    response(overview(&state.pool).await)
}
async fn start(
    State(state): State<Arc<AppState>>,
    Json(mut request): Json<CreateRequest>,
) -> Response {
    // Browser clients download a managed backup; they cannot choose an arbitrary server path.
    request.directory = None;
    response(create(&state.pool, request).await)
}
async fn status(State(state): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    response(job_status(&state.pool, &id).await)
}
async fn cancel_job(State(state): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    response(cancel(&state.pool, &id).await)
}
async fn preview(State(state): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    match file_path(&state.pool, &id).await {
        Ok(path) => response(prepare(&state.pool, path, false).await),
        Err(error) => response::<JobView>(Err(error)),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeRequest {
    include_settings: bool,
}
async fn merge(
    State(state): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    Json(request): Json<MergeRequest>,
) -> Response {
    response(commit_restore(&state.pool, &id, request.include_settings).await)
}
async fn download(State(state): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    let result = async {
        let path = file_path(&state.pool, &id).await?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| "备份文件已移动或删除".to_string())?;
        let bytes = file.metadata().await.map_err(|e| e.to_string())?.len();
        let disposition = format!("attachment; filename=\"XChat-{id}.xchatbackup\"");
        Ok::<_, String>(
            (
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                    (header::CONTENT_DISPOSITION, disposition),
                    (header::CONTENT_LENGTH, bytes.to_string()),
                ],
                Body::from_stream(tokio_util::io::ReaderStream::new(file)),
            )
                .into_response(),
        )
    }
    .await;
    result.unwrap_or_else(|e| response::<JobView>(Err(e)))
}
async fn upload(State(state): State<Arc<AppState>>, mut multipart: Multipart) -> Response {
    let result = async {
        let folder = root(&state.pool).await?.join("uploads");
        tokio::fs::create_dir_all(&folder).await?;
        let path = folder.join(uuid::Uuid::new_v4().to_string());
        let saved = async {
            let mut field = multipart.next_field().await?.ok_or("请选择备份文件")?;
            if field.name() != Some("file") {
                return Err("请选择备份文件".into());
            }
            let mut output = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .await?;
            let mut bytes = 0u64;
            while let Some(chunk) = field.chunk().await? {
                bytes = bytes.saturating_add(chunk.len() as u64);
                if bytes > 8 * 1024 * 1024 * 1024 {
                    return Err("浏览器备份导入上限为 8 GiB，请使用桌面客户端导入更大的备份".into());
                }
                if bytes < chunk.len() as u64 + 1 || bytes % (32 * 1024 * 1024) < chunk.len() as u64
                {
                    require_space(&folder, 32 * 1024 * 1024).await?;
                }
                output.write_all(&chunk).await?;
            }
            output.sync_all().await?;
            drop(output);
            Ok(prepare(&state.pool, path.clone(), true).await?)
        }
        .await;
        if saved.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        saved
    }
    .await;
    response(result.map_err(|e: Box<dyn std::error::Error + Send + Sync>| e.to_string()))
}
