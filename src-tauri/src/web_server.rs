use axum::extract::ws::{Message, WebSocket};
use axum::{
    body::Body,
    extract::{
        ConnectInfo, DefaultBodyLimit, Json, Multipart, Path, Query, Request, State,
        WebSocketUpgrade,
    },
    http::{header, Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

use crate::peers::PeerManager;

// 全局媒体 Token（仅 Android 使用）
static MEDIA_TOKEN: Mutex<String> = Mutex::new(String::new());
// ponytail: group message writes are rare; shard this lock by client_message_id if throughput matters.
static GROUP_MESSAGE_WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(RustEmbed)]
#[folder = "../src/"]
struct Asset;

#[derive(Serialize)]
struct NameResponse {
    name: String,
}

#[derive(Deserialize)]
struct UpdateNameRequest {
    name: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct SendMessageRequest {
    peer_id: String,
    peer_addr: String,
    content: String,
}

#[derive(Deserialize)]
struct PeerIdRequest {
    peer_id: String,
}

#[derive(Deserialize)]
struct WorkspacePreferenceRequest {
    key: String,
    value: String,
}

// Web 服务器的状态
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool<Sqlite>,
    pub peer_manager: Arc<PeerManager>,
    pub media_token: String,
    pub ws_broadcast: broadcast::Sender<String>,
    #[cfg(feature = "desktop")]
    pub app_handle: Option<tauri::AppHandle>,
}

type ApiResponse = axum::response::Response;
const MAX_BROWSER_UPLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_PEER_CHUNK_BYTES: usize = 8 * 1024 * 1024;
const MAX_UPLOAD_REQUEST_BYTES: usize = 5 * 1024 * 1024;
const WEB_OUTBOX_DIRECTORY: &str = ".xchat-outbox";

fn api_error(status: StatusCode, error: impl Into<String>) -> ApiResponse {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
        .into_response()
}

fn backend_error(error: String) -> ApiResponse {
    let lower = error.to_ascii_lowercase();
    let status =
        if lower.contains("not found") || error.contains("不存在") || error.contains("找不到")
        {
            StatusCode::NOT_FOUND
        } else if error.contains("失败") {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::BAD_REQUEST
        };
    api_error(status, error)
}

async fn managed_web_outbox_root(pool: &Pool<Sqlite>) -> Result<std::path::PathBuf, String> {
    let configured_root = std::path::PathBuf::from(crate::db::get_download_path(pool).await?);
    tokio::fs::create_dir_all(&configured_root)
        .await
        .map_err(|error| format!("创建下载目录失败: {error}"))?;
    let download_root = tokio::fs::canonicalize(&configured_root)
        .await
        .map_err(|error| format!("解析下载目录失败: {error}"))?;
    let configured_outbox = configured_root.join(WEB_OUTBOX_DIRECTORY);
    tokio::fs::create_dir_all(&configured_outbox)
        .await
        .map_err(|error| format!("创建 Web 发件箱失败: {error}"))?;
    let outbox = tokio::fs::canonicalize(configured_outbox)
        .await
        .map_err(|error| format!("解析 Web 发件箱失败: {error}"))?;
    if !outbox.starts_with(download_root) {
        return Err("Web 发件箱必须位于下载目录内".to_string());
    }
    Ok(outbox)
}

fn safe_file_name(value: &str) -> Option<String> {
    let value = value.trim();
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let windows_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && matches!(&stem[..3], "COM" | "LPT")
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 240
        || value.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || value.ends_with(['.', ' '])
        || value.chars().any(char::is_control)
        || windows_device
    {
        None
    } else {
        Some(value.to_string())
    }
}

#[derive(Deserialize)]
struct CreateGroupRequest {
    title: String,
    member_ids: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateGroupRequest {
    operation: String,
    value: Option<String>,
    #[serde(default)]
    member_ids: Vec<String>,
}

#[derive(Deserialize)]
struct RecallMessageRequest {
    client_message_id: String,
}

#[derive(Deserialize)]
struct MessageControlRequest {
    conversation_id: String,
    client_message_id: String,
    emoji: Option<String>,
}

#[derive(Deserialize)]
struct ForwardMessageRequest {
    source_message_id: i64,
    conversation_ids: Vec<String>,
    note: Option<String>,
}

#[derive(Deserialize)]
struct ConversationMessageRequest {
    client_message_id: String,
    content: String,
    mention_ids: Vec<String>,
    #[serde(default = "default_message_type")]
    msg_type: String,
}

fn default_message_type() -> String {
    "text".to_string()
}

#[derive(Deserialize)]
struct MessagePageQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct ReadReceiptRequest {
    conversation_id: String,
    message_ids: Vec<String>,
}

#[derive(Deserialize)]
struct SearchMessagesQuery {
    q: String,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct ConversationStateRequest {
    pinned: Option<bool>,
    forced_unread: Option<bool>,
    draft: Option<String>,
}

#[derive(Deserialize)]
struct UpdateDeviceRequest {
    remark: Option<String>,
}

pub async fn start_server(
    port: u16,
    _udp_port: u16,
    pool: Pool<Sqlite>,
    peer_manager: Arc<PeerManager>,
    #[cfg(feature = "desktop")] app_handle: Option<tauri::AppHandle>,
) {
    let media_token = uuid::Uuid::new_v4().to_string();
    println!("[Web Server] 媒体访问 Token: {}", media_token);

    // 将 token 存入全局，供 Tauri command 读取（仅 Android）
    #[cfg(target_os = "android")]
    {
        let mut guard = MEDIA_TOKEN.lock().unwrap();
        *guard = media_token.clone();
    }

    let (ws_broadcast, _) = broadcast::channel::<String>(128);

    let state = Arc::new(AppState {
        pool,
        peer_manager,
        media_token,
        ws_broadcast,
        #[cfg(feature = "desktop")]
        app_handle,
    });
    if let Ok(download_root) = crate::db::get_download_path(&state.pool).await {
        if let Err(error) = crate::network::conversation_file::cleanup_stale_received_partials(
            std::path::Path::new(&download_root),
            std::time::Duration::from_secs(24 * 60 * 60),
        )
        .await
        {
            eprintln!("[Web Server] 清理过期下载临时文件失败: {error}");
        }
    }

    // 配置 CORS - 允许所有来源（局域网内部使用）
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_credentials(false); // 明确设置不需要凭证

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/workspace", get(get_workspace_http))
        .route(
            "/api/settings/preference",
            post(update_workspace_preference_http),
        )
        .route("/api/groups", post(create_group_http))
        .route("/api/groups/:id", post(update_group_http))
        .route(
            "/api/conversations/:id/messages",
            get(get_conversation_messages_http).post(send_conversation_message_http),
        )
        .route(
            "/api/conversations/:id/files",
            post(send_conversation_file_http),
        )
        .route("/api/receipts/read", post(mark_messages_read_http))
        .route("/api/messages/search", get(search_workspace_messages_http))
        .route(
            "/api/conversations/:id/recall",
            post(recall_conversation_message_http),
        )
        .route("/api/messages/forward", post(forward_message_http))
        .route("/api/message/reaction", post(react_to_message_http))
        .route(
            "/api/message/strong-reminder",
            post(send_strong_reminder_http),
        )
        .route(
            "/api/conversations/:id/state",
            post(update_conversation_state_http),
        )
        .route(
            "/api/conversations/:id/clear",
            post(clear_conversation_history_http),
        )
        .route("/api/files", get(get_file_center_http))
        .route("/api/files/:id/retry", post(retry_conversation_file_http))
        .route("/api/transfers", get(get_transfers_http))
        .route("/api/transfers/:id/cancel", post(cancel_transfer_http))
        .route("/api/devices/:id", post(update_device_http))
        .route("/api/files/:id/delete", post(delete_local_file_http))
        .route(
            "/api/uploads/:client_message_id/cancel",
            post(cancel_received_upload_http),
        )
        .route(
            "/api/uploads/v2/prepare",
            post(prepare_parallel_upload_http)
                .layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route(
            "/api/uploads/v2/:transfer_id/:chunk_index",
            post(receive_parallel_upload_http),
        )
        .route("/api/get_my_name", get(get_name_http))
        .route("/api/get_my_id", get(get_id_http))
        .route("/api/update_my_name", post(update_name_http))
        .route("/api/get_settings", get(get_settings_http))
        .route("/api/update_settings", post(update_settings_http))
        .route("/api/get_language", get(get_language_http))
        .route("/api/set_language", post(set_language_http))
        .route("/api/get_custom_peers", get(get_custom_peers_http))
        .route("/api/add_custom_peer", post(add_custom_peer_http))
        .route("/api/remove_custom_peer", post(remove_custom_peer_http))
        .route("/api/get_peers", get(get_peers_http))
        .route("/api/send_message", post(send_message_http))
        .route("/api/chat_history/:peer_id", get(get_chat_history_http))
        .route(
            "/api/upload",
            post(upload_file_http).layer(DefaultBodyLimit::max(MAX_UPLOAD_REQUEST_BYTES)),
        )
        .route("/api/accept_file/:file_id", post(accept_file_http))
        .route("/api/download/:file_id", get(download_file_http))
        .route("/api/create_upload_record", post(create_upload_record_http))
        .route("/api/update_upload_status", post(update_upload_status_http))
        .route("/api/mark_upload_complete", post(mark_upload_complete_http))
        .route("/api/delete_upload_record", post(delete_upload_record_http))
        .route("/api/clear_chat_history", post(clear_chat_history_http))
        .route("/api/delete_user", post(delete_user_http))
        .route("/api/delete_messages", post(delete_messages_http))
        .route("/api/get_theme_list", get(get_theme_list_http))
        .route("/api/get_theme_css/:theme_name", get(get_theme_css_http))
        .route("/api/save_current_theme", post(save_current_theme_http))
        .route("/api/get_current_theme", get(get_current_theme_http))
        .route("/api/auto_download", get(auto_download_http))
        .route("/api/offer_file", post(offer_file_http))
        .route("/api/start_send", post(start_send_http))
        .route("/api/request_file", post(request_file_http))
        .route("/api/media", get(serve_media_http))
        .route("/ws", get(websocket_handler))
        .route("/*path", get(serve_assets))
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::disable()) // 无限制
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("[Web Server] 启动在端口 {} (无文件大小限制)", port);
    axum::serve(listener, app).await.unwrap();
}

async fn get_workspace_http(State(state): State<Arc<AppState>>) -> ApiResponse {
    match crate::workspace::get_snapshot(&state.pool, &state.peer_manager).await {
        Ok(mut snapshot) => {
            snapshot.capabilities.capture = true;
            snapshot.capabilities.capture_shortcut = true;
            snapshot.capabilities.reveal_file = false;
            snapshot.capabilities.native_file_picker = false;
            Json(snapshot).into_response()
        }
        Err(error) => backend_error(error),
    }
}

async fn update_workspace_preference_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WorkspacePreferenceRequest>,
) -> ApiResponse {
    match crate::workspace::update_preference(&state.pool, &payload.key, &payload.value).await {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn create_group_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateGroupRequest>,
) -> ApiResponse {
    match crate::workspace::create_group(
        &state.pool,
        &state.peer_manager,
        &payload.title,
        payload.member_ids,
    )
    .await
    {
        Ok(conversation) => (StatusCode::CREATED, Json(conversation)).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn update_group_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
    Json(payload): Json<UpdateGroupRequest>,
) -> ApiResponse {
    match crate::workspace::update_group(
        &state.pool,
        &state.peer_manager,
        &conversation_id,
        &payload.operation,
        payload.value,
        payload.member_ids,
    )
    .await
    {
        Ok(conversation) => Json(conversation).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn recall_conversation_message_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
    Json(payload): Json<RecallMessageRequest>,
) -> ApiResponse {
    match crate::workspace::recall_message(
        &state.pool,
        &state.peer_manager,
        &conversation_id,
        &payload.client_message_id,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn forward_message_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ForwardMessageRequest>,
) -> ApiResponse {
    match crate::workspace::forward_message(
        &state.pool,
        &state.peer_manager,
        payload.source_message_id,
        payload.conversation_ids,
        payload.note,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn get_conversation_messages_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
    Query(query): Query<MessagePageQuery>,
) -> ApiResponse {
    match crate::workspace::get_messages(
        &state.pool,
        &state.peer_manager,
        &conversation_id,
        query.limit.unwrap_or(40),
        query.offset.unwrap_or(0),
    )
    .await
    {
        Ok(messages) => Json(serde_json::json!({ "messages": messages })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn send_conversation_message_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
    Json(payload): Json<ConversationMessageRequest>,
) -> ApiResponse {
    match crate::workspace::send_message(
        &state.pool,
        &state.peer_manager,
        &conversation_id,
        &payload.client_message_id,
        &payload.content,
        &payload.msg_type,
        payload.mention_ids,
    )
    .await
    {
        Ok(message) => Json(message).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn react_to_message_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MessageControlRequest>,
) -> ApiResponse {
    let Some(emoji) = payload.emoji else {
        return api_error(StatusCode::BAD_REQUEST, "emoji is required".to_string());
    };
    match crate::workspace::react_to_message(
        &state.pool,
        &state.peer_manager,
        &payload.conversation_id,
        &payload.client_message_id,
        &emoji,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn send_strong_reminder_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MessageControlRequest>,
) -> ApiResponse {
    match crate::workspace::send_strong_reminder(
        &state.pool,
        &state.peer_manager,
        &payload.conversation_id,
        &payload.client_message_id,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn send_conversation_file_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
    mut multipart: Multipart,
) -> ApiResponse {
    let mut staged: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                if let Some((directory, _)) = staged {
                    let _ = tokio::fs::remove_dir_all(directory).await;
                }
                return api_error(
                    StatusCode::BAD_REQUEST,
                    format!("解析上传内容失败: {error}"),
                );
            }
        };
        if field.name() != Some("file") {
            continue;
        }
        if staged.is_some() {
            if let Some((directory, _)) = staged {
                let _ = tokio::fs::remove_dir_all(directory).await;
            }
            return api_error(StatusCode::BAD_REQUEST, "一次请求只能上传一个文件");
        }
        let Some(file_name) = field.file_name().and_then(safe_file_name) else {
            return api_error(StatusCode::BAD_REQUEST, "文件名无效");
        };
        let outbox = match managed_web_outbox_root(&state.pool).await {
            Ok(outbox) => outbox,
            Err(error) => {
                return api_error(StatusCode::INTERNAL_SERVER_ERROR, error);
            }
        };
        let directory = outbox.join(uuid::Uuid::new_v4().to_string());
        if let Err(error) = tokio::fs::create_dir_all(&directory).await {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("创建上传暂存目录失败: {error}"),
            );
        }
        let path = directory.join(file_name);
        let mut file = match tokio::fs::File::create(&path).await {
            Ok(file) => file,
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&directory).await;
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("创建上传暂存文件失败: {error}"),
                );
            }
        };
        let mut field = field;
        let mut written = 0u64;
        loop {
            let chunk = match field.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => {
                    let _ = tokio::fs::remove_dir_all(&directory).await;
                    return api_error(
                        StatusCode::BAD_REQUEST,
                        format!("读取上传内容失败: {error}"),
                    );
                }
            };
            written = match written.checked_add(chunk.len() as u64) {
                Some(total) if total <= MAX_BROWSER_UPLOAD_BYTES => total,
                _ => {
                    let _ = tokio::fs::remove_dir_all(&directory).await;
                    return api_error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "单个文件不能超过 8 GiB",
                    );
                }
            };
            if let Err(error) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await {
                let _ = tokio::fs::remove_dir_all(&directory).await;
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("写入上传暂存文件失败: {error}"),
                );
            }
        }
        if let Err(error) = tokio::io::AsyncWriteExt::flush(&mut file).await {
            let _ = tokio::fs::remove_dir_all(&directory).await;
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("保存上传暂存文件失败: {error}"),
            );
        }
        staged = Some((directory, path));
    }

    let Some((directory, path)) = staged else {
        return api_error(StatusCode::BAD_REQUEST, "缺少 file 字段");
    };
    let Some(path) = path.to_str() else {
        let _ = tokio::fs::remove_dir_all(&directory).await;
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "暂存路径不是有效 UTF-8");
    };
    match crate::network::conversation_file::send_path(
        &state.pool,
        &state.peer_manager,
        &conversation_id,
        path,
    )
    .await
    {
        Ok(result) => {
            // ponytail: waiting_peer 和发送方媒体预览依赖这个受控副本；
            // 有持久存储配额后再回收终态副本。
            let waiting = result
                .transfers
                .iter()
                .filter(|transfer| transfer.status == "waiting_peer")
                .map(|transfer| transfer.peer_id.clone())
                .collect::<Vec<_>>();
            let mut body = serde_json::to_value(result).unwrap_or_default();
            body["status"] = serde_json::json!(if waiting.is_empty() {
                "queued"
            } else {
                "waiting_peer"
            });
            if !waiting.is_empty() {
                body["code"] = serde_json::json!("peer_offline");
                body["waiting_peer_ids"] = serde_json::json!(waiting);
                body["message_text"] =
                    serde_json::json!("部分设备离线，已排队并会在设备上线后继续");
            }
            (
                if waiting.is_empty() {
                    StatusCode::CREATED
                } else {
                    StatusCode::ACCEPTED
                },
                Json(body),
            )
                .into_response()
        }
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&directory).await;
            backend_error(error)
        }
    }
}

async fn mark_messages_read_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReadReceiptRequest>,
) -> ApiResponse {
    match crate::workspace::mark_messages_read(
        &state.pool,
        &state.peer_manager,
        &payload.conversation_id,
        payload.message_ids,
    )
    .await
    {
        Ok(marked) => Json(serde_json::json!({ "marked": marked })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn search_workspace_messages_http(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchMessagesQuery>,
) -> ApiResponse {
    match crate::workspace::search_messages(
        &state.pool,
        &state.peer_manager,
        &query.q,
        query.limit.unwrap_or(100),
    )
    .await
    {
        Ok(messages) => Json(serde_json::json!({ "messages": messages })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn update_conversation_state_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
    Json(payload): Json<ConversationStateRequest>,
) -> ApiResponse {
    if payload.pinned.is_none() && payload.forced_unread.is_none() && payload.draft.is_none() {
        return api_error(StatusCode::BAD_REQUEST, "至少需要一个会话状态字段");
    }
    match crate::db::update_conversation_state(
        &state.pool,
        &conversation_id,
        payload.pinned,
        payload.forced_unread,
        payload.draft.as_deref(),
    )
    .await
    {
        Ok(conversation) => Json(conversation).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn clear_conversation_history_http(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
) -> ApiResponse {
    match crate::workspace::clear_conversation_history(&state.pool, &conversation_id).await {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn get_file_center_http(State(state): State<Arc<AppState>>) -> ApiResponse {
    match crate::workspace::file_center(&state.pool, &state.peer_manager).await {
        Ok(files) => Json(serde_json::json!({ "files": files })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn get_transfers_http(State(state): State<Arc<AppState>>) -> ApiResponse {
    match crate::workspace::transfers(&state.pool).await {
        Ok(transfers) => Json(serde_json::json!({ "transfers": transfers })).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn cancel_transfer_http(
    State(state): State<Arc<AppState>>,
    Path(transfer_id): Path<String>,
) -> ApiResponse {
    match crate::workspace::cancel_transfer(&state.pool, &transfer_id).await {
        Ok(transfer) => Json(transfer).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn update_device_http(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
    Json(payload): Json<UpdateDeviceRequest>,
) -> ApiResponse {
    match crate::workspace::update_device(&state.pool, &device_id, payload.remark.as_deref()).await
    {
        Ok(device) => Json(device).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn delete_local_file_http(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> ApiResponse {
    let Ok(message_id) = message_id.parse::<i64>() else {
        return api_error(StatusCode::BAD_REQUEST, "无效的文件消息 ID");
    };
    match crate::workspace::delete_local_file(&state.pool, message_id).await {
        Ok(message) => Json(message).into_response(),
        Err(error) => backend_error(error),
    }
}

async fn retry_conversation_file_http(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> ApiResponse {
    let Ok(message_id) = message_id.parse::<i64>() else {
        return api_error(StatusCode::BAD_REQUEST, "无效的文件消息 ID");
    };
    match crate::network::conversation_file::retry_message(
        &state.pool,
        &state.peer_manager,
        message_id,
    )
    .await
    {
        Ok(result) => (StatusCode::ACCEPTED, Json(result)).into_response(),
        Err(error) => backend_error(error),
    }
}

#[derive(Default, Deserialize)]
struct CancelReceivedUploadQuery {
    status: Option<String>,
    transfer_id: Option<String>,
    peer_id: Option<String>,
}

async fn cancel_received_upload_http(
    State(state): State<Arc<AppState>>,
    Path(client_message_id): Path<String>,
    Query(query): Query<CancelReceivedUploadQuery>,
) -> ApiResponse {
    let status = match query.status.as_deref() {
        None | Some("cancelled") => "cancelled",
        Some("failed") => "failed",
        Some(_) => return api_error(StatusCode::BAD_REQUEST, "无效的传输终态"),
    };
    let Some(transfer_id) = query
        .transfer_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return api_error(StatusCode::BAD_REQUEST, "缺少传输 ID");
    };
    let _guard = crate::network::conversation_file::lock_receive_file(&client_message_id).await;
    let message = match crate::db::get_message_by_client_id(&state.pool, &client_message_id).await {
        Ok(Some(message)) => message,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "文件消息不存在"),
        Err(error) => return backend_error(error),
    };
    let transfer = match crate::db::get_transfer(&state.pool, transfer_id).await {
        Ok(Some(transfer)) if transfer.message_id == Some(message.id) => transfer,
        Ok(Some(_)) => return api_error(StatusCode::CONFLICT, "传输与文件消息不匹配"),
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "文件传输不存在"),
        Err(error) => return backend_error(error),
    };
    if matches!(transfer.status.as_str(), "completed" | "cancelled") {
        return Json(serde_json::json!({
            "success": true,
            "status": transfer.status,
            "transfer_id": transfer.id,
        }))
        .into_response();
    }
    let self_id = match crate::db::get_user_id(&state.pool).await {
        Ok(id) => id,
        Err(error) => return backend_error(error),
    };
    if message.msg_type != "file" {
        return api_error(StatusCode::CONFLICT, "该传输不是文件传输");
    }
    let incoming = message.sender_id != self_id && message.sender_id != "me";
    let event_status;
    if incoming {
        if transfer.direction != "receive" || transfer.peer_id != message.sender_id {
            return api_error(StatusCode::CONFLICT, "接收传输与发送设备不匹配");
        }
        let download_root = match crate::db::get_download_path(&state.pool).await {
            Ok(path) => std::path::PathBuf::from(path),
            Err(error) => return backend_error(error),
        };
        let partial_path =
            crate::network::conversation_file::received_partial_path(&download_root, &transfer.id);
        if let Err(error) = tokio::fs::remove_file(&partial_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("清理接收临时文件失败: {error}"),
                );
            }
        }
        if status == "cancelled" {
            if let Err(error) =
                crate::network::conversation_file::cleanup_parallel_transfer(
                    &download_root,
                    &transfer.id,
                )
                .await
            {
                return backend_error(error);
            }
        }
        if !matches!(transfer.status.as_str(), "completed" | "cancelled") {
            if let Err(error) = crate::db::update_transfer(
                &state.pool,
                &transfer.id,
                status,
                transfer.bytes_transferred,
                (status == "failed").then_some("发送端传输失败"),
            )
            .await
            {
                return backend_error(error);
            }
        }
        let latest_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM transfers
             WHERE message_id = ? AND direction = 'receive'
             ORDER BY rowid DESC LIMIT 1",
        )
        .bind(message.id)
        .fetch_optional(&state.pool)
        .await;
        match latest_id {
            Ok(Some(latest_id)) if latest_id == transfer.id => {
                if let Err(error) =
                    crate::db::update_file_status_by_id(&state.pool, message.id, status).await
                {
                    return backend_error(error);
                }
                event_status = Some(status.to_string());
            }
            Ok(_) => event_status = None,
            Err(error) => return backend_error(format!("查询最新接收传输失败: {error}")),
        }
    } else {
        if transfer.direction != "send"
            || query.peer_id.as_deref().map(str::trim) != Some(transfer.peer_id.as_str())
        {
            return api_error(StatusCode::CONFLICT, "发送传输与接收设备不匹配");
        }
        if !matches!(transfer.status.as_str(), "completed" | "cancelled") {
            if let Err(error) = crate::db::update_transfer(
                &state.pool,
                &transfer.id,
                status,
                transfer.bytes_transferred,
                (status == "failed").then_some("接收端传输失败"),
            )
            .await
            {
                return backend_error(error);
            }
        }
        if let Err(error) =
            crate::network::conversation_file::refresh_file_status(&state.pool, message.id).await
        {
            return backend_error(error);
        }
        event_status = match crate::db::get_file_message_by_id(&state.pool, message.id).await {
            Ok(Some(message)) => message.file_status,
            Ok(None) => None,
            Err(error) => return backend_error(error),
        };
    }
    if let Some(event_status) = event_status.as_deref() {
        let event = serde_json::json!({
            "msg_type": "file_status_update",
            "id": message.id,
            "client_message_id": client_message_id,
            "file_status": event_status,
            "transfer_id": transfer.id,
        });
        broadcast_incoming_event(&state, event);
    }
    Json(serde_json::json!({
        "success": true,
        "status": event_status.as_deref().unwrap_or(status),
        "transfer_id": transfer.id,
    }))
    .into_response()
}

async fn get_name_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    println!("[Web Server] 收到获取用户名请求");

    match crate::db::get_username(&state.pool).await {
        Ok(name) => Json(NameResponse { name }).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("读取用户名失败: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn get_id_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    println!("[Web Server] 收到获取用户 ID 请求");

    match crate::db::get_user_id(&state.pool).await {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("读取用户 ID 失败: {}", e),
            }),
        )
            .into_response(),
    }
}

async fn get_settings_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    println!("[Web Server] 收到获取设置请求");

    let download_path = crate::db::get_download_path(&state.pool)
        .await
        .unwrap_or_else(|_| {
            std::env::temp_dir()
                .join("xchat-downloads")
                .to_str()
                .unwrap()
                .to_string()
        });

    let port = crate::config_file::get_port_from_config()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "8888".to_string());

    let cfg = crate::config_file::read_config();
    let db_path = cfg.db_path.unwrap_or_else(crate::config_file::get_default_db_path);

    let auto_download = crate::db::get_auto_download(&state.pool).await;

    Json(serde_json::json!({
        "download_path": download_path,
        "port": port,
        "db_path": db_path,
        "auto_download": auto_download,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct UpdateSettingsRequest {
    download_path: Option<String>,
    port: Option<String>,
    db_path: Option<String>,
    auto_download: Option<bool>,
}

async fn update_settings_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateSettingsRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 收到更新设置请求");

    if let Some(ref path) = payload.download_path {
        if let Err(e) = crate::db::update_download_path(&state.pool, path.clone()).await {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response();
        }
    }
    if let Some(ref p) = payload.port {
        let port_num: u16 = match p.parse() {
            Ok(n) if n > 0 => n,
            _ => {
                return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid port".to_string() })).into_response();
            }
        };
        if let Err(e) = crate::config_file::save_port_to_config(port_num) {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response();
        }
    }
    if let Some(path) = payload.db_path {
        if !cfg!(target_os = "android") {
            let mut cfg = crate::config_file::read_config();
            if path.is_empty() {
                cfg.db_path = None;
            } else {
                cfg.db_path = Some(path);
            }
            if let Err(e) = crate::config_file::write_config(&cfg) {
                return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response();
            }
        }
    }
    if let Some(enabled) = payload.auto_download {
        let _ = crate::db::set_auto_download(&state.pool, enabled).await;
    }

    Json(serde_json::json!({ "success": true })).into_response()
}

async fn get_language_http() -> impl IntoResponse {
    Json(serde_json::json!({
        "lang": crate::config_file::get_lang_from_config().unwrap_or_else(|| "auto".to_string())
    }))
}

#[derive(Deserialize)]
struct SetLanguageRequest {
    lang: String,
}

async fn set_language_http(Json(payload): Json<SetLanguageRequest>) -> impl IntoResponse {
    match crate::config_file::save_lang_to_config(&payload.lang) {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response(),
    }
}

#[derive(Deserialize)]
struct CustomPeerRequest {
    peer: String,
}

async fn get_custom_peers_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let peers = crate::db::get_custom_peers(&state.pool).await;
    Json(serde_json::json!({ "peers": peers })).into_response()
}

async fn add_custom_peer_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CustomPeerRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 添加自定义 IP: {}", payload.peer);
    if let Err(e) = crate::db::add_custom_peer(&state.pool, &payload.peer).await {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response();
    }
    Json(serde_json::json!({ "success": true })).into_response()
}

async fn remove_custom_peer_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CustomPeerRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 删除自定义 IP: {}", payload.peer);
    if let Err(e) = crate::db::remove_custom_peer(&state.pool, &payload.peer).await {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response();
    }
    Json(serde_json::json!({ "success": true })).into_response()
}

// ── 自动下载状态 ──
async fn auto_download_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let enabled = crate::db::get_auto_download(&state.pool).await;
    Json(serde_json::json!({"enabled": enabled}))
}

// ── 本端前端发送 file_offer（POST /api/offer_file） ──
#[derive(Deserialize)]
struct OfferFileRequest {
    peer_addr: String,
    file_name: String,
    file_size: u64,
    sender_msg_id: i64,
}

async fn offer_file_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<OfferFileRequest>,
) -> impl IntoResponse {
    println!("[Web Server] file_offer: {} -> {}", payload.file_name, payload.peer_addr);
    let my_id = crate::db::get_user_id(&state.pool).await.unwrap_or_default();
    let my_name = crate::db::get_username(&state.pool).await.unwrap_or_default();
    let offer = serde_json::json!({
        "msg_type": "file_offer",
        "from_id": my_id,
        "from_name": my_name,
        "file_name": payload.file_name,
        "file_size": payload.file_size,
        "sender_msg_id": payload.sender_msg_id,
    });
    match crate::network::messaging::send_json_via_ws(&payload.peer_addr, &offer.to_string()).await {
        Ok(_) => Json(serde_json::json!({"success": true})).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
            "error": format!("发送 file_offer 失败: {}", e)
        }))).into_response(),
    }
}

// ── 对方请求开始发送文件（POST /api/start_send） ──
#[derive(Deserialize)]
struct StartSendRequest {
    sender_msg_id: i64,
    receiver_addr: String,
}

async fn start_send_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StartSendRequest>,
) -> impl IntoResponse {
    println!("[手动下载] HTTP请求开始发送文件: msg_id={}, 接收端={}", payload.sender_msg_id, payload.receiver_addr);

    // 查询发送端 DB 中的文件记录
    match crate::db::get_sender_file_by_msg_id(&state.pool, payload.sender_msg_id).await {
        Ok((file_path, file_name, file_size)) => {
            if file_path.is_empty() || !std::path::Path::new(&file_path).exists() {
                if file_path.is_empty() {
                    // Web 端：文件在浏览器中 → 通知浏览器开始上传
                    let start_evt = serde_json::json!({
                        "msg_type": "start_upload",
                        "sender_msg_id": payload.sender_msg_id,
                        "file_name": file_name,
                        "file_size": file_size,
                        "receiver_addr": payload.receiver_addr,
                    });
                    let _ = state.ws_broadcast.send(start_evt.to_string());
                    #[cfg(feature = "desktop")]
                    if let Some(ref app) = state.app_handle {
                        use tauri::Emitter;
                        let _ = app.emit("new-message", start_evt);
                    }
                    // 通知发送端前端显示上传中
                    let upload_notice = serde_json::json!({
                        "msg_type": "file_status_update",
                        "sender_msg_id": payload.sender_msg_id,
                        "file_status": "uploading",
                    });
                    let _ = state.ws_broadcast.send(upload_notice.to_string());
                    #[cfg(feature = "desktop")]
                    if let Some(ref app) = state.app_handle {
                        use tauri::Emitter;
                        let _ = app.emit("new-message", upload_notice);
                    }
                    return (StatusCode::OK, Json(serde_json::json!({
                        "success": true, "status": "notifying_browser"
                    }))).into_response();
                }
                // 文件丢失 → 通知接收端
                let notice = serde_json::json!({
                    "msg_type": "file_not_found",
                    "sender_msg_id": payload.sender_msg_id,
                });
                let _ = crate::network::messaging::send_json_via_ws(
                    &payload.receiver_addr,
                    &notice.to_string(),
                ).await;
                return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                    "error": "文件不存在",
                    "not_found": true,
                }))).into_response();
            }

            // 文件存在，开始上传
            let file = match tokio::fs::File::open(&file_path).await {
                Ok(f) => f,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": format!("打开文件失败: {}", e)
                    }))).into_response();
                }
            };

            // 通知发送端前端显示上传中
            let upload_notice = serde_json::json!({
                "msg_type": "file_status_update",
                "sender_msg_id": payload.sender_msg_id,
                "file_status": "uploading",
            });
            let _ = state.ws_broadcast.send(upload_notice.to_string());
            #[cfg(feature = "desktop")]
            if let Some(ref app) = state.app_handle {
                use tauri::Emitter;
                let _ = app.emit("new-message", upload_notice);
            }

            // 调用同步的上传函数
            let _ = state.pool.clone();
            let pool = state.pool.clone();
            let receiver_addr = payload.receiver_addr.clone();
            let fname = file_name.clone();
            let fsize = file_size as usize;
            let fpath = file_path.clone();

            let sm_id = payload.sender_msg_id;
            let ws_tx = state.ws_broadcast.clone();
            #[cfg(feature = "desktop")]
            let app_clone = state.app_handle.clone();
            tokio::spawn(async move {
                upload_to_receiver(
                    &pool,
                    &receiver_addr,
                    &fname,
                    fsize,
                    &fpath,
                    file,
                    sm_id,
                    #[cfg(feature = "desktop")] app_clone.clone(),
                ).await;
                // 上传完成 → 更新发送端 DB 状态
                let _ = crate::db::update_file_status_by_id(&pool, sm_id, "sent").await;
                // 通知发送端前端更新 UI
                let update = serde_json::json!({
                    "msg_type": "file_status_update",
                    "sender_msg_id": sm_id,
                    "file_status": "sent",
                });
                let _ = ws_tx.send(update.to_string());
                #[cfg(feature = "desktop")]
                if let Some(ref app_ref) = app_clone {
                    use tauri::Emitter;
                    let _ = app_ref.emit("new-message", update);
                }
            });

            (StatusCode::OK, Json(serde_json::json!({"success": true, "status": "sending"}))).into_response()
        }
        Err(e) => {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("查询文件记录失败: {}", e),
                "not_found": true,
            }))).into_response()
        }
    }
}

// 非 Tauri 环境下的文件上传（web 端用）
async fn upload_to_receiver(
    _pool: &sqlx::Pool<sqlx::Sqlite>,
    _receiver_addr: &str,
    _file_name: &str,
    _file_size: usize,
    _file_path: &str,
    file: tokio::fs::File,
    _sender_msg_id: i64,
    #[cfg(feature = "desktop")] _app: Option<tauri::AppHandle>,
) {
    // 获取自己的 ID
    let my_id = crate::db::get_user_id(_pool).await.unwrap_or_default();

    let file_size = _file_size;
    let file_name = _file_name.to_string();
    let peer_addr = _receiver_addr.to_string();

    // 分块上传到接收端
    let chunk_size = 4 * 1024 * 1024;
    let total_chunks = (file_size + chunk_size - 1) / chunk_size;

    let mut reader = tokio::io::BufReader::new(file);
    let mut chunk_index = 0usize;
    let mut offset: usize = 0;
    let start_time = std::time::Instant::now();
    let client = reqwest::Client::new();
    let upload_url = format!("http://{}/api/upload", peer_addr);

    loop {
        let mut buf = vec![0u8; chunk_size];
        let mut bytes_read = 0usize;
        while bytes_read < chunk_size {
            let n = match tokio::io::AsyncReadExt::read(&mut reader, &mut buf[bytes_read..]).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            bytes_read += n;
            if n == 0 { break; }
        }
        if bytes_read == 0 { break; }
        buf.truncate(bytes_read);

        let speed_mb_s = if chunk_index > 0 {
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                (offset as f64 / (1024.0 * 1024.0)) / elapsed
            } else { 0.0 }
        } else { 0.0 };

        let form = reqwest::multipart::Form::new()
            .text("peer_id", my_id.clone())
            .text("file_name", file_name.clone())
            .text("file_size", file_size.to_string())
            .text("chunk_index", chunk_index.to_string())
            .text("chunk_total", total_chunks.to_string())
            .text("sender_msg_id", _sender_msg_id.to_string())
            .text("speed_mb_s", format!("{:.1}", speed_mb_s))
            .part("chunk", reqwest::multipart::Part::bytes(buf).mime_str("application/octet-stream").unwrap());

        if let Ok(resp) = client.post(&upload_url).multipart(form).send().await {
            if resp.status().is_success() {
                println!("[WebServer] ✓ 分块 {}/{}", chunk_index + 1, total_chunks);
            }
        }
        offset += bytes_read;
        chunk_index += 1;

        // 发送进度到发送端前端
        let elapsed = start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            #[cfg(feature = "desktop")]
            if let Some(ref app_ref) = _app {
                use tauri::Emitter;
                let speed = offset as f64 / (1024.0 * 1024.0) / elapsed;
                let _ = app_ref.emit("upload_progress", serde_json::json!({
                    "file_name": _file_name,
                    "speed_mb_s": speed,
                    "sender_msg_id": _sender_msg_id,
                }));
            }
        }
    }

    println!("[WebServer] ✓ 文件上传完成: {}", file_name);
}

// ── 接收端通过本地服务器发送 file_request（POST /api/request_file） ──
#[derive(Deserialize)]
struct RequestFilePayload {
    message_id: Option<i64>,
    sender_msg_id: i64,
}

async fn request_file_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RequestFilePayload>,
) -> impl IntoResponse {
    println!("[手动下载] 接收端请求文件: msg_id={}", payload.sender_msg_id);

    match crate::workspace::request_incoming_file(
        &state.pool,
        payload.message_id,
        payload.sender_msg_id,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({"success": true})).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("发送 file_request 失败: {error}")
            })),
        )
            .into_response(),
    }
}

async fn update_name_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateNameRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 收到改名请求: {}", payload.name);

    // 使用数据库的更新函数（包含验证逻辑）
    match crate::db::update_username(&state.pool, payload.name.clone()).await {
        Ok(_) => {
            // 数据库更新后，定时广播线程会自动使用新名称
            println!("[Web Server] 用户名已更新，广播线程将使用新名称");

            Json(NameResponse { name: payload.name }).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response(),
    }
}

async fn get_peers_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // 不打印日志,避免刷屏
    let peers = state.peer_manager.get_all_peers();
    Json(peers).into_response()
}

async fn serve_index() -> impl IntoResponse {
    serve_assets(axum::extract::Path("index.html".to_string())).await
}

async fn serve_assets(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    match Asset::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("404"))
            .unwrap(),
    }
}

async fn send_message_http(
    State(state): State<Arc<AppState>>,
    Json(mut payload): Json<SendMessageRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 收到发送消息请求");

    // 获取自己的信息
    let my_id = match crate::db::get_user_id(&state.pool).await {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
                .into_response();
        }
    };

    let my_name = match crate::db::get_username(&state.pool).await {
        Ok(name) => name,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
                .into_response();
        }
    };

    // 检查用户状态的同时，执行后端 IP 二次校验
    let peers = state.peer_manager.get_all_peers();
    let mut is_online = false;

    if let Some(p) = peers.iter().find(|p| p.id == payload.peer_id) {
        if !p.is_offline {
            is_online = true;
            // 发现 IP 不一致，强行改写前端的请求载荷！
            if p.addr != payload.peer_addr {
                println!("[Web Server] 🛡️ 拦截到过期 Web 请求 IP，后端强行纠正: {} -> {}", payload.peer_addr, p.addr);
                payload.peer_addr = p.addr.clone();
            }
        }
    }

    if is_online {
        // 用户在线，尝试发送（这也是一种网络探测）
        match crate::network::messaging::send_text_message(
            &payload.peer_addr,
            my_id,
            my_name,
            payload.content.clone(),
        )
        .await
        {
            Ok(_) => {
                // 发送成功，保存到数据库(标记为已发送)
                if let Err(e) = crate::db::save_text_message_with_status(
                    &state.pool,
                    payload.peer_id,
                    payload.content,
                    "sent".to_string(),
                )
                .await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse { error: e }),
                    )
                        .into_response();
                }
            }
            Err(e) => {
                // 发送失败（探测到实际已离线或网络故障，比如 IP 刚变但心跳还没发）
                eprintln!(
                    "[Web Server] 发送失败(网络探测): {}. 消息将转入挂起队列。",
                    e
                );

                // 1. 立即更新 Web Server 内存中的状态，标记为离线
                state.peer_manager.force_mark_offline(&payload.peer_id);

                // 2. 保存为挂起状态 (pending)
                if let Err(db_e) = crate::db::save_text_message_with_status(
                    &state.pool,
                    payload.peer_id.clone(),
                    payload.content,
                    "pending".to_string(),
                )
                .await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse { error: db_e }),
                    )
                        .into_response();
                }
            }
        }
    } else {
        // 用户本来就在离线记录中，直接保存为挂起状态
        println!(
            "[Web Server] 用户 {} 离线，消息保存为挂起状态",
            payload.peer_id
        );
        if let Err(e) = crate::db::save_text_message_with_status(
            &state.pool,
            payload.peer_id,
            payload.content,
            "pending".to_string(),
        )
        .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
                .into_response();
        }
    }

    Json(serde_json::json!({ "success": true })).into_response()
}

async fn get_chat_history_http(
    State(state): State<Arc<AppState>>,
    Path(peer_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // 从查询参数获取 limit 和 offset
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(10);

    let offset = params
        .get("offset")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);

    match crate::network::messaging::get_chat_history_with_offset(
        &state.pool,
        &peer_id,
        limit,
        offset,
    )
    .await
    {
        Ok(messages) => Json(serde_json::json!({ "messages": messages })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

// WebSocket 处理器
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> axum::response::Response {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

fn broadcast_incoming_event(state: &AppState, value: serde_json::Value) {
    let _ = state.ws_broadcast.send(value.to_string());
    #[cfg(feature = "desktop")]
    if let Some(ref app) = state.app_handle {
        use tauri::Emitter;
        let _ = app.emit("new-message", value);
    }
}

fn wire_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

async fn send_delivery_ack(
    state: &AppState,
    conversation_id: &str,
    message_client_id: &str,
    author_id: &str,
    reader_id: &str,
    timestamp: i64,
) -> Result<(), String> {
    let peer_addr = state
        .peer_manager
        .get_all_peers()
        .into_iter()
        .find(|peer| peer.id == author_id && !peer.is_offline)
        .map(|peer| peer.addr)
        .ok_or_else(|| format!("消息作者 {} 当前不可达", author_id))?;
    let ack = crate::network::protocol::ProtocolMessage::DeliveryAck {
        conversation_id: conversation_id.to_string(),
        from_id: reader_id.to_string(),
        message_ids: vec![message_client_id.to_string()],
        timestamp: timestamp.max(0) as u64,
    };
    crate::network::protocol::send_protocol_message(&peer_addr, &ack).await?;
    crate::db::mark_receipt_ack_sent(&state.pool, message_client_id, reader_id, "delivery").await?;
    Ok(())
}

async fn record_local_delivery(
    state: &AppState,
    conversation_id: &str,
    message_client_id: &str,
    author_id: &str,
    reader_id: &str,
) -> Result<(), String> {
    crate::db::ensure_message_recipients(&state.pool, message_client_id, &[reader_id.to_string()])
        .await?;
    let receipt = crate::db::save_message_receipt(
        &state.pool,
        message_client_id,
        reader_id,
        Some(now_timestamp()),
        None,
    )
    .await?;

    if let Err(error) = send_delivery_ack(
        state,
        conversation_id,
        message_client_id,
        author_id,
        reader_id,
        receipt.delivered_at.unwrap_or_else(now_timestamp),
    )
    .await
    {
        // 已落库的 receipt 会在作者重新上线后由持久重试路径补发。
        eprintln!(
            "[WebSocket] delivery ack 暂未发送 (message={}): {}",
            message_client_id, error
        );
    }
    Ok(())
}

async fn apply_receipt_ack(
    state: &AppState,
    conversation_id: &str,
    from_id: &str,
    message_ids: &[String],
    timestamp: u64,
    is_read: bool,
) -> Result<(), String> {
    let my_id = crate::db::get_user_id(&state.pool).await?;
    let mut messages = Vec::with_capacity(message_ids.len());
    for message_id in message_ids {
        let message = crate::db::get_message_by_client_id(&state.pool, message_id)
            .await?
            .ok_or_else(|| format!("回执对应消息不存在: {}", message_id))?;
        if message.conversation_id.as_deref() != Some(conversation_id) {
            return Err(format!("回执会话不匹配: {}", message_id));
        }
        if message.sender_id != my_id && message.sender_id != "me" {
            return Err(format!("拒绝非本机消息的回执: {}", message_id));
        }
        let expected = crate::db::get_message_receipts(&state.pool, message_id)
            .await?
            .iter()
            .any(|receipt| receipt.reader_id == from_id);
        if !expected {
            return Err(format!("拒绝非目标设备的回执: {}", from_id));
        }
        messages.push(message);
    }

    let acknowledged_at = if timestamp == 0 {
        now_timestamp()
    } else {
        wire_i64(timestamp)
    };
    for (message_id, message) in message_ids.iter().zip(messages) {
        crate::db::save_message_receipt(
            &state.pool,
            message_id,
            from_id,
            Some(acknowledged_at),
            is_read.then_some(acknowledged_at),
        )
        .await?;
        if message.receiver_id.is_some() {
            crate::db::mark_message_status_by_client_id(
                &state.pool,
                message_id,
                if is_read { "read" } else { "delivered" },
            )
            .await?;
        }
    }
    Ok(())
}

async fn handle_protocol_message(
    state: &AppState,
    message: crate::network::protocol::ProtocolMessage,
) -> Result<(), String> {
    use crate::network::protocol::ProtocolMessage;

    match message {
        ProtocolMessage::GroupSync {
            group_id,
            title,
            created_by,
            members,
            version,
            timestamp,
        } => {
            let db_members = members
                .iter()
                .map(|member| crate::db::NewConversationMember {
                    peer_id: member.peer_id.clone(),
                    display_name: member.display_name.clone(),
                    role: member.role.clone(),
                })
                .collect::<Vec<_>>();
            let my_id = crate::db::get_user_id(&state.pool).await?;
            if !members.iter().any(|member| member.peer_id == my_id) {
                let existing = crate::db::get_conversation(&state.pool, &group_id).await?;
                if existing.as_ref().is_some_and(|conversation| {
                    conversation.kind == "group"
                        && conversation.created_by.as_deref() == Some(created_by.as_str())
                        && conversation.version < wire_i64(version)
                }) {
                    crate::db::delete_conversation(&state.pool, &group_id).await?;
                    broadcast_incoming_event(
                        state,
                        serde_json::json!({ "msg_type": "group_removed", "group_id": group_id }),
                    );
                    return Ok(());
                }
                return Err("忽略未包含本机的群同步".to_string());
            }
            let conversation = crate::db::apply_group_sync(
                &state.pool,
                &group_id,
                &title,
                &created_by,
                wire_i64(version),
                &db_members,
            )
            .await?;
            broadcast_incoming_event(
                state,
                serde_json::json!({
                    "msg_type": "group_sync",
                    "group_id": group_id,
                    "title": title,
                    "created_by": created_by,
                    "members": members,
                    "version": version,
                    "timestamp": timestamp,
                    "conversation": conversation,
                }),
            );
        }
        ProtocolMessage::GroupMessage {
            group_id,
            client_message_id,
            from_id,
            from_name,
            content,
            content_type,
            mention_ids,
            timestamp,
        } => {
            if !matches!(content_type.as_str(), "text" | "file" | "quote" | "announcement") {
                return Err(format!("不支持的群消息类型: {}", content_type));
            }
            let members = crate::db::get_conversation_members(&state.pool, &group_id).await?;
            if !members.iter().any(|member| member.peer_id == from_id) {
                return Err(format!("群消息发送者不在群成员中: {}", from_id));
            }
            let my_id = crate::db::get_user_id(&state.pool).await?;
            if !members.iter().any(|member| member.peer_id == my_id) {
                return Err("本机不在该群成员中".to_string());
            }
            let mention_ids = mention_ids
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            if let Some(mention_id) = mention_ids
                .iter()
                .find(|mention_id| {
                    mention_id.as_str() == from_id.as_str()
                        || !members
                            .iter()
                            .any(|member| member.peer_id == mention_id.as_str())
                })
            {
                return Err(format!("群消息 @ 目标无效: {mention_id}"));
            }
            let mention_ids = mention_ids.into_iter().collect::<Vec<_>>();
            let recipients = members
                .iter()
                .map(|member| member.peer_id.clone())
                .filter(|peer_id| peer_id != &from_id)
                .collect::<Vec<_>>();

            let write_guard = GROUP_MESSAGE_WRITE_LOCK.lock().await;
            let existing = crate::db::get_message_by_client_id(&state.pool, &client_message_id)
                .await?;
            let is_new = existing.is_none();
            let stored = if let Some(stored) = existing {
                if stored.status.as_deref() == Some("recalled") {
                    if stored.conversation_id.as_deref() == Some(group_id.as_str())
                        && stored.sender_id == from_id
                    {
                        return Ok(());
                    }
                    return Err("client message id conflicts with another message".to_string());
                }
                if stored.conversation_id.as_deref() != Some(group_id.as_str())
                    || stored.sender_id != from_id
                    || stored.content != content
                    || stored.msg_type != content_type
                {
                    return Err("client message id conflicts with another message".to_string());
                }
                let stored_mentions = crate::db::get_message_receipts(
                    &state.pool,
                    &client_message_id,
                )
                .await?
                .into_iter()
                .filter(|receipt| receipt.mentioned)
                .map(|receipt| receipt.reader_id)
                .collect::<std::collections::BTreeSet<_>>();
                if stored_mentions != mention_ids.iter().cloned().collect() {
                    return Err("client message id conflicts with another mention set".to_string());
                }
                stored
            } else {
                let stored = crate::db::save_conversation_message(
                    &state.pool,
                    &group_id,
                    &from_id,
                    None,
                    &content,
                    &content_type,
                    wire_i64(timestamp),
                    "delivered",
                    &client_message_id,
                )
                .await?;
                crate::db::ensure_message_recipients(
                    &state.pool,
                    &client_message_id,
                    &recipients,
                )
                .await?;
                crate::db::mark_message_mentions(&state.pool, &client_message_id, &mention_ids)
                    .await?;
                stored
            };
            drop(write_guard);
            record_local_delivery(state, &group_id, &client_message_id, &from_id, &my_id).await?;
            if is_new {
                broadcast_incoming_event(
                    state,
                    serde_json::json!({
                        "id": stored.id,
                        "conversation_id": group_id,
                        "client_message_id": client_message_id,
                        "from_id": from_id,
                        "sender_id": stored.sender_id,
                        "from_name": from_name,
                        "content": stored.content,
                        "timestamp": stored.timestamp,
                        "msg_type": stored.msg_type,
                        "wire_msg_type": "group_message",
                        "status": stored.status,
                        "mention_ids": mention_ids,
                    }),
                );
            }
        }
        ProtocolMessage::MessageReaction {
            conversation_id,
            client_message_id,
            from_id,
            emoji,
            timestamp,
        } => {
            let message = crate::db::get_message_by_client_id(&state.pool, &client_message_id)
                .await?
                .ok_or_else(|| "reaction target message not found".to_string())?;
            if message.conversation_id.as_deref() != Some(conversation_id.as_str()) {
                return Err("reaction target does not belong to conversation".to_string());
            }
            let members = crate::db::get_conversation_members(&state.pool, &conversation_id).await?;
            if !members.iter().any(|member| member.peer_id == from_id) {
                return Err("reaction sender is not a conversation member".to_string());
            }
            crate::db::save_message_reaction(
                &state.pool,
                &client_message_id,
                &from_id,
                &emoji,
            )
            .await?;
            broadcast_incoming_event(
                state,
                serde_json::json!({
                    "msg_type": "message_reaction",
                    "conversation_id": conversation_id,
                    "client_message_id": client_message_id,
                    "from_id": from_id,
                    "emoji": emoji,
                    "timestamp": timestamp,
                }),
            );
        }
        ProtocolMessage::StrongReminder {
            conversation_id,
            client_message_id,
            from_id,
            from_name,
            summary,
            timestamp,
        } => {
            let message = crate::db::get_message_by_client_id(&state.pool, &client_message_id)
                .await?
                .ok_or_else(|| "strong reminder target message not found".to_string())?;
            if message.conversation_id.as_deref() != Some(conversation_id.as_str())
                || message.sender_id != from_id
            {
                return Err("strong reminder sender does not own the target message".to_string());
            }
            broadcast_incoming_event(
                state,
                serde_json::json!({
                    "msg_type": "strong_reminder",
                    "conversation_id": conversation_id,
                    "client_message_id": client_message_id,
                    "from_id": from_id,
                    "from_name": from_name,
                    "summary": summary,
                    "timestamp": timestamp,
                }),
            );
        }
        ProtocolMessage::MessageRecall {
            conversation_id,
            client_message_id,
            from_id,
            timestamp,
        } => {
            if let Some(message) =
                crate::db::get_message_by_client_id(&state.pool, &client_message_id).await?
            {
                if message.conversation_id.as_deref() != Some(conversation_id.as_str())
                    || message.sender_id != from_id
                {
                    return Err("撤回消息身份不匹配".to_string());
                }
            } else {
                let conversation = crate::db::get_conversation(&state.pool, &conversation_id)
                    .await?
                    .ok_or_else(|| "会话不存在".to_string())?;
                let valid_sender = if conversation.kind == "group" {
                    crate::db::get_conversation_members(&state.pool, &conversation_id)
                        .await?
                        .iter()
                        .any(|member| member.peer_id == from_id)
                } else {
                    conversation.peer_id.as_deref() == Some(from_id.as_str())
                };
                if !valid_sender {
                    return Err("撤回消息身份不匹配".to_string());
                }
            }
            crate::db::store_recalled_tombstone(
                &state.pool,
                &conversation_id,
                &client_message_id,
                &from_id,
                wire_i64(timestamp),
            )
            .await?;
            broadcast_incoming_event(
                state,
                serde_json::json!({
                    "msg_type": "message_recall",
                    "conversation_id": conversation_id,
                    "client_message_id": client_message_id,
                }),
            );
        }
        ProtocolMessage::DeliveryAck {
            conversation_id,
            from_id,
            message_ids,
            timestamp,
        } => {
            apply_receipt_ack(
                state,
                &conversation_id,
                &from_id,
                &message_ids,
                timestamp,
                false,
            )
            .await?;
            broadcast_incoming_event(
                state,
                serde_json::json!({
                    "msg_type": "delivery_ack",
                    "conversation_id": conversation_id,
                    "from_id": from_id,
                    "message_ids": message_ids,
                    "timestamp": timestamp,
                }),
            );
        }
        ProtocolMessage::ReadAck {
            conversation_id,
            from_id,
            message_ids,
            timestamp,
        } => {
            apply_receipt_ack(
                state,
                &conversation_id,
                &from_id,
                &message_ids,
                timestamp,
                true,
            )
            .await?;
            broadcast_incoming_event(
                state,
                serde_json::json!({
                    "msg_type": "read_ack",
                    "conversation_id": conversation_id,
                    "from_id": from_id,
                    "message_ids": message_ids,
                    "timestamp": timestamp,
                }),
            );
        }
    }
    Ok(())
}

async fn handle_stable_direct_message(
    state: &AppState,
    message: crate::network::messaging::TextMessage,
) -> Result<bool, String> {
    let (Some(conversation_id), Some(client_message_id)) = (
        message.conversation_id.as_deref(),
        message.client_message_id.as_deref(),
    ) else {
        return Ok(false);
    };
    if matches!(message.msg_type.as_str(), "message_reaction" | "strong_reminder") {
        let my_id = crate::db::get_user_id(&state.pool).await?;
        let expected_conversation_id =
            crate::db::stable_direct_conversation_id(&my_id, &message.from_id);
        if conversation_id != expected_conversation_id {
            return Err("direct control conversation does not match sender".to_string());
        }
        let target = crate::db::get_message_by_client_id(&state.pool, client_message_id)
            .await?
            .ok_or_else(|| "control target message not found".to_string())?;
        if target.conversation_id.as_deref() != Some(conversation_id) {
            return Err("control target does not belong to conversation".to_string());
        }
        let control = serde_json::from_str::<serde_json::Value>(&message.content)
            .map_err(|error| format!("invalid message control: {error}"))?;
        if message.msg_type == "strong_reminder" && target.sender_id != message.from_id {
            return Err("strong reminder sender does not own target message".to_string());
        }
        if message.msg_type == "message_reaction" {
            let emoji = control
                .get("emoji")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "reaction emoji is missing".to_string())?;
            crate::db::save_message_reaction(
                &state.pool,
                client_message_id,
                &message.from_id,
                emoji,
            )
            .await?;
        }
        broadcast_incoming_event(
            state,
            serde_json::json!({
                "msg_type": message.msg_type,
                "conversation_id": conversation_id,
                "client_message_id": client_message_id,
                "from_id": message.from_id,
                "from_name": message.from_name,
                "emoji": control.get("emoji").and_then(|value| value.as_str()),
                "summary": control.get("summary").and_then(|value| value.as_str()),
                "timestamp": message.timestamp,
            }),
        );
        return Ok(true);
    }
    if !matches!(message.msg_type.as_str(), "text" | "quote" | "announcement")
        || message.from_id.trim().is_empty()
        || conversation_id.trim().is_empty()
        || client_message_id.trim().is_empty()
    {
        return Err("稳定单聊消息字段无效".to_string());
    }

    let my_id = crate::db::get_user_id(&state.pool).await?;
    let expected_conversation_id =
        crate::db::stable_direct_conversation_id(&my_id, &message.from_id);
    if conversation_id != expected_conversation_id {
        return Err("稳定单聊会话 ID 与发送者不匹配".to_string());
    }
    crate::db::ensure_direct_conversation(&state.pool, &message.from_id).await?;
    if let Some(existing) = crate::db::get_message_by_client_id(&state.pool, client_message_id).await?
    {
        if existing.status.as_deref() == Some("recalled") {
            if existing.conversation_id.as_deref() == Some(conversation_id)
                && existing.sender_id == message.from_id
            {
                return Ok(true);
            }
            return Err("client message id conflicts with another message".to_string());
        }
    }
    let stored = crate::db::save_conversation_message(
        &state.pool,
        conversation_id,
        &message.from_id,
        Some(&my_id),
        &message.content,
        &message.msg_type,
        wire_i64(message.timestamp),
        "delivered",
        client_message_id,
    )
    .await?;
    record_local_delivery(
        state,
        conversation_id,
        client_message_id,
        &message.from_id,
        &my_id,
    )
    .await?;
    broadcast_incoming_event(
        state,
        serde_json::json!({
            "id": stored.id,
            "conversation_id": conversation_id,
            "client_message_id": client_message_id,
            "from_id": message.from_id,
            "sender_id": stored.sender_id,
            "from_name": message.from_name,
            "content": stored.content,
            "timestamp": stored.timestamp,
            "msg_type": stored.msg_type,
            "status": stored.status,
        }),
    );
    Ok(true)
}

// 处理 WebSocket 连接
async fn handle_websocket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    println!("[WebSocket] 新的 WebSocket 连接");

    // 订阅广播频道并转发给此 WebSocket 客户端
    let mut broadcast_rx: broadcast::Receiver<String> = state.ws_broadcast.subscribe();
    let forward_handle = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    if sender.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => {
                    // Lagged（滞后）或 Closed（不会发生）：继续接收
                    continue;
                }
            }
        }
    });

    // 接收消息
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                println!("[WebSocket] 收到文本消息: {}", text);

                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(val) => {
                        match crate::network::protocol::parse_protocol_value(val.clone()) {
                            Ok(Some(message)) => {
                                if let Err(error) =
                                    handle_protocol_message(state.as_ref(), message).await
                                {
                                    eprintln!("[WebSocket] 新协议消息处理失败: {}", error);
                                }
                                continue;
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("[WebSocket] 新协议消息无效: {}", error);
                                continue;
                            }
                        }

                        let stream_id = val
                            .get("stream_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let stream_final = val
                            .get("stream_final")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        let msg_type = val
                            .get("msg_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("text")
                            .to_string();

                        if msg_type == "text"
                            && (val.get("conversation_id").is_some()
                                || val.get("client_message_id").is_some())
                        {
                            match serde_json::from_value::<crate::network::messaging::TextMessage>(
                                val.clone(),
                            ) {
                                Ok(message) => {
                                    match handle_stable_direct_message(state.as_ref(), message)
                                        .await
                                    {
                                        Ok(true) => {}
                                        Ok(false) => {
                                            eprintln!(
                                                "[WebSocket] 稳定单聊消息缺少 conversation_id 或 client_message_id"
                                            );
                                        }
                                        Err(error) => {
                                            eprintln!(
                                                "[WebSocket] 稳定单聊消息处理失败: {}",
                                                error
                                            );
                                        }
                                    }
                                }
                                Err(error) => {
                                    eprintln!("[WebSocket] 稳定单聊消息无效: {}", error);
                                }
                            }
                            continue;
                        }

                        if let Some(ref sid) = stream_id {
                            // ── 流式消息 ──
                            // 构建广播消息（保留原字段，统一注入 is_streaming）
                            let mut broadcast_val = val.clone();
                            if let Some(obj) = broadcast_val.as_object_mut() {
                                obj.insert(
                                    "is_streaming".to_string(),
                                    serde_json::json!(!stream_final),
                                );
                                obj.remove("stream_final");
                            }
                            let broadcast_msg = broadcast_val.to_string();

                            // 流式消息不存 DB（除了 text 类型的 stream_final）
                            if stream_final && msg_type == "text" {
                                if let Ok(message) = serde_json::from_value::<
                                    crate::network::messaging::TextMessage,
                                >(val.clone())
                                {
                                    match save_message_to_db(&state.pool, &message).await {
                                        Ok(msg_id) => {
                                            println!(
                                                "[WebSocket] 流式完成: {} (id={})",
                                                message
                                                    .content
                                                    .chars()
                                                    .take(40)
                                                    .collect::<String>(),
                                                msg_id
                                            );
                                            // 重新广播（带 id）
                                            let mut final_val = val.clone();
                                            if let Some(obj) = final_val.as_object_mut() {
                                                obj.insert(
                                                    "id".to_string(),
                                                    serde_json::json!(msg_id),
                                                );
                                                obj.insert(
                                                    "is_streaming".to_string(),
                                                    serde_json::json!(false),
                                                );
                                                obj.remove("stream_final");
                                            }
                                            let final_msg = final_val.to_string();
                                            let _ = state.ws_broadcast.send(final_msg);
                                            #[cfg(feature = "desktop")]
                                            if let Some(ref app) = state.app_handle {
                                                use tauri::Emitter;
                                                let _ = app.emit("new-message", final_val);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("[WebSocket] 保存流式最终消息失败: {}", e)
                                        }
                                    }
                                }
                            } else {
                                // 非 final 或非 text 类型：直接广播
                                println!("[WebSocket] 流式块 ({}, stream={})", msg_type, sid);
                                let _ = state.ws_broadcast.send(broadcast_msg.clone());
                                #[cfg(feature = "desktop")]
                                if let Some(ref app) = state.app_handle {
                                    use tauri::Emitter;
                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&broadcast_msg) {
                                        let _ = app.emit("new-message", v);
                                    }
                                }
                            }
                        } else if msg_type == "file_offer" {
                            // ── 收到文件邀请 ──
                            let file_name = val.get("file_name").and_then(|v| v.as_str()).unwrap_or("unknown");
                            let file_size = val.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0);
                            let from_id = val.get("from_id").and_then(|v| v.as_str()).unwrap_or("");
                            let sender_msg_id = val.get("sender_msg_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            let from_name = val.get("from_name").and_then(|v| v.as_str()).unwrap_or("").to_string();

                            let auto_dl = crate::db::get_auto_download(&state.pool).await;
                            if auto_dl {
                                // 自动下载开启 → 通知发送端开始上传
                                let accept = serde_json::json!({
                                    "msg_type": "file_accept",
                                    "sender_msg_id": sender_msg_id,
                                });
                                // 需要知道对方的地址来回复，用 from_id 查找 peer
                                let peer_addr = state.peer_manager.get_all_peers().iter()
                                    .find(|p| p.id == from_id)
                                    .map(|p| p.addr.clone());
                                if let Some(addr) = peer_addr {
                                    let _ = crate::network::messaging::send_json_via_ws(
                                        &addr, &accept.to_string(),
                                    ).await;
                                }
                            } else {
                                // 自动下载关闭 → 创建 offered 记录并通知前端
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
                                if let Ok(msg_id) = crate::db::create_offered_file_record(
                                    &state.pool,
                                    from_id,
                                    file_name,
                                    file_size,
                                    &sender_msg_id.to_string(),
                                    timestamp,
                                ).await {
                                    let broadcast_msg = serde_json::json!({
                                        "id": msg_id,
                                        "from_id": from_id,
                                        "from_name": from_name,
                                        "content": file_name,
                                        "msg_type": "file",
                                        "file_status": "offered",
                                        "file_size": file_size,
                                        "sender_msg_id": sender_msg_id,
                                        "timestamp": timestamp,
                                    });
                                    let _ = state.ws_broadcast.send(broadcast_msg.to_string());
                                    #[cfg(feature = "desktop")]
                                    if let Some(ref app) = state.app_handle {
                                        use tauri::Emitter;
                                        let _ = app.emit("new-message", broadcast_msg);
                                    }
                                }
                            }
                        } else if msg_type == "file_accept" {
                            // ── 对方确认接受（auto_download=ON），开始上传 ──
                            let sender_msg_id = val.get("sender_msg_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            eprintln!("[WebSocket] 收到 file_accept for msg_id={}, 开始上传", sender_msg_id);
                            // 更新 DB 状态
                            let _ = crate::db::update_file_status_by_id(&state.pool, sender_msg_id, "uploading").await;

                        } else if msg_type == "file_request" {
                            // ── 对方请求开始发送文件（手动下载） ──
                            let sender_msg_id = val.get("sender_msg_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            let from_id = val.get("from_id").and_then(|v| v.as_str()).unwrap_or("");
                            println!("[手动下载] 发送端收到下载请求: msg_id={}, from={}", sender_msg_id, from_id);

                            // 从 peer manager 查找对方地址
                            let receiver_addr = state.peer_manager.get_all_peers().iter()
                                .find(|p| p.id == from_id)
                                .map(|p| p.addr.clone())
                                .unwrap_or_default();
                            let parallel_v2 = state.peer_manager.get_all_peers().iter()
                                .find(|p| p.id == from_id)
                                .is_some_and(|peer| peer.capabilities.iter().any(
                                    |capability| capability
                                        == crate::network::conversation_file::PARALLEL_FILE_CAPABILITY,
                                ));

                            if receiver_addr.is_empty() {
                                eprintln!("[WebSocket] file_request: 找不到对方地址");
                                break;
                            }

                            let stable_message = crate::db::get_file_message_by_id(
                                &state.pool,
                                sender_msg_id,
                            )
                            .await
                            .unwrap_or(None)
                            .filter(|message| {
                                message.msg_type == "file"
                                    && message.client_message_id.is_some()
                                    && message.conversation_id.is_some()
                            });
                            if stable_message.is_some() {
                                match crate::network::conversation_file::resume_transfer(
                                    &state.pool,
                                    sender_msg_id,
                                    from_id,
                                    &receiver_addr,
                                    parallel_v2,
                                )
                                .await
                                {
                                    Ok(transfer) => {
                                        let update = serde_json::json!({
                                            "msg_type": "file_status_update",
                                            "sender_msg_id": sender_msg_id,
                                            "file_status": "queued",
                                            "transfer_id": transfer.id,
                                        });
                                        let _ = state.ws_broadcast.send(update.to_string());
                                        #[cfg(feature = "desktop")]
                                        if let Some(ref app) = state.app_handle {
                                            use tauri::Emitter;
                                            let _ = app.emit("new-message", update);
                                        }
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "[WebSocket] stable file_request 恢复失败: {}",
                                            error
                                        );
                                    }
                                }
                                continue;
                            }

                            // 查询发送端文件记录
                            match crate::db::get_sender_file_by_msg_id(&state.pool, sender_msg_id).await {
                                Ok((file_path, fname, fsize)) => {
                                    // 立即更新为 uploading，前端显示「上传中」
                                    let _ = crate::db::update_file_status_by_id(
                                        &state.pool, sender_msg_id, "uploading",
                                    ).await;
                                    let update = serde_json::json!({
                                        "msg_type": "file_status_update",
                                        "sender_msg_id": sender_msg_id,
                                        "file_status": "uploading",
                                    });
                                    let _ = state.ws_broadcast.send(update.to_string());
                                    #[cfg(feature = "desktop")]
                                    if let Some(ref app) = state.app_handle {
                                        use tauri::Emitter;
                                        let _ = app.emit("new-message", update);
                                    }

                                    if file_path.is_empty() {
                                        // ── Web 端：文件在浏览器中，通知浏览器上传 ──
                                        println!("[手动下载] Web端文件在浏览器中，通知浏览器上传: msg_id={}", sender_msg_id);
                                        let start_evt = serde_json::json!({
                                            "msg_type": "start_upload",
                                            "sender_msg_id": sender_msg_id,
                                            "file_name": fname,
                                            "file_size": fsize,
                                            "receiver_addr": receiver_addr,
                                        });
                                        let _ = state.ws_broadcast.send(start_evt.to_string());
                                        #[cfg(feature = "desktop")]
                                        if let Some(ref app) = state.app_handle {
                                            use tauri::Emitter;
                                            let _ = app.emit("new-message", start_evt);
                                        }
                                    } else if file_path.starts_with("content://") {
                                        // ── Android SAF 文件（URI 已持久化权限） ──
                                        println!("[手动下载] Android content URI 文件: msg_id={}, uri={}", sender_msg_id, file_path);
                                        #[cfg(target_os = "android")]
                                        {
                                            use crate::android_fd::AndroidFile;
                                            match AndroidFile::from_content_uri(&file_path) {
                                                Ok(af) => {
                                                    let file = tokio::fs::File::from_std(af.into_file());
                                                    let pool = state.pool.clone();
                                                    let raddr = receiver_addr.clone();
                                                    let ws_tx = state.ws_broadcast.clone();
                                                    #[cfg(feature = "desktop")]
                                                    let app_clone = state.app_handle.clone();
                                                    let fname2 = fname.clone();
                                                    tokio::spawn(async move {
                                                        upload_to_receiver(
                                                            &pool, &raddr, &fname2, fsize as usize, &file_path, file,
                                                            sender_msg_id,
                                                            #[cfg(feature = "desktop")] app_clone.clone(),
                                                        ).await;
                                                        let _ = crate::db::update_file_status_by_id(
                                                            &pool, sender_msg_id, "sent",
                                                        ).await;
                                                        let update = serde_json::json!({
                                                            "msg_type": "file_status_update",
                                                            "sender_msg_id": sender_msg_id,
                                                            "file_status": "sent",
                                                        });
                                                        let _ = ws_tx.send(update.to_string());
                                                        #[cfg(feature = "desktop")]
                                                        if let Some(ref app_ref) = app_clone {
                                                            use tauri::Emitter;
                                                            let _ = app_ref.emit("new-message", update);
                                                        }
                                                    });
                                                }
                                                Err(e) => {
                                                    eprintln!("[手动下载] content URI 打开失败: {}", e);
                                                    let notice = serde_json::json!({
                                                        "msg_type": "file_not_found",
                                                        "sender_msg_id": sender_msg_id,
                                                    });
                                                    let _ = crate::network::messaging::send_json_via_ws(
                                                        &receiver_addr,
                                                        &notice.to_string(),
                                                    ).await;
                                                }
                                            }
                                        }
                                        #[cfg(not(target_os = "android"))]
                                        {
                                            let notice = serde_json::json!({
                                                "msg_type": "file_not_found",
                                                "sender_msg_id": sender_msg_id,
                                            });
                                            let _ = crate::network::messaging::send_json_via_ws(
                                                &receiver_addr,
                                                &notice.to_string(),
                                            ).await;
                                        }
                                    } else if file_path.starts_with("fd:") {
                                        // ── Android Share Intent 文件（FD 缓存） ──
                                        println!("[手动下载] Share Intent FD 缓存文件: msg_id={}", sender_msg_id);
                                        #[cfg(target_os = "android")]
                                        {
                                            match crate::android_fd::duplicate_cached_file(sender_msg_id) {
                                                Some((file, cached_name, cached_size)) => {
                                                    let pool = state.pool.clone();
                                                    let raddr = receiver_addr.clone();
                                                    let ws_tx = state.ws_broadcast.clone();
                                                    #[cfg(feature = "desktop")]
                                                    let app_clone = state.app_handle.clone();
                                                    let fname2 = cached_name;
                                                    tokio::spawn(async move {
                                                        upload_to_receiver(
                                                            &pool, &raddr, &fname2, cached_size as usize, &file_path, file,
                                                            sender_msg_id,
                                                            #[cfg(feature = "desktop")] app_clone.clone(),
                                                        ).await;
                                                        let _ = crate::db::update_file_status_by_id(
                                                            &pool, sender_msg_id, "sent",
                                                        ).await;
                                                        let update = serde_json::json!({
                                                            "msg_type": "file_status_update",
                                                            "sender_msg_id": sender_msg_id,
                                                            "file_status": "sent",
                                                        });
                                                        let _ = ws_tx.send(update.to_string());
                                                        #[cfg(feature = "desktop")]
                                                        if let Some(ref app_ref) = app_clone {
                                                            use tauri::Emitter;
                                                            let _ = app_ref.emit("new-message", update);
                                                        }
                                                    });
                                                }
                                                None => {
                                                    eprintln!("[手动下载] FD 缓存未命中 (app 可能已被杀): msg_id={}", sender_msg_id);
                                                    let notice = serde_json::json!({
                                                        "msg_type": "file_not_found",
                                                        "sender_msg_id": sender_msg_id,
                                                    });
                                                    let _ = crate::network::messaging::send_json_via_ws(
                                                        &receiver_addr,
                                                        &notice.to_string(),
                                                    ).await;
                                                }
                                            }
                                        }
                                        #[cfg(not(target_os = "android"))]
                                        {
                                            let notice = serde_json::json!({
                                                "msg_type": "file_not_found",
                                                "sender_msg_id": sender_msg_id,
                                            });
                                            let _ = crate::network::messaging::send_json_via_ws(
                                                &receiver_addr,
                                                &notice.to_string(),
                                            ).await;
                                        }
                                    } else if !std::path::Path::new(&file_path).exists() {
                                        // ── 普通文件不存在 ──
                                        println!("[手动下载] 文件已丢失: msg_id={}, path={}", sender_msg_id, file_path);
                                        let notice = serde_json::json!({
                                            "msg_type": "file_not_found",
                                            "sender_msg_id": sender_msg_id,
                                        });
                                        let _ = crate::network::messaging::send_json_via_ws(
                                            &receiver_addr,
                                            &notice.to_string(),
                                        ).await;
                                    } else {
                                        // ── 普通文件存在，开始上传 ──
                                        println!("[手动下载] 文件存在，开始上传: msg_id={}, file={}, to={}", sender_msg_id, file_path, receiver_addr);
                                        match tokio::fs::File::open(&file_path).await {
                                            Ok(file) => {
                                                let pool = state.pool.clone();
                                                let raddr = receiver_addr.clone();
                                                let ws_tx = state.ws_broadcast.clone();
                                                #[cfg(feature = "desktop")]
                                                let app_clone = state.app_handle.clone();
                                                let fname2 = fname.clone();
                                                tokio::spawn(async move {
                                                    upload_to_receiver(
                                                        &pool, &raddr, &fname2, fsize as usize, &file_path, file,
                                                        sender_msg_id,
                                                        #[cfg(feature = "desktop")] app_clone.clone(),
                                                    ).await;
                                                    // 上传完成 → 更新发送端 DB 状态
                                                    let _ = crate::db::update_file_status_by_id(
                                                        &pool, sender_msg_id, "sent",
                                                    ).await;
                                                    // 通知发送端前端更新 UI
                                                    let update = serde_json::json!({
                                                        "msg_type": "file_status_update",
                                                        "sender_msg_id": sender_msg_id,
                                                        "file_status": "sent",
                                                    });
                                                    let _ = ws_tx.send(update.to_string());
                                                    #[cfg(feature = "desktop")]
                                                    if let Some(ref app_ref) = app_clone {
                                                        use tauri::Emitter;
                                                        let _ = app_ref.emit("new-message", update);
                                                    }
                                                });
                                            }
                                            Err(_) => {
                                                println!("[手动下载] 打开文件失败(打开时错误): msg_id={}, path={}", sender_msg_id, file_path);
                                                let notice = serde_json::json!({
                                                    "msg_type": "file_not_found",
                                                    "sender_msg_id": sender_msg_id,
                                                });
                                                let _ = crate::network::messaging::send_json_via_ws(
                                                    &receiver_addr,
                                                    &notice.to_string(),
                                                ).await;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("[手动下载] 查询文件记录失败: msg_id={}, err={}", sender_msg_id, e);
                                    let notice = serde_json::json!({
                                        "msg_type": "file_not_found",
                                        "sender_msg_id": sender_msg_id,
                                    });
                                    let _ = crate::network::messaging::send_json_via_ws(
                                        &receiver_addr,
                                        &notice.to_string(),
                                    ).await;
                                }
                            }
                        } else if msg_type == "file_not_found" {
                            // ── 发送端告知文件已丢失 ──
                            let sender_msg_id = val.get("sender_msg_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            println!("[WebSocket] 收到 file_not_found: msg_id={}", sender_msg_id);
                            let _ = crate::db::update_file_status_by_sender_msg_id(
                                &state.pool,
                                &sender_msg_id.to_string(),
                                "invalid",
                            ).await;

                            // 通知前端更新状态
                            let update = serde_json::json!({
                                "msg_type": "file_status_update",
                                "sender_msg_id": sender_msg_id,
                                "file_status": "invalid",
                            });
                            let _ = state.ws_broadcast.send(update.to_string());
                            #[cfg(feature = "desktop")]
                            if let Some(ref app) = state.app_handle {
                                use tauri::Emitter;
                                let _ = app.emit("new-message", update);
                            }
                        } else {
                            // ── 非流式消息：尝试作为 TextMessage 存 DB ──
                            if let Ok(message) = serde_json::from_value::<crate::network::messaging::TextMessage>(val.clone()) {
                                match save_message_to_db(&state.pool, &message).await {
                                    Ok(msg_id) => {
                                        println!("[WebSocket] 消息已保存: {} 说: {} (id={})", message.from_name, message.content, msg_id);
                                        let broadcast_msg = serde_json::json!({
                                            "id": msg_id,
                                            "from_id": message.from_id,
                                            "from_name": message.from_name,
                                            "content": message.content,
                                            "timestamp": message.timestamp,
                                            "msg_type": message.msg_type,
                                        }).to_string();
                                        let _ = state.ws_broadcast.send(broadcast_msg);
                                        #[cfg(feature = "desktop")]
                                        if let Some(ref app) = state.app_handle {
                                            use tauri::Emitter;
                                            let _ = app.emit(
                                                "new-message",
                                                serde_json::json!({
                                                    "id": msg_id,
                                                    "from_id": message.from_id,
                                                    "from_name": message.from_name,
                                                    "content": message.content,
                                                    "timestamp": message.timestamp,
                                                    "msg_type": message.msg_type,
                                                }),
                                            );
                                        }
                                    }
                                    Err(e) => eprintln!("[WebSocket] 保存消息失败: {}", e)
                                }
                            } else {
                                eprintln!("[WebSocket] 无法解析消息: {}", &text[..text.len().min(100)]);
                            }
                        }
                    }
                    Err(e) => eprintln!("[WebSocket] JSON 解析失败: {}", e)
                }
            }
            Ok(Message::Close(_)) => {
                println!("[WebSocket] 连接关闭");
                break;
            }
            Err(e) => {
                eprintln!("[WebSocket] 错误: {}", e);
                break;
            }
            _ => {}
        }
    }

    forward_handle.abort();
}

// 保存消息到数据库
async fn save_message_to_db(
    pool: &Pool<Sqlite>,
    message: &crate::network::messaging::TextMessage,
) -> Result<i64, String> {
    crate::db::save_received_text_message(
        pool,
        message.from_id.clone(),
        message.content.clone(),
        message.msg_type.clone(),
        message.timestamp as i64,
    )
    .await
}

async fn validate_incoming_file_conversation(
    state: &AppState,
    conversation_id: &str,
    sender_id: &str,
) -> Result<crate::db::ConversationRecord, String> {
    let self_id = crate::db::get_user_id(&state.pool).await?;
    if sender_id.trim().is_empty() || sender_id == self_id || sender_id == "me" {
        return Err("无效的文件发送者".to_string());
    }
    let expected_direct = crate::db::stable_direct_conversation_id(&self_id, sender_id);
    let is_expected_direct = conversation_id == expected_direct;
    if is_expected_direct {
        crate::db::ensure_direct_conversation(&state.pool, sender_id).await?;
    }
    for attempt in 0..=10 {
        if let Some(conversation) =
            crate::db::get_conversation(&state.pool, conversation_id).await?
        {
            let members =
                crate::db::get_conversation_members(&state.pool, conversation_id).await?;
            let has_members = members.iter().any(|member| member.peer_id == self_id)
                && members.iter().any(|member| member.peer_id == sender_id);
            match conversation.kind.as_str() {
                "direct"
                    if is_expected_direct
                        && has_members
                        && conversation.peer_id.as_deref() == Some(sender_id) =>
                {
                    return Ok(conversation);
                }
                "group" if has_members => return Ok(conversation),
                "group" if attempt < 10 => {}
                _ => return Err("文件会话与发送者不匹配".to_string()),
            }
        } else if is_expected_direct || attempt == 10 {
            return Err("conversation not found".to_string());
        }
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    }
    Err("发送者不是会话成员".to_string())
}

async fn available_received_file_name(
    download_root: &std::path::Path,
    requested: &str,
) -> Result<String, String> {
    let path = std::path::Path::new(requested);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    for index in 0..=9999 {
        let name = if index == 0 {
            requested.to_string()
        } else {
            format!("{stem}({index}){extension}")
        };
        let complete = download_root.join(&name);
        let partial = download_root.join(format!("{name}.downloading"));
        let complete_exists = tokio::fs::try_exists(&complete)
            .await
            .map_err(|error| format!("检查目标文件失败: {error}"))?;
        let partial_exists = tokio::fs::try_exists(&partial)
            .await
            .map_err(|error| format!("检查临时文件失败: {error}"))?;
        if !complete_exists && !partial_exists {
            return Ok(name);
        }
    }
    Err("同名文件过多，无法分配安全文件名".to_string())
}

async fn finalize_received_file(
    download_root: &std::path::Path,
    requested: &str,
    partial_path: &std::path::Path,
) -> Result<(String, std::path::PathBuf), String> {
    let path = std::path::Path::new(requested);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    for index in 0..=9999 {
        let name = if index == 0 {
            requested.to_string()
        } else {
            format!("{stem}({index}){extension}")
        };
        let final_path = download_root.join(&name);
        match tokio::fs::hard_link(partial_path, &final_path).await {
            Ok(()) => {
                let _ = tokio::fs::remove_file(partial_path).await;
                return Ok((name, final_path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("完成接收文件失败: {error}")),
        }
    }
    Err("同名文件过多，无法安全保存接收文件".to_string())
}

async fn fail_received_transfer(
    state: &AppState,
    transfer_id: &str,
    message_id: i64,
    partial_path: &std::path::Path,
    bytes_transferred: i64,
    error: String,
) -> ApiResponse {
    let _ = tokio::fs::remove_file(partial_path).await;
    let _ = crate::db::update_transfer(
        &state.pool,
        transfer_id,
        "failed",
        bytes_transferred,
        Some(&error),
    )
    .await;
    let _ = crate::db::update_file_status_by_id(&state.pool, message_id, "failed").await;
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error)
}

fn append_peer_chunk(target: &mut Vec<u8>, chunk: &[u8]) -> Result<(), ()> {
    if target.len().saturating_add(chunk.len()) > MAX_PEER_CHUNK_BYTES {
        Err(())
    } else {
        target.extend_from_slice(chunk);
        Ok(())
    }
}

async fn complete_received_conversation_file(
    state: &AppState,
    message_id: i64,
    conversation_id: &str,
    client_message_id: &str,
    sender_id: &str,
    final_path: &std::path::Path,
    file_size: i64,
) -> Result<crate::db::MessageRecord, String> {
    let message = crate::db::set_file_message_metadata(
        &state.pool,
        message_id,
        final_path
            .to_str()
            .ok_or_else(|| "最终文件路径不是有效 UTF-8".to_string())?,
        file_size,
        "accepted",
    )
    .await?;
    let self_id = crate::db::get_user_id(&state.pool).await?;
    record_local_delivery(
        state,
        conversation_id,
        client_message_id,
        sender_id,
        &self_id,
    )
    .await?;
    let sender_name = state
        .peer_manager
        .get_all_peers()
        .into_iter()
        .find(|peer| peer.id == sender_id)
        .map(|peer| peer.name)
        .unwrap_or_else(|| sender_id.to_string());
    broadcast_incoming_event(
        state,
        serde_json::json!({
            "id": message.id,
            "message_id": message.id,
            "client_message_id": client_message_id,
            "conversation_id": conversation_id,
            "sender_id": sender_id,
            "from_id": sender_id,
            "sender_name": sender_name,
            "from_name": sender_name,
            "content": message.content,
            "msg_type": "file",
            "timestamp": message.timestamp,
            "status": "received",
            "file_status": "accepted",
            "file_path": message.file_path,
            "file_size": file_size,
        }),
    );
    Ok(message)
}

async fn prepare_parallel_upload_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<crate::network::conversation_file::ParallelPrepareRequest>,
) -> ApiResponse {
    use crate::network::conversation_file::{
        create_or_resume_parallel_manifest, load_parallel_manifest, valid_parallel_prepare,
        ParallelTransferManifest,
    };

    if !valid_parallel_prepare(&payload) || payload.file_size > MAX_BROWSER_UPLOAD_BYTES {
        return api_error(StatusCode::BAD_REQUEST, "并行传输参数无效");
    }
    let Some(requested_file_name) = safe_file_name(&payload.file_name) else {
        return api_error(StatusCode::BAD_REQUEST, "文件名无效");
    };
    let conversation = match validate_incoming_file_conversation(
        &state,
        &payload.conversation_id,
        &payload.sender_id,
    )
    .await
    {
        Ok(conversation) => conversation,
        Err(error) => return backend_error(error),
    };
    let self_id = match crate::db::get_user_id(&state.pool).await {
        Ok(id) => id,
        Err(error) => return backend_error(error),
    };
    let expected_transfer_id = crate::network::conversation_file::recipient_transfer_id(
        &payload.client_message_id,
        &self_id,
    );
    if payload.transfer_id != expected_transfer_id
        && !payload
            .transfer_id
            .starts_with(&format!("{expected_transfer_id}:retry:"))
    {
        return api_error(StatusCode::BAD_REQUEST, "传输 ID 与接收者不匹配");
    }

    let transfer_guard =
        crate::network::conversation_file::lock_receive_file(&payload.client_message_id).await;
    let configured_root = match crate::db::get_download_path(&state.pool).await {
        Ok(path) => std::path::PathBuf::from(path),
        Err(error) => return backend_error(error),
    };
    if let Err(error) = tokio::fs::create_dir_all(&configured_root).await {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("创建下载目录失败: {error}"),
        );
    }
    let download_root = match tokio::fs::canonicalize(&configured_root).await {
        Ok(path) => path,
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取下载目录失败: {error}"),
            )
        }
    };

    let existing =
        match crate::db::get_message_by_client_id(&state.pool, &payload.client_message_id).await {
            Ok(message) => message,
            Err(error) => return backend_error(error),
        };
    let is_new = existing.is_none();
    let message = if let Some(message) = existing {
        if message.conversation_id.as_deref() != Some(payload.conversation_id.as_str())
            || message.sender_id != payload.sender_id
            || message.msg_type != "file"
            || message
                .file_size
                .is_some_and(|size| size != payload.file_size as i64)
            || message
                .sender_msg_id
                .as_deref()
                .is_some_and(|id| id != payload.sender_msg_id)
        {
            return api_error(StatusCode::CONFLICT, "客户端消息 ID 与已有文件冲突");
        }
        message
    } else {
        let file_name =
            match available_received_file_name(&download_root, &requested_file_name).await {
                Ok(name) => name,
                Err(error) => return backend_error(error),
            };
        let receiver_id = (conversation.kind == "direct").then_some(self_id.as_str());
        let message = match crate::db::save_conversation_message(
            &state.pool,
            &payload.conversation_id,
            &payload.sender_id,
            receiver_id,
            &file_name,
            "file",
            now_timestamp(),
            "received",
            &payload.client_message_id,
        )
        .await
        {
            Ok(message) => message,
            Err(error) => return backend_error(error),
        };
        let file_status = if crate::db::get_auto_download(&state.pool).await {
            "downloading"
        } else {
            "offered"
        };
        let message = match crate::db::set_file_message_metadata(
            &state.pool,
            message.id,
            download_root.join(&file_name).to_str().unwrap_or_default(),
            payload.file_size as i64,
            file_status,
        )
        .await
        {
            Ok(message) => message,
            Err(error) => return backend_error(error),
        };
        if let Err(error) = sqlx::query("UPDATE messages SET sender_msg_id = ? WHERE id = ?")
            .bind(&payload.sender_msg_id)
            .bind(message.id)
            .execute(&state.pool)
            .await
        {
            return backend_error(format!("保存发送端消息 ID 失败: {error}"));
        }
        message
    };
    let Some(final_file_name) = safe_file_name(&message.content) else {
        return api_error(StatusCode::CONFLICT, "数据库中的接收文件名无效");
    };
    let final_path = download_root.join(&final_file_name);
    if message.file_status.as_deref() == Some("accepted") {
        let final_matches = tokio::fs::metadata(&final_path)
            .await
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == payload.file_size);
        let digest_matches = if final_matches {
            crate::network::conversation_file::sha256_file(&final_path)
                .await
                .is_ok_and(|digest| digest == payload.file_sha256)
        } else {
            false
        };
        if !digest_matches {
            return api_error(StatusCode::CONFLICT, "已完成文件的本地副本不一致");
        }
        let transfer = match crate::db::create_transfer(
            &state.pool,
            &payload.transfer_id,
            Some(message.id),
            &payload.conversation_id,
            &payload.sender_id,
            "receive",
            "completed",
            payload.file_size as i64,
        )
        .await
        {
            Ok(transfer) => transfer,
            Err(error) => return backend_error(error),
        };
        if transfer.status != "completed" {
            let _ = crate::db::update_transfer(
                &state.pool,
                &payload.transfer_id,
                "completed",
                payload.file_size as i64,
                None,
            )
            .await;
        }
        let _ = crate::network::conversation_file::cleanup_parallel_transfer(
            &download_root,
            &payload.transfer_id,
        )
        .await;
        return Json(serde_json::json!({
            "status": "already_exists",
            "message_id": message.id,
            "transfer_id": payload.transfer_id,
            "received": payload.file_size,
            "missing_chunks": [],
        }))
        .into_response();
    }

    let manifest = ParallelTransferManifest {
        version: 2,
        sender_id: payload.sender_id.clone(),
        conversation_id: payload.conversation_id.clone(),
        client_message_id: payload.client_message_id.clone(),
        transfer_id: payload.transfer_id.clone(),
        sender_msg_id: payload.sender_msg_id.clone(),
        file_name: requested_file_name,
        final_file_name: final_file_name.clone(),
        file_size: payload.file_size,
        file_sha256: payload.file_sha256.clone(),
        chunks: payload.chunks.clone(),
        message_id: message.id,
    };
    match load_parallel_manifest(&download_root, &payload.transfer_id).await {
        Ok(Some(existing)) if existing != manifest => {
            return api_error(StatusCode::CONFLICT, "并行传输清单与已有记录冲突")
        }
        Ok(_) => {}
        Err(error) => return backend_error(error),
    }

    let auto_download = crate::db::get_auto_download(&state.pool).await;
    let manually_accepted =
        !is_new && message.file_status.as_deref() == Some("downloading");
    let transfer_status = if auto_download || manually_accepted {
        "transferring"
    } else {
        "awaiting_acceptance"
    };
    let transfer = match crate::db::create_transfer(
        &state.pool,
        &payload.transfer_id,
        Some(message.id),
        &payload.conversation_id,
        &payload.sender_id,
        "receive",
        transfer_status,
        payload.file_size as i64,
    )
    .await
    {
        Ok(transfer) => transfer,
        Err(error) => return backend_error(error),
    };
    if transfer.status == "cancelled" {
        return api_error(
            StatusCode::CONFLICT,
            "该接收尝试已结束，请使用新的传输 ID 重试",
        );
    }
    if transfer.status != transfer_status {
        if let Err(error) = crate::db::update_transfer(
            &state.pool,
            &payload.transfer_id,
            transfer_status,
            transfer.bytes_transferred,
            None,
        )
        .await {
            return backend_error(error);
        }
    }
    if let Err(error) = sqlx::query(
        "UPDATE transfers
         SET status = 'failed', error = 'superseded by retry', updated_at = ?
         WHERE message_id = ? AND direction = 'receive' AND id != ?
           AND status IN ('queued', 'awaiting_acceptance', 'transferring')",
    )
    .bind(now_timestamp())
    .bind(message.id)
    .bind(&payload.transfer_id)
    .execute(&state.pool)
    .await
    {
        return backend_error(format!("结束旧接收传输失败: {error}"));
    }
    let visible_file_status = if transfer_status == "awaiting_acceptance" {
        "offered"
    } else {
        "downloading"
    };
    if message.file_status.as_deref() != Some(visible_file_status) {
        if let Err(error) =
            crate::db::update_file_status_by_id(&state.pool, message.id, visible_file_status).await
        {
            return backend_error(error);
        }
    }

    let (manifest, missing_chunks, received) =
        match create_or_resume_parallel_manifest(&download_root, manifest).await {
            Ok(result) => result,
            Err(error) if error.contains("冲突") => {
                return api_error(StatusCode::CONFLICT, error)
            }
            Err(error) => return backend_error(error),
        };
    if let Err(error) = sqlx::query(
        "UPDATE transfers
         SET status = ?, bytes_transferred = ?, error = NULL, updated_at = ?
         WHERE id = ? AND status != 'cancelled'",
    )
    .bind(transfer_status)
    .bind(received as i64)
    .bind(now_timestamp())
    .bind(&payload.transfer_id)
    .execute(&state.pool)
    .await
    {
        return backend_error(format!("恢复并行传输进度失败: {error}"));
    }
    if is_new {
        let sender_name = state
            .peer_manager
            .get_all_peers()
            .into_iter()
            .find(|peer| peer.id == payload.sender_id)
            .map(|peer| peer.name)
            .unwrap_or_else(|| payload.sender_id.clone());
        broadcast_incoming_event(
            &state,
            serde_json::json!({
                "id": message.id,
                "message_id": message.id,
                "client_message_id": payload.client_message_id,
                "conversation_id": payload.conversation_id,
                "sender_id": payload.sender_id,
                "from_id": payload.sender_id,
                "sender_name": sender_name,
                "from_name": sender_name,
                "content": final_file_name,
                "msg_type": "file",
                "timestamp": message.timestamp,
                "status": "received",
                "file_status": visible_file_status,
                "file_size": payload.file_size,
                "sender_msg_id": payload.sender_msg_id,
                "transfer_id": payload.transfer_id,
            }),
        );
    }
    if transfer_status == "awaiting_acceptance" {
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "awaiting_acceptance",
                "message_id": message.id,
                "transfer_id": manifest.transfer_id,
                "received": received,
                "missing_chunks": missing_chunks,
            })),
        )
            .into_response();
    }
    drop(transfer_guard);
    if missing_chunks.is_empty() {
        return match finalize_parallel_receive(&state, &download_root, &manifest).await {
            Ok(message) => Json(serde_json::json!({
                "status": "completed",
                "message_id": message.id,
                "transfer_id": manifest.transfer_id,
                "received": manifest.file_size,
                "missing_chunks": [],
            }))
            .into_response(),
            Err(error) => backend_error(error),
        };
    }
    Json(serde_json::json!({
        "status": "ready",
        "message_id": message.id,
        "transfer_id": manifest.transfer_id,
        "received": received,
        "missing_chunks": missing_chunks,
    }))
    .into_response()
}

async fn finalize_parallel_receive(
    state: &AppState,
    download_root: &std::path::Path,
    manifest: &crate::network::conversation_file::ParallelTransferManifest,
) -> Result<crate::db::MessageRecord, String> {
    let _guard =
        crate::network::conversation_file::lock_receive_file(&manifest.client_message_id).await;
    let transfer = crate::db::get_transfer(&state.pool, &manifest.transfer_id)
        .await?
        .ok_or_else(|| "并行接收传输不存在".to_string())?;
    if transfer.status == "completed" {
        return crate::db::get_file_message_by_id(&state.pool, manifest.message_id)
            .await?
            .ok_or_else(|| "并行文件消息不存在".to_string());
    }
    if transfer.status != "transferring" {
        return Err("并行接收传输已结束".to_string());
    }
    let partial_path =
        match crate::network::conversation_file::merge_parallel_parts(download_root, manifest)
            .await
        {
            Ok(path) => path,
            Err(error) => {
                let _ = crate::db::update_transfer(
                    &state.pool,
                    &manifest.transfer_id,
                    "failed",
                    transfer.bytes_transferred,
                    Some(&error),
                )
                .await;
                let _ = crate::db::update_file_status_by_id(
                    &state.pool,
                    manifest.message_id,
                    "failed",
                )
                .await;
                return Err(error);
            }
        };
    let expected_final_path = download_root.join(&manifest.final_file_name);
    let already_materialized = tokio::fs::metadata(&expected_final_path)
        .await
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == manifest.file_size)
        && crate::network::conversation_file::sha256_file(&expected_final_path)
            .await
            .is_ok_and(|hash| hash == manifest.file_sha256);
    let (final_file_name, final_path) = if already_materialized {
        let _ = tokio::fs::remove_file(&partial_path).await;
        (manifest.final_file_name.clone(), expected_final_path)
    } else {
        finalize_received_file(download_root, &manifest.final_file_name, &partial_path).await?
    };
    if final_file_name != manifest.final_file_name {
        if let Err(error) = sqlx::query("UPDATE messages SET content = ? WHERE id = ?")
            .bind(&final_file_name)
            .bind(manifest.message_id)
            .execute(&state.pool)
            .await
        {
            let _ = tokio::fs::remove_file(&final_path).await;
            return Err(format!("保存并行接收文件名失败: {error}"));
        }
    }
    let message = match complete_received_conversation_file(
        state,
        manifest.message_id,
        &manifest.conversation_id,
        &manifest.client_message_id,
        &manifest.sender_id,
        &final_path,
        manifest.file_size as i64,
    )
    .await
    {
        Ok(message) => message,
        Err(error) => {
            let _ = tokio::fs::remove_file(&final_path).await;
            return Err(error);
        }
    };
    crate::db::update_transfer(
        &state.pool,
        &manifest.transfer_id,
        "completed",
        manifest.file_size as i64,
        None,
    )
    .await?;
    crate::network::conversation_file::cleanup_parallel_transfer(
        download_root,
        &manifest.transfer_id,
    )
    .await?;
    Ok(message)
}

async fn receive_parallel_upload_http(
    State(state): State<Arc<AppState>>,
    Path((transfer_id, chunk_index)): Path<(String, usize)>,
    request: Request,
) -> ApiResponse {
    let download_root = match crate::db::get_download_path(&state.pool).await {
        Ok(path) => std::path::PathBuf::from(path),
        Err(error) => return backend_error(error),
    };
    let download_root = match tokio::fs::canonicalize(&download_root).await {
        Ok(path) => path,
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取下载目录失败: {error}"),
            )
        }
    };
    let manifest =
        match crate::network::conversation_file::load_parallel_manifest(
            &download_root,
            &transfer_id,
        )
        .await
        {
            Ok(Some(manifest)) => manifest,
            Ok(None) => return api_error(StatusCode::NOT_FOUND, "并行传输尚未准备"),
            Err(error) => return backend_error(error),
        };
    let Some(chunk) = manifest
        .chunks
        .get(chunk_index)
        .filter(|chunk| chunk.index == chunk_index)
    else {
        return api_error(StatusCode::BAD_REQUEST, "并行分块序号无效");
    };
    let content_length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if content_length != Some(chunk.length) {
        return api_error(StatusCode::BAD_REQUEST, "并行分块长度无效");
    }
    let result = match crate::network::conversation_file::receive_parallel_chunk(
        &state.pool,
        &download_root,
        &transfer_id,
        chunk_index,
        request.into_body(),
    )
    .await
    {
        Ok(result) => result,
        Err(error) if error.contains("已结束") => {
            return api_error(StatusCode::CONFLICT, error)
        }
        Err(error) if error.contains("无效") || error.contains("长度") => {
            return api_error(StatusCode::BAD_REQUEST, error)
        }
        Err(error) => return backend_error(error),
    };
    broadcast_incoming_event(
        &state,
        serde_json::json!({
            "msg_type": "file_download_progress",
            "id": result.manifest.message_id,
            "client_message_id": result.manifest.client_message_id,
            "conversation_id": result.manifest.conversation_id,
            "file_name": result.manifest.final_file_name,
            "file_status": "downloading",
            "received": result.received,
            "total": result.manifest.file_size,
            "transfer_id": result.manifest.transfer_id,
        }),
    );
    if result.complete {
        return match finalize_parallel_receive(&state, &download_root, &result.manifest).await {
            Ok(message) => Json(serde_json::json!({
                "status": "completed",
                "message_id": message.id,
                "transfer_id": result.manifest.transfer_id,
                "received": result.manifest.file_size,
            }))
            .into_response(),
            Err(error) => backend_error(error),
        };
    }
    Json(serde_json::json!({
        "status": "receiving",
        "message_id": result.manifest.message_id,
        "transfer_id": result.manifest.transfer_id,
        "received": result.received,
        "total": result.manifest.file_size,
    }))
    .into_response()
}

#[allow(clippy::too_many_arguments)]
async fn receive_conversation_file_chunk(
    state: &AppState,
    sender_id: &str,
    conversation_id: &str,
    client_message_id: &str,
    sender_msg_id: &str,
    requested_transfer_id: &str,
    requested_file_name: &str,
    file_size: u64,
    chunk_index: usize,
    chunk_total: usize,
    chunk_data: Vec<u8>,
    speed_mb_s: f64,
) -> ApiResponse {
    let Some(requested_file_name) = safe_file_name(requested_file_name) else {
        return api_error(StatusCode::BAD_REQUEST, "文件名无效");
    };
    if client_message_id.trim().is_empty()
        || client_message_id.len() > 128
        || chunk_total == 0
        || chunk_index >= chunk_total
        || chunk_data.len() > MAX_PEER_CHUNK_BYTES
        || file_size > MAX_BROWSER_UPLOAD_BYTES
        || chunk_data.len() as u64 > file_size
        || (file_size == 0 && (chunk_total != 1 || chunk_index != 0))
    {
        return api_error(StatusCode::BAD_REQUEST, "文件分块参数无效");
    }
    let conversation =
        match validate_incoming_file_conversation(state, conversation_id, sender_id).await {
            Ok(conversation) => conversation,
            Err(error) => return backend_error(error),
        };
    let self_id = match crate::db::get_user_id(&state.pool).await {
        Ok(id) => id,
        Err(error) => return backend_error(error),
    };
    let expected_transfer_id =
        crate::network::conversation_file::recipient_transfer_id(client_message_id, &self_id);
    let transfer_id = if requested_transfer_id.trim().is_empty() {
        expected_transfer_id.clone()
    } else {
        requested_transfer_id.trim().to_string()
    };
    let retry_prefix = format!("{expected_transfer_id}:retry:");
    if transfer_id.len() > 256
        || (transfer_id != expected_transfer_id && !transfer_id.starts_with(&retry_prefix))
    {
        return api_error(StatusCode::BAD_REQUEST, "传输 ID 与接收者不匹配");
    }
    let _transfer_guard =
        crate::network::conversation_file::lock_receive_file(client_message_id).await;
    let download_root = match crate::db::get_download_path(&state.pool).await {
        Ok(path) => std::path::PathBuf::from(path),
        Err(error) => return backend_error(error),
    };
    if let Err(error) = tokio::fs::create_dir_all(&download_root).await {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("创建下载目录失败: {error}"),
        );
    }
    let download_root = match tokio::fs::canonicalize(&download_root).await {
        Ok(path) => path,
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取下载目录失败: {error}"),
            )
        }
    };
    let existing = match crate::db::get_message_by_client_id(&state.pool, client_message_id).await {
        Ok(message) => message,
        Err(error) => return backend_error(error),
    };
    if existing.is_none() && chunk_index != 0 {
        return api_error(StatusCode::CONFLICT, "缺少文件首个分块，请从头重试");
    }
    let is_new = existing.is_none();
    let message = if let Some(message) = existing {
        if message.conversation_id.as_deref() != Some(conversation_id)
            || message.sender_id != sender_id
            || message.msg_type != "file"
            || message.file_size.is_some_and(|size| size != file_size as i64)
            || (!sender_msg_id.is_empty()
                && message
                    .sender_msg_id
                    .as_deref()
                    .is_some_and(|existing| existing != sender_msg_id))
        {
            return api_error(StatusCode::CONFLICT, "客户端消息 ID 与已有文件冲突");
        }
        message
    } else {
        let file_name =
            match available_received_file_name(&download_root, &requested_file_name).await {
                Ok(name) => name,
                Err(error) => return backend_error(error),
            };
        let receiver_id = (conversation.kind == "direct").then_some(self_id.as_str());
        let message = match crate::db::save_conversation_message(
            &state.pool,
            conversation_id,
            sender_id,
            receiver_id,
            &file_name,
            "file",
            now_timestamp(),
            "received",
            client_message_id,
        )
        .await
        {
            Ok(message) => message,
            Err(error) => return backend_error(error),
        };
        let final_path = download_root.join(&file_name);
        let file_status = if crate::db::get_auto_download(&state.pool).await {
            "downloading"
        } else {
            "offered"
        };
        let message = match crate::db::set_file_message_metadata(
            &state.pool,
            message.id,
            final_path.to_str().unwrap_or_default(),
            file_size as i64,
            file_status,
        )
        .await
        {
            Ok(message) => message,
            Err(error) => return backend_error(error),
        };
        if !sender_msg_id.is_empty() {
            if sender_msg_id.len() > 64 {
                return api_error(StatusCode::BAD_REQUEST, "发送端消息 ID 无效");
            }
            if let Err(error) = sqlx::query("UPDATE messages SET sender_msg_id = ? WHERE id = ?")
                .bind(sender_msg_id)
                .bind(message.id)
                .execute(&state.pool)
                .await
            {
                return backend_error(format!("保存发送端消息 ID 失败: {error}"));
            }
        }
        message
    };
    let Some(file_name) = safe_file_name(&message.content) else {
        return api_error(StatusCode::CONFLICT, "数据库中的接收文件名无效");
    };
    let final_path = download_root.join(&file_name);
    let partial_path =
        crate::network::conversation_file::received_partial_path(&download_root, &transfer_id);
    if message.file_status.as_deref() == Some("accepted") {
        let final_matches = tokio::fs::metadata(&final_path)
            .await
            .map(|metadata| metadata.is_file() && metadata.len() == file_size)
            .unwrap_or(false);
        if !final_matches {
            return api_error(StatusCode::CONFLICT, "已完成文件的本地副本不一致");
        }
        let _ = tokio::fs::remove_file(&partial_path).await;
        let transfer = match crate::db::create_transfer(
            &state.pool,
            &transfer_id,
            Some(message.id),
            conversation_id,
            sender_id,
            "receive",
            "completed",
            file_size as i64,
        )
        .await
        {
            Ok(transfer) => transfer,
            Err(error) => return backend_error(error),
        };
        if transfer.status != "completed" {
            let _ = crate::db::update_transfer(
                &state.pool,
                &transfer_id,
                "completed",
                file_size as i64,
                None,
            )
            .await;
        }
        return Json(serde_json::json!({
            "status": "already_exists",
            "message_id": message.id,
            "client_message_id": client_message_id,
            "transfer_id": transfer_id,
        }))
        .into_response();
    }

    let auto_download = crate::db::get_auto_download(&state.pool).await;
    let manually_accepted =
        !is_new && message.file_status.as_deref() == Some("downloading");
    let initial_transfer_status = if auto_download || manually_accepted {
        "transferring"
    } else {
        "awaiting_acceptance"
    };
    let mut transfer = match crate::db::create_transfer(
        &state.pool,
        &transfer_id,
        Some(message.id),
        conversation_id,
        sender_id,
        "receive",
        initial_transfer_status,
        file_size as i64,
    )
    .await
    {
        Ok(transfer) => transfer,
        Err(error) => return backend_error(error),
    };
    let latest_transfer_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM transfers
         WHERE message_id = ? AND direction = 'receive'
         ORDER BY rowid DESC LIMIT 1",
    )
    .bind(message.id)
    .fetch_optional(&state.pool)
    .await;
    match latest_transfer_id {
        Ok(Some(latest_transfer_id)) if latest_transfer_id != transfer_id => {
            return api_error(StatusCode::CONFLICT, "该接收尝试已被新的重试取代");
        }
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::CONFLICT, "接收传输未保存"),
        Err(error) => return backend_error(format!("查询最新接收传输失败: {error}")),
    }
    if let Err(error) = sqlx::query(
        "UPDATE transfers
         SET status = 'failed', error = 'superseded by retry', updated_at = ?
         WHERE message_id = ? AND direction = 'receive' AND id != ?
           AND status IN ('queued', 'awaiting_acceptance', 'transferring')",
    )
    .bind(now_timestamp())
    .bind(message.id)
    .bind(&transfer_id)
    .execute(&state.pool)
    .await
    {
        return backend_error(format!("结束旧接收传输失败: {error}"));
    }
    if matches!(transfer.status.as_str(), "cancelled" | "failed") {
        return api_error(
            StatusCode::CONFLICT,
            "该接收尝试已结束，请使用新的传输 ID 重试",
        );
    }
    if transfer.status == "awaiting_acceptance" && auto_download {
        transfer = match crate::db::update_transfer(
            &state.pool,
            &transfer_id,
            "transferring",
            transfer.bytes_transferred,
            None,
        )
        .await
        {
            Ok(transfer) => transfer,
            Err(error) => return backend_error(error),
        };
    } else if transfer.status == "queued" {
        transfer = match crate::db::update_transfer(
            &state.pool,
            &transfer_id,
            "transferring",
            transfer.bytes_transferred,
            None,
        )
        .await
        {
            Ok(transfer) => transfer,
            Err(error) => return backend_error(error),
        };
    }
    let awaiting_acceptance = transfer.status == "awaiting_acceptance";
    let visible_file_status = if awaiting_acceptance {
        "offered"
    } else {
        "downloading"
    };
    if message.file_status.as_deref() != Some(visible_file_status) {
        if let Err(error) =
            crate::db::update_file_status_by_id(&state.pool, message.id, visible_file_status).await
        {
            return backend_error(error);
        }
    }

    if is_new {
        let sender_name = state
            .peer_manager
            .get_all_peers()
            .into_iter()
            .find(|peer| peer.id == sender_id)
            .map(|peer| peer.name)
            .unwrap_or_else(|| sender_id.to_string());
        broadcast_incoming_event(
            state,
            serde_json::json!({
                "id": message.id,
                "message_id": message.id,
                "client_message_id": client_message_id,
                "conversation_id": conversation_id,
                "sender_id": sender_id,
                "from_id": sender_id,
                "sender_name": sender_name,
                "from_name": sender_name,
                "content": file_name,
                "msg_type": "file",
                "timestamp": message.timestamp,
                "status": "received",
                "file_status": visible_file_status,
                "file_size": file_size,
                "sender_msg_id": sender_msg_id,
                "transfer_id": transfer_id,
            }),
        );
        let _ = tokio::fs::remove_file(&partial_path).await;
    }
    if awaiting_acceptance {
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "awaiting_acceptance",
                "message_id": message.id,
                "client_message_id": client_message_id,
                "transfer_id": transfer_id,
                "received": transfer.bytes_transferred,
                "total": file_size,
            })),
        )
            .into_response();
    }
    let mut current_size = tokio::fs::metadata(&partial_path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if chunk_index == 0 && current_size > 0 && current_size < chunk_data.len() as u64 {
        if let Err(error) = tokio::fs::remove_file(&partial_path).await {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("重置不完整分块失败: {error}"),
            );
        }
        current_size = 0;
    }
    let expected_size = if chunk_index + 1 == chunk_total {
        file_size.saturating_sub(chunk_data.len() as u64)
    } else {
        (chunk_index as u64).saturating_mul(chunk_data.len() as u64)
    };
    let chunk_end = expected_size.saturating_add(chunk_data.len() as u64);
    if current_size < expected_size || (current_size > expected_size && current_size < chunk_end) {
        return api_error(StatusCode::CONFLICT, "文件分块顺序不正确，请重试");
    }
    if current_size == expected_size {
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&partial_path)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                return fail_received_transfer(
                    state,
                    &transfer_id,
                    message.id,
                    &partial_path,
                    current_size as i64,
                    format!("打开接收临时文件失败: {error}"),
                )
                .await
            }
        };
        if let Err(error) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk_data).await {
            return fail_received_transfer(
                state,
                &transfer_id,
                message.id,
                &partial_path,
                current_size as i64,
                format!("写入接收分块失败: {error}"),
            )
            .await;
        }
        if let Err(error) = tokio::io::AsyncWriteExt::flush(&mut file).await {
            return fail_received_transfer(
                state,
                &transfer_id,
                message.id,
                &partial_path,
                current_size as i64,
                format!("保存接收分块失败: {error}"),
            )
            .await;
        }
        current_size = chunk_end;
    }
    if current_size > file_size {
        return fail_received_transfer(
            state,
            &transfer_id,
            message.id,
            &partial_path,
            current_size as i64,
            "接收文件大小超过声明值".to_string(),
        )
        .await;
    }
    if let Err(error) = crate::db::update_transfer(
        &state.pool,
        &transfer_id,
        "transferring",
        current_size as i64,
        None,
    )
    .await
    {
        return fail_received_transfer(
            state,
            &transfer_id,
            message.id,
            &partial_path,
            current_size as i64,
            error,
        )
        .await;
    }
    broadcast_incoming_event(
        state,
        serde_json::json!({
            "msg_type": "file_download_progress",
            "id": message.id,
            "client_message_id": client_message_id,
            "conversation_id": conversation_id,
            "file_name": file_name,
            "file_status": "downloading",
            "received": current_size,
            "total": file_size,
            "speed_mb_s": speed_mb_s,
            "transfer_id": transfer_id,
        }),
    );

    if current_size == file_size {
        let (final_file_name, final_path) =
            match finalize_received_file(&download_root, &file_name, &partial_path).await {
                Ok(result) => result,
                Err(error) => {
                    return fail_received_transfer(
                        state,
                        &transfer_id,
                        message.id,
                        &partial_path,
                        current_size as i64,
                        error,
                    )
                    .await
                }
            };
        if final_file_name != file_name {
            if let Err(error) = sqlx::query("UPDATE messages SET content = ? WHERE id = ?")
                .bind(&final_file_name)
                .bind(message.id)
                .execute(&state.pool)
                .await
            {
                let _ = tokio::fs::remove_file(&final_path).await;
                return fail_received_transfer(
                    state,
                    &transfer_id,
                    message.id,
                    &partial_path,
                    current_size as i64,
                    format!("保存接收文件名失败: {error}"),
                )
                .await;
            }
        }
        let message = match complete_received_conversation_file(
            state,
            message.id,
            conversation_id,
            client_message_id,
            sender_id,
            &final_path,
            file_size as i64,
        )
        .await
        {
            Ok(message) => message,
            Err(error) => {
                let _ = tokio::fs::remove_file(&final_path).await;
                return fail_received_transfer(
                    state,
                    &transfer_id,
                    message.id,
                    &partial_path,
                    current_size as i64,
                    error,
                )
                .await;
            }
        };
        if let Err(error) = crate::db::update_transfer(
            &state.pool,
            &transfer_id,
            "completed",
            current_size as i64,
            None,
        )
        .await
        {
            return backend_error(error);
        }
        return Json(serde_json::json!({
            "status": "completed",
            "message_id": message.id,
            "client_message_id": client_message_id,
            "transfer_id": transfer_id,
        }))
        .into_response();
    }

    Json(serde_json::json!({
        "status": "receiving",
        "message_id": message.id,
        "client_message_id": client_message_id,
        "transfer_id": transfer_id,
        "received": current_size,
        "total": file_size,
    }))
    .into_response()
}

async fn upload_file_http(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    println!("[Web Server] 收到文件上传请求");

    let mut sender_id = String::new();
    let mut file_name = String::new();
    let mut file_size: u64 = 0;
    let mut chunk_index: usize = 0;
    let mut chunk_total: usize = 0;
    let mut chunk_data: Option<Vec<u8>> = None;
    let mut sender_msg_id = String::new();
    let mut speed_mb_s: f64 = 0.0;
    let mut conversation_id = String::new();
    let mut client_message_id = String::new();
    let mut transfer_id = String::new();
    let mut group_sync = String::new();

    // 获取下载目录
    let download_dir = get_download_dir(&state.pool).await;
    if let Err(e) = fs::create_dir_all(&download_dir).await {
        eprintln!("[Web Server] 创建目录失败: {}", e);
    }

    // 解析 multipart 字段
    println!("[Web Server] 开始解析 multipart 字段");
    while let Some(mut field) = multipart.next_field().await.ok().flatten() {
        let field_name = field.name().map(|s| s.to_string()).unwrap_or_default();

        match field_name.as_str() {
            "peer_id" => {
                if let Ok(text) = field.text().await {
                    sender_id = text;
                    println!("[Web Server] sender_id (发送者): {}", sender_id);
                }
            }
            "file_name" => {
                if let Ok(text) = field.text().await {
                    file_name = text;
                    println!("[Web Server] 文件名: {}", file_name);
                }
            }
            "file_size" => {
                if let Ok(text) = field.text().await {
                    file_size = text.parse().unwrap_or(0);
                    println!("[Web Server] 文件总大小: {}", file_size);
                }
            }
            "chunk_index" => {
                if let Ok(text) = field.text().await {
                    chunk_index = text.parse().unwrap_or(0);
                }
            }
            "chunk_total" => {
                if let Ok(text) = field.text().await {
                    chunk_total = text.parse().unwrap_or(0);
                    println!("[Web Server] 分块信息: {}/{}", chunk_index + 1, chunk_total);
                }
            }
            "sender_msg_id" => {
                if let Ok(text) = field.text().await {
                    sender_msg_id = text;
                }
            }
            "speed_mb_s" => {
                if let Ok(text) = field.text().await {
                    speed_mb_s = text.parse().unwrap_or(0.0);
                }
            }
            "conversation_id" => {
                if let Ok(text) = field.text().await {
                    conversation_id = text;
                }
            }
            "client_message_id" => {
                if let Ok(text) = field.text().await {
                    client_message_id = text;
                }
            }
            "transfer_id" => {
                if let Ok(text) = field.text().await {
                    transfer_id = text;
                }
            }
            "group_sync" => {
                if let Ok(text) = field.text().await {
                    group_sync = text;
                }
            }
            "chunk" => {
                let mut data = Vec::new();
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            if append_peer_chunk(&mut data, &chunk).is_err() {
                                return api_error(
                                    StatusCode::PAYLOAD_TOO_LARGE,
                                    "文件分块超过接收上限",
                                );
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            return api_error(
                                error.status(),
                                format!("读取文件分块失败: {error}"),
                            )
                        }
                    }
                }
                chunk_data = Some(data);
                println!(
                    "[Web Server] 收到分块数据，大小: {} 字节",
                    chunk_data.as_ref().map(|d| d.len()).unwrap_or(0)
                );
            }
            _ => {
                println!("[Web Server] 忽略未知字段: {}", field_name);
            }
        }
    }

    // 验证必需字段
    // 第一块需要所有字段，后续块只需要 chunk_index 和 chunk
    println!(
        "[Web Server] 验证字段: file_name={}, chunk_index={}, chunk_total={}, has_chunk={}",
        file_name,
        chunk_index,
        chunk_total,
        chunk_data.is_some()
    );

    if chunk_data.is_none() {
        eprintln!("[Web Server] ✗ 缺少 chunk 数据");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "缺少 chunk 数据".to_string(),
            }),
        )
            .into_response();
    }

    if chunk_index == 0 && file_name.is_empty() {
        eprintln!("[Web Server] ✗ 第一块缺少 file_name");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "第一块缺少 file_name".to_string(),
            }),
        )
            .into_response();
    }

    let chunk_data = chunk_data.unwrap();
    let Some(valid_file_name) = safe_file_name(&file_name) else {
        return api_error(StatusCode::BAD_REQUEST, "文件名无效");
    };
    file_name = valid_file_name;
    if chunk_total == 0
        || chunk_index >= chunk_total
        || file_size > MAX_BROWSER_UPLOAD_BYTES
        || chunk_data.len() > MAX_PEER_CHUNK_BYTES
        || chunk_data.len() as u64 > file_size
    {
        return api_error(StatusCode::BAD_REQUEST, "文件分块参数无效");
    }

    if !conversation_id.is_empty() || !client_message_id.is_empty() {
        if conversation_id.is_empty() || client_message_id.is_empty() {
            return api_error(
                StatusCode::BAD_REQUEST,
                "conversation_id 与 client_message_id 必须同时提供",
            );
        }
        if transfer_id.trim().is_empty() {
            return api_error(StatusCode::BAD_REQUEST, "缺少稳定传输 ID");
        }
        if !group_sync.is_empty() {
            let sync = match crate::network::protocol::parse_protocol_message(&group_sync) {
                Ok(Some(sync)) => sync,
                _ => return api_error(StatusCode::BAD_REQUEST, "群同步快照无效"),
            };
            if !matches!(
                &sync,
                crate::network::protocol::ProtocolMessage::GroupSync {
                    group_id,
                    members,
                    ..
                } if group_id == &conversation_id
                    && members.iter().any(|member| member.peer_id == sender_id)
            ) {
                return api_error(StatusCode::BAD_REQUEST, "群同步快照无效");
            }
            if let Err(error) = handle_protocol_message(&state, sync).await {
                return backend_error(error);
            }
        }
        return receive_conversation_file_chunk(
            &state,
            &sender_id,
            &conversation_id,
            &client_message_id,
            &sender_msg_id,
            &transfer_id,
            &file_name,
            file_size,
            chunk_index,
            chunk_total,
            chunk_data,
            speed_mb_s,
        )
        .await;
    }
    let legacy_lock_key = format!("legacy:{sender_id}:{sender_msg_id}:{file_name}");
    let _legacy_guard =
        crate::network::conversation_file::lock_receive_file(&legacy_lock_key).await;

    // ── 第一块：智能命名协商 ──────────────────────────────────────────────
    // 最终写入的文件名（可能因重命名而与 file_name 不同）
    let final_file_name: String;

    if chunk_index == 0 {
        // 拆分主文件名和扩展名
        let (stem, ext) = {
            let p = std::path::Path::new(&file_name);
            let s = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&file_name)
                .to_string();
            let e = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|e| format!(".{}", e))
                .unwrap_or_default();
            (s, e)
        };

        // 检查目标路径是否存在完整文件（无 .downloading 后缀）
        let candidate = download_dir.join(&file_name);
        let downloading_path = download_dir.join(format!("{}.downloading", file_name));

        if candidate.exists() && !downloading_path.exists() {
            // 目标文件存在且完整，比较大小
            let existing_size = tokio::fs::metadata(&candidate)
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            if existing_size == file_size {
                // 大小完全相同 → 秒传：直接复用已有文件，写入数据库记录
                println!(
                    "[Web Server] ✓ 秒传命中: {:?} (大小相同: {} 字节)",
                    candidate, file_size
                );

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                // 秒传路径：先检查 offered 记录
                let instant_path = candidate.to_str().unwrap_or("").to_string();
                let updated_offered = crate::db::find_and_update_offered_record(
                    &state.pool,
                    &sender_id,
                    &file_name,
                    &instant_path,
                )
                .await
                .unwrap_or(None);

                if let Some((existing_id, _)) = updated_offered {
                    // 立即标记为 accepted（文件已完整）
                    let _ = crate::db::update_file_status_by_id(
                        &state.pool, existing_id, "accepted",
                    )
                    .await;
                    println!(
                        "[Web Server] ✓ 秒传: 已更新 offered 记录(ID={}) 为 accepted",
                        existing_id
                    );

                    let sender_name = state.peer_manager.get_all_peers().iter()
                        .find(|p| p.id == sender_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();

                    // 通知前端替换消息元素（完整 file 消息，触发图片渲染）
                    let full_msg = serde_json::json!({
                        "id": existing_id,
                        "from_id": sender_id,
                        "from_name": sender_name,
                        "content": file_name,
                        "msg_type": "file",
                        "file_status": "accepted",
                        "file_path": instant_path,
                        "file_size": file_size,
                        "file_id": instant_path.split('/').last().unwrap_or(&file_name),
                        "file_name": file_name,
                        "sender_msg_id": sender_msg_id,
                        "timestamp": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                    });
                    let _ = state.ws_broadcast.send(full_msg.to_string());
                    #[cfg(feature = "desktop")]
                    if let Some(ref app) = state.app_handle {
                        use tauri::Emitter;
                        let _ = app.emit("new-message", full_msg);
                    }
                } else {
                    match crate::db::create_received_file_record(
                        &state.pool,
                        sender_id.clone(),
                        file_name.clone(),
                        instant_path.clone(),
                        file_size,
                        timestamp,
                        &sender_msg_id,
                    )
                    .await
                    {
                        Ok(msg_id) => {
                            // 立即标记为 accepted（文件已完整）
                            let _ = crate::db::update_file_status_by_id(
                                &state.pool, msg_id, "accepted",
                            )
                            .await;
                            println!(
                                "[Web Server] ✓ 秒传记录已创建并标记为 accepted，ID: {}",
                                msg_id
                            );
                            let sender_name = state.peer_manager.get_all_peers().iter()
                                .find(|p| p.id == sender_id)
                                .map(|p| p.name.clone())
                                .unwrap_or_default();
                            // 通知前端替换消息元素（完整 file 消息，触发图片渲染）
                            let full_msg = serde_json::json!({
                                "id": msg_id,
                                "from_id": sender_id,
                                "from_name": sender_name,
                                "content": file_name,
                                "msg_type": "file",
                                "file_status": "accepted",
                                "file_path": instant_path,
                                "file_size": file_size,
                                "file_id": instant_path.split('/').last().unwrap_or(&file_name),
                                "file_name": file_name,
                                "sender_msg_id": sender_msg_id,
                                "timestamp": std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                            });
                            let _ = state.ws_broadcast.send(full_msg.to_string());
                            #[cfg(feature = "desktop")]
                            if let Some(ref app) = state.app_handle {
                                use tauri::Emitter;
                                let _ = app.emit("new-message", full_msg);
                            }
                        }
                        Err(e) => {
                            eprintln!("[Web Server] ✗ 秒传记录创建失败: {}", e);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ErrorResponse {
                                    error: format!("秒传记录创建失败: {}", e),
                                }),
                            )
                                .into_response();
                        }
                    }
                }

                println!("[Web Server] ========== 秒传完成，通知发送端停止 ==========");
                return Json(serde_json::json!({
                    "status": "already_exists",
                    "file_name": file_name,
                    "file_size": file_size,
                    "message": "文件已存在且完整，秒传成功",
                }))
                .into_response();
            } else {
                // 大小不同 → 冲突，需要重命名
                println!(
                    "[Web Server] 同名文件大小不同 (已有: {}, 新: {})，触发重命名",
                    existing_size, file_size
                );
            }
        }

        // 需要找一个不冲突的文件名（原名冲突 或 存在 .downloading 残留）
        let mut resolved_name = file_name.clone();
        if candidate.exists() || downloading_path.exists() {
            let mut i = 1usize;
            loop {
                let candidate_name = format!("{}({}){}", stem, i, ext);
                let candidate_path = download_dir.join(&candidate_name);
                let candidate_dl = download_dir.join(format!("{}.downloading", candidate_name));
                if !candidate_path.exists() && !candidate_dl.exists() {
                    resolved_name = candidate_name;
                    println!("[Web Server] 重命名为: {}", resolved_name);
                    break;
                }
                i += 1;
                if i > 9999 {
                    // 极端情况兜底
                    resolved_name = format!("{}_{}{}", stem, chrono::Utc::now().timestamp(), ext);
                    break;
                }
            }
        }

        final_file_name = resolved_name;
    } else {
        // 后续块：从数据库按 sender_msg_id 查询正在下载的文件名（多文件并发隔离）
        let target = if !sender_msg_id.is_empty() {
            crate::db::get_downloading_file_by_sender_msg_id(&state.pool, &sender_msg_id).await
        } else {
            crate::db::get_downloading_file(&state.pool, &sender_id).await
        };
        if file_name.is_empty() {
            match target {
                Ok(Some(name)) => {
                    final_file_name = name;
                    println!(
                        "[Web Server] 后续块从数据库查询到文件名: {}",
                        final_file_name
                    );
                }
                Ok(None) => {
                    // DB 中没有 downloading 记录 → 文件可能已秒传完成
                    // 返回 success 让发送端停止上传
                    println!(
                        "[Web Server] 后续块无可下载记录 (sender_msg_id={})，假定已完成，通知发送端停止",
                        sender_msg_id
                    );
                    return Json(serde_json::json!({
                        "status": "already_exists",
                        "file_name": file_name,
                        "file_size": file_size,
                    }))
                    .into_response();
                }
                Err(e) => {
                    eprintln!("[Web Server] ✗ 数据库查询失败: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("数据库查询失败: {}", e),
                        }),
                    )
                        .into_response();
                }
            }
        } else {
            // 发送端传来了 file_name，但后续块应以数据库中的 resolved 名为准
            match target {
                Ok(Some(name)) => final_file_name = name,
                Ok(None) => {
                    // 无可下载记录 → 文件已秒传完成，通知发送端停止
                    return Json(serde_json::json!({
                        "status": "already_exists",
                        "file_name": file_name,
                        "file_size": file_size,
                    }))
                    .into_response();
                }
                _ => final_file_name = file_name.clone(),
            }
        }
    }

    // 临时文件路径（写入期间使用 .downloading 后缀）
    let temp_path = download_dir.join(format!("{}.downloading", final_file_name));
    let final_path = download_dir.join(&final_file_name);

    // 第一块：创建/截断临时文件（不触碰已有的完整文件）
    if chunk_index == 0 {
        println!("[Web Server] 创建临时文件: {:?}", temp_path);
        // 清理可能残留的旧临时文件
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    // 以追加模式写入临时文件
    let file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&temp_path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[Web Server] ✗ 打开临时文件失败: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("打开文件失败: {}", e),
                }),
            )
                .into_response();
        }
    };

    let mut writer = tokio::io::BufWriter::new(file);

    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut writer, &chunk_data).await {
        eprintln!("[Web Server] ✗ 写入文件失败: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("写入文件失败: {}", e),
            }),
        )
            .into_response();
    }

    if let Err(e) = tokio::io::AsyncWriteExt::flush(&mut writer).await {
        eprintln!("[Web Server] ✗ 刷新缓冲区失败: {}", e);
    }

    println!(
        "[Web Server] ✓ 分块 {}/{} 已写入临时文件，大小: {} 字节",
        chunk_index + 1,
        chunk_total,
        chunk_data.len()
    );

    // 第一块时创建/更新数据库记录，并广播初始消息给前端
    if chunk_index == 0 {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let file_path = final_path.to_str().unwrap_or("").to_string();

        // 先检查是否存在 offered 记录（手动下载场景），更新它而非新建
        let updated_offered = crate::db::find_and_update_offered_record(
            &state.pool,
            &sender_id,
            &final_file_name,
            &file_path,
        )
        .await
        .unwrap_or(None);

        let record_id = if let Some((found_id, _)) = updated_offered {
            println!(
                "[Web Server] ✓ 已更新 offered 记录为 downloading，无需新建"
            );
            found_id
        } else {
            // 没有 offered 记录 → 新建接收记录（含 sender_msg_id，用于 DOM 查找）
            match crate::db::create_received_file_record(
                &state.pool,
                sender_id.clone(),
                final_file_name.clone(),
                file_path,
                file_size,
                timestamp,
                &sender_msg_id,
            )
            .await
            {
                Ok(msg_id) => {
                    println!(
                        "[Web Server] ✓ 文件消息已创建，ID: {}, 最终名: {}",
                        msg_id, final_file_name
                    );
                    msg_id
                }
                Err(e) => {
                    eprintln!("[Web Server] ✗ 创建文件消息失败: {}", e);
                    0
                }
            }
        };

        // 查询发送者名称
        let sender_name = state.peer_manager.get_all_peers().iter()
            .find(|p| p.id == sender_id)
            .map(|p| p.name.clone())
            .unwrap_or_default();

        // 广播初始消息（使前端创建 DOM 元素，data-sender-msg-id 供进度查找）
        if record_id > 0 && !sender_msg_id.is_empty() {
            let init_msg = serde_json::json!({
                "id": record_id,
                "from_id": sender_id,
                "from_name": sender_name,
                "content": final_file_name,
                "msg_type": "file",
                "file_status": "downloading",
                "file_size": file_size,
                "file_name": final_file_name,
                "sender_msg_id": sender_msg_id,
                "timestamp": timestamp,
            });
            let _ = state.ws_broadcast.send(init_msg.to_string());
            #[cfg(feature = "desktop")]
            if let Some(ref app) = state.app_handle {
                use tauri::Emitter;
                let _ = app.emit("new-message", init_msg);
            }
        }
    }

    // 广播发送端速度给前端（用 sender_msg_id 查找 DOM 元素，无需查 DB）
    if !sender_msg_id.is_empty() {
        let progress_msg = serde_json::json!({
            "msg_type": "file_download_progress",
            "sender_msg_id": sender_msg_id,
            "speed_mb_s": speed_mb_s,
            "received": chunk_data.len(),
            "total": file_size,
        });
        let _ = state.ws_broadcast.send(progress_msg.to_string());
        #[cfg(feature = "desktop")]
        if let Some(ref app) = state.app_handle {
            use tauri::Emitter;
            let _ = app.emit("new-message", progress_msg);
        }
    }

    // 检查是否是最后一块
    let temp_size = tokio::fs::metadata(&temp_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    if temp_size >= file_size && file_size > 0 {
        // 最后一块写完：将临时文件重命名为最终文件（原子操作，绕过 Android 覆盖限制）
        match tokio::fs::rename(&temp_path, &final_path).await {
            Ok(_) => {
                println!(
                    "[Web Server] ✓ 临时文件已重命名为最终文件: {:?}",
                    final_path
                );
                match crate::db::update_file_status(&state.pool, &final_file_name, "accepted").await
                {
                    Ok(_) => {
                        println!("[Web Server] ✓ 文件状态已更新为 accepted");

                        // 通知前端刷新（广播完整消息对象，让前端重新渲染）
                        let msg_id = crate::db::get_latest_msg_id_by_file(
                            &state.pool, &sender_id, &final_file_name,
                        ).await.unwrap_or(0);
                        let sender_msg_id = crate::db::get_sender_msg_id_by_file(
                            &state.pool, &sender_id, &final_file_name,
                        ).await.unwrap_or_default();
                        let file_id = std::path::Path::new(&final_file_name)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&final_file_name)
                            .to_string();
                        let sender_name = state.peer_manager.get_all_peers().iter()
                            .find(|p| p.id == sender_id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default();
                        let full_msg = serde_json::json!({
                            "id": msg_id,
                            "from_id": sender_id,
                            "from_name": sender_name,
                            "content": final_file_name,
                            "msg_type": "file",
                            "file_status": "accepted",
                            "file_path": final_path.to_str().unwrap_or(""),
                            "file_size": file_size,
                            "file_id": file_id,
                            "file_name": final_file_name,
                            "sender_msg_id": sender_msg_id,
                            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                        });
                        let _ = state.ws_broadcast.send(full_msg.to_string());
                        #[cfg(feature = "desktop")]
                        if let Some(app) = &state.app_handle {
                            // 查一下这条消息的 ID
                            let msg_id = crate::db::get_latest_msg_id_by_file(
                                &state.pool,
                                &sender_id,
                                &final_file_name,
                            )
                            .await;
                            #[cfg(feature = "desktop")]
                            use tauri::Emitter;
                        let sender_name = state.peer_manager.get_all_peers().iter()
                            .find(|p| p.id == sender_id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default();
                            let _ = app.emit(
                            "new-message",
                            serde_json::json!({
                                "id": msg_id,
                                "from_id": sender_id,
                                "from_name": sender_name,
                                "msg_type": "file",
                                "file_status": "accepted",
                                "file_path": final_path.to_str().unwrap_or(""),
                                "file_size": file_size,
                                "content": final_file_name,
                                "file_name": final_file_name,
                                "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                            }),
                        );
                        }
                    }
                    Err(e) => eprintln!("[Web Server] ✗ 更新文件状态失败: {}", e),
                }
            }
            Err(e) => {
                eprintln!("[Web Server] ✗ 重命名临时文件失败: {}", e);
            }
        }
    }

    println!("[Web Server] ========== 文件上传处理完成 ==========");
    Json(serde_json::json!({
        "status": "success",
        "file_name": final_file_name,
        "file_size": file_size,
        "chunk_index": chunk_index,
        "chunk_total": chunk_total,
    }))
    .into_response()
}

// 接受文件（手动接收模式）
#[derive(Deserialize)]
struct AcceptFileRequest {
    save_path: Option<String>,
}

async fn accept_file_http(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
    Json(payload): Json<AcceptFileRequest>,
) -> impl IntoResponse {
    println!("[Web Server] ========== 开始处理文件接收 ==========");
    println!("[Web Server] 收到接受文件请求: file_id={}", file_id);
    let _ignored_untrusted_save_path = payload.save_path;
    let Some(file_id) = safe_file_name(&file_id) else {
        return api_error(StatusCode::BAD_REQUEST, "文件 ID 无效");
    };
    let _accept_guard =
        crate::network::conversation_file::lock_receive_file(&format!("legacy-accept:{file_id}"))
            .await;

    // 先列出所有文件消息，方便调试
    println!("[Web Server] 查询所有文件消息...");
    if let Ok(rows) = crate::db::get_all_file_messages(&state.pool, 10).await {
        for (id, sender, content, path, status) in rows {
            println!(
                "[Web Server]   ID={}, sender={}, content={}, path={}, status={}",
                id, sender, content, path, status
            );
        }
    }

    let row = match sqlx::query_as::<_, (String, String)>(
        "SELECT file_path, content FROM messages
         WHERE content = ? AND file_status = 'pending'
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&file_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| format!("查询待接收文件失败: {error}"))
    {
        Ok(Some((path, name))) => {
            println!("[Web Server] ✓ 找到匹配的 pending 文件记录");
            (path, name)
        }
        Ok(None) => {
            println!("[Web Server] ✗ 未找到匹配的 pending 文件");
            println!("[Web Server] 可能原因:");
            println!("[Web Server]   1. file_id 不匹配任何文件路径");
            println!("[Web Server]   2. 文件状态不是 'pending'");
            println!("[Web Server]   3. 文件已被接收或删除");
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("文件不存在或已接收 (file_id={})", file_id),
                }),
            )
                .into_response();
        }
        Err(e) => {
            println!("[Web Server] ✗ 数据库查询失败: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("查询文件失败: {}", e),
                }),
            )
                .into_response();
        }
    };

    let temp_path = row.0;
    let Some(file_name) = safe_file_name(&row.1) else {
        return api_error(StatusCode::CONFLICT, "待接收文件名无效");
    };

    // 检查文件是否存在（可能还在上传中）
    if !std::path::Path::new(&temp_path).exists() {
        println!("[Web Server] ⏳ 文件还在下载中");
        return (
            StatusCode::ACCEPTED, // 202 表示请求已接受但还在处理中
            Json(serde_json::json!({
                "downloading": true,
                "message": "文件正在下载中，请稍候..."
            })),
        )
            .into_response();
    }

    // 客户端提供的 save_path 不可信，旧协议也只能保存到受管下载目录。
    let download_path = match crate::db::get_download_path(&state.pool).await {
        Ok(path) => path,
        Err(error) => return backend_error(error),
    };
    let final_path = std::path::PathBuf::from(download_path).join(&file_name);
    if final_path.exists() {
        return api_error(StatusCode::CONFLICT, "目标文件已存在");
    }

    println!("[Web Server] 最终路径: {:?}", final_path);

    // 确保目标目录存在
    if let Some(parent) = final_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            println!("[Web Server] ✗ 创建目录失败: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("创建目录失败: {}", e),
                }),
            )
                .into_response();
        }
    }

    // 移动文件 - 使用复制+删除来支持跨文件系统
    println!("[Web Server] 开始移动文件...");
    if let Err(e) = std::fs::rename(&temp_path, &final_path) {
        // rename 失败（可能是跨文件系统），尝试复制+删除
        println!("[Web Server] rename 失败 ({}), 尝试复制+删除", e);

        if let Err(e) = std::fs::copy(&temp_path, &final_path) {
            println!("[Web Server] ✗ 复制文件失败: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("复制文件失败: {}", e),
                }),
            )
                .into_response();
        }

        // 复制成功后删除临时文件
        if let Err(e) = std::fs::remove_file(&temp_path) {
            println!("[Web Server] ⚠ 删除临时文件失败: {}", e);
            // 不返回错误，因为文件已经复制成功了
        }

        println!("[Web Server] ✓ 文件已复制到目标位置");
    } else {
        println!("[Web Server] ✓ 文件已移动到目标位置");
    }

    // 更新数据库状态
    if let Err(e) = crate::db::update_file_status_by_path(
        &state.pool,
        &temp_path,
        final_path.to_str().unwrap(),
        "accepted",
    )
    .await
    {
        println!("[Web Server] ✗ 更新数据库失败: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("更新数据库失败: {}", e),
            }),
        )
            .into_response();
    }

    println!("[Web Server] ✓ 文件已接受并保存到: {:?}", final_path);
    Json(serde_json::json!({
        "success": true,
        "path": final_path.to_str().unwrap()
    }))
    .into_response()
}

async fn open_received_file(
    pool: &Pool<Sqlite>,
    message_id: i64,
) -> Result<(tokio::fs::File, String, u64), ApiResponse> {
    let message = crate::db::get_file_message_by_id(pool, message_id)
        .await
        .map_err(backend_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "文件消息不存在"))?;
    let self_id = crate::db::get_user_id(pool).await.map_err(backend_error)?;
    let outgoing = message.sender_id == self_id || message.sender_id == "me";
    let path = message
        .file_path
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "文件尚未下载或已被删除"))?;
    let root = tokio::fs::canonicalize(
        crate::db::get_download_path(pool)
            .await
            .map_err(backend_error)?,
    )
    .await
    .map_err(|_| api_error(StatusCode::NOT_FOUND, "下载目录不可用"))?;
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "本地文件不存在"))?;
    let allowed = if outgoing {
        let outbox = managed_web_outbox_root(pool).await.map_err(|_| {
            api_error(
                StatusCode::FORBIDDEN,
                "发送方源文件不能通过 Web 接口读取",
            )
        })?;
        canonical.starts_with(outbox)
    } else {
        canonical.starts_with(&root)
    };
    if !allowed {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            if outgoing {
                "发送方源文件不能通过 Web 接口读取"
            } else {
                "拒绝读取下载目录之外的文件"
            },
        ));
    }
    let metadata = tokio::fs::metadata(&canonical)
        .await
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "本地文件不存在"))?;
    if !metadata.is_file() {
        return Err(api_error(StatusCode::NOT_FOUND, "本地文件不存在"));
    }
    let name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&message.content)
        .to_string();
    let file = tokio::fs::File::open(canonical)
        .await
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "本地文件不可读"))?;
    Ok((file, name, metadata.len()))
}

async fn received_file_response(
    pool: &Pool<Sqlite>,
    message_id: i64,
    with_disposition: bool,
) -> ApiResponse {
    let (file, name, length) = match open_received_file(pool, message_id).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let mime = mime_guess::from_path(&name).first_or_octet_stream();
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CONTENT_LENGTH, length)
        .header(header::CACHE_CONTROL, "private, max-age=3600");
    if with_disposition {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename*=UTF-8''{}", urlencoding::encode(&name)),
        );
    }
    builder
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .unwrap()
}

// 仅按数据库消息 ID 提供已接收且位于下载目录内的文件。
async fn download_file_http(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> ApiResponse {
    let Ok(message_id) = message_id.parse::<i64>() else {
        return api_error(StatusCode::BAD_REQUEST, "无效的文件消息 ID");
    };
    received_file_response(&state.pool, message_id, true).await
}

// 获取下载目录
async fn get_download_dir(pool: &Pool<Sqlite>) -> std::path::PathBuf {
    // 从数据库读取配置
    match crate::db::get_download_path(pool).await {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            // 默认路径
            std::env::temp_dir().join("xchat-downloads")
        }
    }
}

// 创建上传记录（Web 端发送文件时）
#[derive(Deserialize)]
struct CreateUploadRecordRequest {
    file_name: String,
    file_size: u64,
    timestamp: i64,
    receiver_id: String,
    auto_download: Option<bool>,
}

async fn create_upload_record_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateUploadRecordRequest>,
) -> impl IntoResponse {
    let is_online = state
        .peer_manager
        .get_active_peers()
        .iter()
        .any(|p| p.id == payload.receiver_id);

    let auto_dl = payload.auto_download.unwrap_or(true);
    let (file_status, overall_status) = if is_online {
        if auto_dl {
            ("uploading".to_string(), "sent".to_string())
        } else {
            ("offering".to_string(), "sent".to_string())
        }
    } else {
        ("pending".to_string(), "pending".to_string())
    };

    match crate::db::create_upload_record(
        &state.pool,
        payload.receiver_id.clone(),
        payload.file_name.clone(),
        payload.file_size,
        payload.timestamp,
        file_status.clone(),
        overall_status.clone(),
    )
    .await
    {
        Ok(msg_id) => {
            Json(serde_json::json!({
                "success": true,
                "msg_id": msg_id,
                "is_online": is_online,
                "file_status": file_status,
                "status": overall_status,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

// 更新上传状态
#[derive(Deserialize)]
struct UpdateUploadStatusRequest {
    file_name: String,
    status: String,
}

async fn update_upload_status_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateUploadStatusRequest>,
) -> impl IntoResponse {
    println!(
        "[Web Server] 更新上传状态: {} -> {}",
        payload.file_name, payload.status
    );

    match crate::db::update_upload_status(
        &state.pool,
        payload.file_name.clone(),
        payload.status.clone(),
    )
    .await
    {
        Ok(_) => {
            println!("[Web Server] ✓ 上传状态已更新");
            Json(serde_json::json!({ "success": true })).into_response()
        }
        Err(e) => {
            eprintln!("[Web Server] ✗ 更新上传状态失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("更新状态失败: {}", e),
                }),
            )
                .into_response()
        }
    }
}

// 秒传完成标记（web 端发送者收到 already_exists 后更新本地状态）
#[derive(Deserialize)]
struct MarkUploadCompleteRequest {
    msg_id: i64,
    status: String,
}

async fn mark_upload_complete_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MarkUploadCompleteRequest>,
) -> impl IntoResponse {
    println!(
        "[Web Server] 标记上传完成: msg_id={}, status={}",
        payload.msg_id, payload.status
    );

    match crate::db::update_file_status_by_id(&state.pool, payload.msg_id, &payload.status).await {
        Ok(_) => {
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(e) => {
            eprintln!("[Web Server] ✗ 标记上传完成失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("标记失败: {}", e),
                }),
            )
                .into_response()
        }
    }
}

// 删除上传记录（上传失败时）
#[derive(Deserialize)]
struct DeleteUploadRecordRequest {
    file_name: String,
    timestamp: i64,
}

async fn delete_upload_record_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DeleteUploadRecordRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 删除上传记录: {}", payload.file_name);

    match crate::db::delete_upload_record(&state.pool, payload.file_name.clone(), payload.timestamp)
        .await
    {
        Ok(_) => {
            println!("[Web Server] ✓ 上传记录已删除");
            Json(serde_json::json!({ "success": true })).into_response()
        }
        Err(e) => {
            eprintln!("[Web Server] ✗ 删除上传记录失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("删除记录失败: {}", e),
                }),
            )
                .into_response()
        }
    }
}

// 主题相关的 HTTP 处理函数
async fn get_theme_list_http() -> impl IntoResponse {
    println!("[Web Server] 收到获取主题列表请求");

    let mut themes = vec![
        serde_json::json!({
            "name": "default",
            "display_name": "Default",
            "is_custom": false,
            "is_builtin": true
        }),
        serde_json::json!({
            "name": "vscode",
            "display_name": "VSCode",
            "is_custom": false,
            "is_builtin": true
        }),
    ];

    // 检查自定义主题目录
    if let Some(home_dir) = dirs::home_dir() {
    let theme_dir = home_dir.join(".config").join("xchat");

        if theme_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&theme_dir) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file()
                            && path.extension().and_then(|s| s.to_str()) == Some("css")
                        {
                            if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
                                themes.push(serde_json::json!({
                                    "name": file_name,
                                    "display_name": file_name,
                                    "is_custom": true,
                                    "is_builtin": false,
                                    "path": path.to_string_lossy()
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    println!("[Web Server] 找到 {} 个主题", themes.len());
    Json(themes).into_response()
}

async fn get_theme_css_http(Path(theme_name): Path<String>) -> impl IntoResponse {
    println!("[Web Server] 收到获取主题CSS请求: {}", theme_name);

    if theme_name == "default" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/css")
            .body(Body::from(""))
            .unwrap()
            .into_response();
    }

    // 检查是否是内置主题 vscode
    if theme_name == "vscode" {
        // 从嵌入的资源中读取 vscode.css
        if let Some(content) = Asset::get("css/vscode.css") {
            let css_content = String::from_utf8_lossy(&content.data).to_string();
            println!(
                "[Web Server] 加载内置主题: vscode ({} 字节)",
                css_content.len()
            );
            return Response::builder()
                .header(header::CONTENT_TYPE, "text/css")
                .body(Body::from(css_content))
                .unwrap()
                .into_response();
        } else {
            eprintln!("[Web Server] 内置主题 vscode.css 未找到");
        }
    }

    // 自定义主题从用户目录读取
    if let Some(home_dir) = dirs::home_dir() {
        let theme_path = home_dir
            .join(".config")
        .join("xchat")
            .join(format!("{}.css", theme_name));

        if theme_path.exists() {
            match std::fs::read_to_string(&theme_path) {
                Ok(css_content) => {
                    println!(
                        "[Web Server] 成功读取主题文件: {} ({} 字节)",
                        theme_path.display(),
                        css_content.len()
                    );
                    return Response::builder()
                        .header(header::CONTENT_TYPE, "text/css")
                        .body(Body::from(css_content))
                        .unwrap()
                        .into_response();
                }
                Err(e) => {
                    eprintln!("[Web Server] 读取主题文件失败: {}", e);
                }
            }
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("主题文件不存在: {}", theme_name),
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
struct SaveThemeRequest {
    theme_name: String,
}

async fn save_current_theme_http(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveThemeRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 收到保存主题请求: {}", req.theme_name);

    match crate::db::save_current_theme(&state.pool, req.theme_name.clone()).await {
        Ok(_) => {
            println!("[Web Server] 主题设置已保存: {}", req.theme_name);
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(e) => {
            eprintln!("[Web Server] 保存主题设置失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("保存主题设置失败: {}", e),
                }),
            )
                .into_response()
        }
    }
}

async fn get_current_theme_http(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    println!("[Web Server] 收到获取当前主题请求");

    match crate::db::get_current_theme(&state.pool).await {
        Ok(result) => {
            let theme = result.unwrap_or_else(|| "default".to_string());
            println!("[Web Server] 当前主题: {}", theme);
            Json(serde_json::json!({"theme": theme})).into_response()
        }
        Err(e) => {
            eprintln!("[Web Server] 查询主题设置失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("查询主题设置失败: {}", e),
                }),
            )
                .into_response()
        }
    }
}

// 批量删除消息
async fn delete_messages_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let msg_ids: Vec<i64> = match payload.get("msg_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_i64()).collect(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "缺少 msg_ids 参数".to_string(),
                }),
            )
                .into_response();
        }
    };

    match crate::db::delete_messages_by_ids(&state.pool, msg_ids).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "success": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

async fn clear_chat_history_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PeerIdRequest>,
) -> impl IntoResponse {
    println!(
        "[Web Server] 收到清空聊天记录请求: peer_id={}",
        payload.peer_id
    );

    // 获取自己的 ID
    let my_id = match crate::db::get_user_id(&state.pool).await {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
                .into_response()
        }
    };

    match crate::db::clear_chat_history(&state.pool, &my_id, &payload.peer_id).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "success": true }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

async fn delete_user_http(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PeerIdRequest>,
) -> impl IntoResponse {
    println!("[Web Server] 收到删除用户请求: peer_id={}", payload.peer_id);

    // 获取自己的 ID
    let my_id = match crate::db::get_user_id(&state.pool).await {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
                .into_response()
        }
    };

    match crate::db::delete_user_and_history(&state.pool, &my_id, &payload.peer_id).await {
        Ok(_) => {
            // 同步删除 Web Server 内存中的用户状态，防止轮询再次下发
            state.peer_manager.remove_peer(&payload.peer_id);

            (StatusCode::OK, Json(serde_json::json!({ "success": true }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
            .into_response(),
    }
}

/// 获取媒体访问 Token（供 Tauri command 读取后传给前端）
pub fn get_media_token() -> String {
    MEDIA_TOKEN.lock().unwrap().clone()
}

#[cfg(test)]
mod websocket_protocol_tests {
    use super::*;
    use axum::extract::FromRequest;
    use crate::network::protocol::{GroupMember, ProtocolMessage};

    #[tokio::test]
    async fn duplicate_group_message_keeps_first_mentions_without_rebroadcasting() {
        let app_dir = std::env::temp_dir().join(format!("xchat-ws-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let my_id = crate::db::get_user_id(&pool).await.unwrap();
        let (ws_broadcast, mut ws_events) = broadcast::channel(8);
        let state = AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        };

        handle_protocol_message(
            &state,
            ProtocolMessage::GroupSync {
                group_id: "group-test".into(),
                title: "Test".into(),
                created_by: "peer-a".into(),
                members: vec![
                    GroupMember {
                        peer_id: "peer-a".into(),
                        display_name: "Alice".into(),
                        role: "owner".into(),
                    },
                    GroupMember {
                        peer_id: my_id.clone(),
                        display_name: "Me".into(),
                        role: "member".into(),
                    },
                ],
                version: 1,
                timestamp: 1,
            },
        )
        .await
        .unwrap();
        let sync_event = ws_events.recv().await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sync_event).unwrap()["msg_type"],
            "group_sync"
        );
        let message = ProtocolMessage::GroupMessage {
            group_id: "group-test".into(),
            client_message_id: "message-test".into(),
            from_id: "peer-a".into(),
            from_name: "Alice".into(),
            content: "hello".into(),
            content_type: "text".into(),
            mention_ids: vec![my_id.clone(), my_id.clone()],
            timestamp: 2,
        };
        handle_protocol_message(&state, message.clone())
            .await
            .unwrap();
        let first_event = ws_events.recv().await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&first_event).unwrap()["mention_ids"],
            serde_json::json!([my_id])
        );

        handle_protocol_message(&state, message.clone()).await.unwrap();
        assert!(matches!(
            ws_events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        let mut conflicting = message;
        let ProtocolMessage::GroupMessage { mention_ids, .. } = &mut conflicting else {
            unreachable!();
        };
        mention_ids.clear();
        assert!(handle_protocol_message(&state, conflicting)
            .await
            .unwrap_err()
            .contains("mention"));
        assert!(matches!(
            ws_events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        let messages = crate::db::get_conversation_messages(&pool, "group-test", 20, 0)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        let receipts = crate::db::get_message_receipts(&pool, "message-test")
            .await
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].reader_id, my_id);
        assert!(receipts[0].mentioned);
        assert!(receipts[0].delivered_at.is_some());
        let views = crate::workspace::get_messages(&pool, &state.peer_manager, "group-test", 20, 0)
            .await
            .unwrap();
        assert_eq!(views[0].mention_ids, vec![my_id]);

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn web_file_access_stays_inside_the_download_directory() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-media-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();

        let outside = app_dir.join("outside.txt");
        tokio::fs::write(&outside, b"private").await.unwrap();
        let result = sqlx::query(
            "INSERT INTO messages
                (sender_id, receiver_id, content, msg_type, timestamp, file_path,
                 file_status, file_size, status)
             VALUES ('peer-a', 'me', 'outside.txt', 'file', 1, ?, 'accepted', 7, 'received')",
        )
        .bind(outside.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
        match open_received_file(&pool, result.last_insert_rowid()).await {
            Err(response) => assert_eq!(response.status(), StatusCode::FORBIDDEN),
            Ok(_) => panic!("outside file must not be exposed"),
        }

        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let result = sqlx::query(
            "INSERT INTO messages
                (sender_id, receiver_id, content, msg_type, timestamp, file_path,
                 file_status, file_size, status)
             VALUES (?, 'peer-a', 'outside.txt', 'file', 2, ?, 'sent', 7, 'sent')",
        )
        .bind(self_id)
        .bind(outside.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
        match open_received_file(&pool, result.last_insert_rowid()).await {
            Err(response) => assert_eq!(response.status(), StatusCode::FORBIDDEN),
            Ok(_) => panic!("outgoing source files must not be exposed"),
        }

        let inside = download_dir.join("inside.txt");
        tokio::fs::write(&inside, b"public").await.unwrap();
        let result = sqlx::query(
            "INSERT INTO messages
                (sender_id, receiver_id, content, msg_type, timestamp, file_path,
                 file_status, file_size, status)
             VALUES ('peer-a', 'me', 'inside.txt', 'file', 3, ?, 'accepted', 6, 'received')",
        )
        .bind(inside.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
        let (_, name, size) = open_received_file(&pool, result.last_insert_rowid())
            .await
            .unwrap();
        assert_eq!(name, "inside.txt");
        assert_eq!(size, 6);

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn web_uploaded_outgoing_image_remains_readable_after_send() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-web-outbox-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        tokio::fs::create_dir_all(&download_dir).await.unwrap();
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        crate::db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = Arc::new(AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        });

        let boundary = "xchat-boundary";
        let png = b"\x89PNG\r\n\x1a\nmanaged-web-image";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"capture.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(png);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let request = axum::http::Request::builder()
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let multipart = Multipart::from_request(request, &()).await.unwrap();

        let response = send_conversation_file_http(
            State(state.clone()),
            Path(conversation.id.clone()),
            multipart,
        )
        .await;
        assert!(
            response.status() == StatusCode::CREATED
                || response.status() == StatusCode::ACCEPTED
        );

        let messages = crate::db::get_conversation_messages(&pool, &conversation.id, 20, 0)
            .await
            .unwrap();
        let message = messages
            .iter()
            .find(|message| message.msg_type == "file")
            .unwrap();
        let response =
            download_file_http(State(state), Path(message.id.to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), png);

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn stable_file_chunks_complete_and_cancel_without_arbitrary_paths() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-upload-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        crate::db::set_auto_download(&pool, true).await.unwrap();
        crate::db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = Arc::new(AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        });

        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-complete",
            "101",
            "",
            "report.txt",
            6,
            0,
            2,
            b"abcd".to_vec(),
            1.0,
        )
        .await;
        assert!(response.status().is_success());
        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let complete_transfer_id = format!("file-complete:{self_id}");
        let transfer = crate::db::get_transfer(&pool, &complete_transfer_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.direction, "receive");
        assert_eq!(transfer.status, "transferring");
        assert_eq!(transfer.bytes_transferred, 4);
        let partial_path = crate::network::conversation_file::received_partial_path(
            &download_dir,
            &complete_transfer_id,
        );
        assert_eq!(tokio::fs::metadata(&partial_path).await.unwrap().len(), 4);

        let duplicate = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-complete",
            "101",
            "",
            "report.txt",
            6,
            0,
            2,
            b"abcd".to_vec(),
            1.0,
        )
        .await;
        assert!(duplicate.status().is_success());
        assert_eq!(tokio::fs::metadata(&partial_path).await.unwrap().len(), 4);
        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-complete",
            "101",
            "",
            "report.txt",
            6,
            1,
            2,
            b"ef".to_vec(),
            1.0,
        )
        .await;
        assert!(response.status().is_success());
        let completed = crate::db::get_message_by_client_id(&pool, "file-complete")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.file_status.as_deref(), Some("accepted"));
        assert_eq!(
            tokio::fs::read(completed.file_path.unwrap()).await.unwrap(),
            b"abcdef"
        );
        let transfer = crate::db::get_transfer(&pool, &complete_transfer_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.status, "completed");
        assert_eq!(transfer.bytes_transferred, 6);
        assert!(!partial_path.exists());
        assert!(
            crate::db::get_message_receipts(&pool, "file-complete")
                .await
                .unwrap()[0]
                .delivered_at
                .is_some()
        );

        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-cancel",
            "102",
            "",
            "large.bin",
            6,
            0,
            2,
            b"abcd".to_vec(),
            1.0,
        )
        .await;
        assert!(response.status().is_success());
        let cancel_transfer_id = format!("file-cancel:{self_id}");
        let response = cancel_received_upload_http(
            State(state.clone()),
            Path("file-cancel".to_string()),
            Query(CancelReceivedUploadQuery {
                transfer_id: Some(cancel_transfer_id.clone()),
                ..Default::default()
            }),
        )
        .await;
        assert!(response.status().is_success());
        let cancelled = crate::db::get_message_by_client_id(&pool, "file-cancel")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.file_status.as_deref(), Some("cancelled"));
        let transfer = crate::db::get_transfer(&pool, &cancel_transfer_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.status, "cancelled");
        assert!(!crate::network::conversation_file::received_partial_path(
            &download_dir,
            &cancel_transfer_id
        )
        .exists());

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn conflicting_parallel_prepare_does_not_reactivate_failed_transfer() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-parallel-conflict-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        crate::db::set_auto_download(&pool, true).await.unwrap();
        crate::db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let transfer_id =
            crate::network::conversation_file::recipient_transfer_id("parallel-conflict", &self_id);
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = Arc::new(AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        });
        let payload = crate::network::conversation_file::ParallelPrepareRequest {
            sender_id: "peer-a".into(),
            conversation_id: conversation.id,
            client_message_id: "parallel-conflict".into(),
            transfer_id: transfer_id.clone(),
            sender_msg_id: "parallel-sender-1".into(),
            file_name: "report.bin".into(),
            file_size: 6,
            file_sha256: "0".repeat(64),
            chunks: crate::network::conversation_file::parallel_chunk_ranges(6),
        };

        let response =
            prepare_parallel_upload_http(State(state.clone()), Json(payload.clone())).await;
        assert!(response.status().is_success());
        crate::db::update_transfer(
            &pool,
            &transfer_id,
            "failed",
            0,
            Some("interrupted"),
        )
        .await
        .unwrap();
        let message = crate::db::get_message_by_client_id(&pool, "parallel-conflict")
            .await
            .unwrap()
            .unwrap();
        crate::db::update_file_status_by_id(&pool, message.id, "failed")
            .await
            .unwrap();

        let mut conflicting = payload;
        conflicting.file_sha256 = "1".repeat(64);
        let response = prepare_parallel_upload_http(State(state), Json(conflicting)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let transfer = crate::db::get_transfer(&pool, &transfer_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.status, "failed");
        let message = crate::db::get_message_by_client_id(&pool, "parallel-conflict")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.file_status.as_deref(), Some("failed"));

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn parallel_finalize_retry_reuses_materialized_file() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-parallel-finalize-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        crate::db::set_auto_download(&pool, true).await.unwrap();
        crate::db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let transfer_id =
            crate::network::conversation_file::recipient_transfer_id("parallel-finalize", &self_id);
        let data = b"abcdef";
        let source = app_dir.join("source.bin");
        tokio::fs::write(&source, data).await.unwrap();
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = Arc::new(AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        });
        let payload = crate::network::conversation_file::ParallelPrepareRequest {
            sender_id: "peer-a".into(),
            conversation_id: conversation.id,
            client_message_id: "parallel-finalize".into(),
            transfer_id: transfer_id.clone(),
            sender_msg_id: "parallel-sender-2".into(),
            file_name: "report.bin".into(),
            file_size: data.len() as u64,
            file_sha256: crate::network::conversation_file::sha256_file(&source)
                .await
                .unwrap(),
            chunks: crate::network::conversation_file::parallel_chunk_ranges(data.len() as u64),
        };

        let response =
            prepare_parallel_upload_http(State(state.clone()), Json(payload.clone())).await;
        assert!(response.status().is_success());
        let received = crate::network::conversation_file::receive_parallel_chunk(
            &pool,
            &download_dir,
            &transfer_id,
            0,
            Body::from(data.as_slice()),
        )
        .await
        .unwrap();
        assert!(received.complete);

        // Simulate a crash after the hard link is published but before DB completion.
        let partial = crate::network::conversation_file::merge_parallel_parts(
            &download_dir,
            &received.manifest,
        )
        .await
        .unwrap();
        let (_, first_path) =
            finalize_received_file(&download_dir, &received.manifest.final_file_name, &partial)
                .await
                .unwrap();

        let message =
            finalize_parallel_receive(&state, &download_dir, &received.manifest)
                .await
                .unwrap();
        assert_eq!(message.file_path.as_deref(), first_path.to_str());
        let final_files = std::fs::read_dir(&download_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("bin"))
            .count();
        assert_eq!(final_files, 1);

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn stable_file_offer_waits_for_acceptance_when_auto_download_is_disabled() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-offer-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        crate::db::set_auto_download(&pool, false).await.unwrap();
        crate::db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = Arc::new(AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        });

        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-offer",
            "201",
            "",
            "offer.txt",
            5,
            0,
            1,
            b"hello".to_vec(),
            1.0,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let message = crate::db::get_message_by_client_id(&pool, "file-offer")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.file_status.as_deref(), Some("offered"));
        assert_eq!(message.sender_msg_id.as_deref(), Some("201"));
        assert!(!download_dir.join("offer.txt").exists());
        assert!(!download_dir.join("offer.txt.downloading").exists());
        let transfer = crate::db::get_transfer(&pool, &format!("file-offer:{self_id}"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.direction, "receive");
        assert_eq!(transfer.status, "awaiting_acceptance");
        assert_eq!(transfer.bytes_transferred, 0);

        let request = request_file_http(
            State(state.clone()),
            Json(RequestFilePayload {
                message_id: Some(message.id),
                sender_msg_id: 201,
            }),
        )
        .await
        .into_response();
        assert_eq!(request.status(), StatusCode::BAD_GATEWAY);
        let transfer = crate::db::get_transfer(&pool, &format!("file-offer:{self_id}"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.status, "failed");
        crate::db::transition_transfer_status(
            &pool,
            &transfer.id,
            "failed",
            "queued",
            0,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        crate::db::update_file_status_by_id(&pool, message.id, "downloading")
            .await
            .unwrap();
        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-offer",
            "201",
            "",
            "offer.txt",
            5,
            0,
            1,
            b"hello".to_vec(),
            1.0,
        )
        .await;
        assert!(response.status().is_success());
        let accepted = crate::db::get_message_by_client_id(&pool, "file-offer")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(accepted.file_status.as_deref(), Some("accepted"));
        let transfer = crate::db::get_transfer(&pool, &format!("file-offer:{self_id}"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.status, "completed");

        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-retry",
            "202",
            "",
            "retry.txt",
            5,
            0,
            1,
            b"retry".to_vec(),
            1.0,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let retry_message = crate::db::get_message_by_client_id(&pool, "file-retry")
            .await
            .unwrap()
            .unwrap();
        let old_transfer_id = format!("file-retry:{self_id}");
        crate::db::update_transfer(&pool, &old_transfer_id, "failed", 0, Some("network"))
            .await
            .unwrap();
        crate::db::update_file_status_by_id(&pool, retry_message.id, "failed")
            .await
            .unwrap();
        let request = request_file_http(
            State(state.clone()),
            Json(RequestFilePayload {
                message_id: Some(retry_message.id),
                sender_msg_id: 202,
            }),
        )
        .await
        .into_response();
        assert_eq!(request.status(), StatusCode::BAD_GATEWAY);
        crate::db::transition_transfer_status(
            &pool,
            &old_transfer_id,
            "failed",
            "queued",
            0,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        crate::db::update_file_status_by_id(&pool, retry_message.id, "downloading")
            .await
            .unwrap();
        let retry_transfer_id = format!("{old_transfer_id}:retry:test");
        let response = receive_conversation_file_chunk(
            &state,
            "peer-a",
            &conversation.id,
            "file-retry",
            "202",
            &retry_transfer_id,
            "retry.txt",
            5,
            0,
            1,
            b"retry".to_vec(),
            1.0,
        )
        .await;
        assert!(response.status().is_success());
        let retried_message = crate::db::get_message_by_client_id(&pool, "file-retry")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retried_message.id, retry_message.id);
        assert_eq!(
            retried_message.client_message_id.as_deref(),
            Some("file-retry")
        );
        assert_eq!(
            crate::db::get_transfer(&pool, &old_transfer_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );
        assert_eq!(
            crate::db::get_transfer(&pool, &retry_transfer_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "completed"
        );

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn stable_same_name_receives_use_distinct_partial_and_final_paths() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-same-name-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        crate::db::update_download_path(
            &pool,
            download_dir.to_string_lossy().into_owned(),
        )
        .await
        .unwrap();
        crate::db::set_auto_download(&pool, true).await.unwrap();
        crate::db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = Arc::new(AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        });

        for (client_id, sender_msg_id, chunk) in
            [("same-a", "301", b"abcd"), ("same-b", "302", b"WXYZ")]
        {
            let response = receive_conversation_file_chunk(
                &state,
                "peer-a",
                &conversation.id,
                client_id,
                sender_msg_id,
                "",
                "same.txt",
                6,
                0,
                2,
                chunk.to_vec(),
                1.0,
            )
            .await;
            assert!(response.status().is_success());
        }
        let first_partial = crate::network::conversation_file::received_partial_path(
            &download_dir,
            &format!("same-a:{self_id}"),
        );
        let second_partial = crate::network::conversation_file::received_partial_path(
            &download_dir,
            &format!("same-b:{self_id}"),
        );
        assert_ne!(first_partial, second_partial);
        assert!(first_partial.exists());
        assert!(second_partial.exists());

        for (client_id, sender_msg_id, chunk) in
            [("same-a", "301", b"ef"), ("same-b", "302", b"12")]
        {
            let response = receive_conversation_file_chunk(
                &state,
                "peer-a",
                &conversation.id,
                client_id,
                sender_msg_id,
                "",
                "same.txt",
                6,
                1,
                2,
                chunk.to_vec(),
                1.0,
            )
            .await;
            assert!(response.status().is_success());
        }
        let first = crate::db::get_message_by_client_id(&pool, "same-a")
            .await
            .unwrap()
            .unwrap();
        let second = crate::db::get_message_by_client_id(&pool, "same-b")
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first.file_path, second.file_path);
        assert_eq!(
            tokio::fs::read(first.file_path.unwrap()).await.unwrap(),
            b"abcdef"
        );
        assert_eq!(
            tokio::fs::read(second.file_path.unwrap()).await.unwrap(),
            b"WXYZ12"
        );

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[tokio::test]
    async fn group_file_validation_waits_briefly_for_group_sync() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-group-race-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let self_id = crate::db::get_user_id(&pool).await.unwrap();
        let (ws_broadcast, _) = broadcast::channel(8);
        let state = AppState {
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            media_token: String::new(),
            ws_broadcast,
            #[cfg(feature = "desktop")]
            app_handle: None,
        };
        let sync_pool = pool.clone();
        let sync_self_id = self_id.clone();
        let sync = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            crate::db::apply_group_sync(
                &sync_pool,
                "group-race",
                "Race",
                "peer-a",
                1,
                &[
                    crate::db::NewConversationMember {
                        peer_id: "peer-a".into(),
                        display_name: "Alice".into(),
                        role: "owner".into(),
                    },
                    crate::db::NewConversationMember {
                        peer_id: sync_self_id,
                        display_name: "Me".into(),
                        role: "member".into(),
                    },
                ],
            )
            .await
            .unwrap();
        });

        let conversation = validate_incoming_file_conversation(&state, "group-race", "peer-a")
            .await
            .unwrap();
        assert_eq!(conversation.kind, "group");
        sync.await.unwrap();

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[test]
    fn peer_chunk_accumulator_rejects_oversized_streams() {
        let mut chunk = vec![0; MAX_PEER_CHUNK_BYTES - 1];
        assert!(append_peer_chunk(&mut chunk, &[1]).is_ok());
        assert!(append_peer_chunk(&mut chunk, &[2]).is_err());
        assert_eq!(chunk.len(), MAX_PEER_CHUNK_BYTES);
        assert_eq!(safe_file_name("report_01.txt").as_deref(), Some("report_01.txt"));
        for unsafe_name in [
            "../../escape",
            r"C:\escape",
            "C:stream",
            "CON.txt",
            "report.txt.",
        ] {
            assert!(safe_file_name(unsafe_name).is_none(), "{unsafe_name}");
        }
    }
}

/// 媒体代理请求参数
#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct MediaQuery {
    uri: String,
    token: String,
}

#[cfg(not(target_os = "android"))]
#[derive(Deserialize)]
struct MediaQuery {
    message_id: i64,
}

/// GET /api/media?uri=<content_uri>&token=<token>
/// 仅允许本机（127.0.0.1）访问，并校验 token
#[cfg(target_os = "android")]
async fn serve_media_http(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Query(params): Query<MediaQuery>,
) -> impl IntoResponse {
    // 防线 A：IP 白名单，只允许本机回环地址
    if !addr.ip().is_loopback() {
        println!("[Media] 拒绝非本机请求: {}", addr.ip());
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    // 防线 B：Token 校验
    if params.token != state.media_token {
        println!("[Media] Token 校验失败");
        return (StatusCode::FORBIDDEN, "Invalid token").into_response();
    }

    println!("[Media] 请求媒体: {}", params.uri);

    // 获取 MIME 类型（用于 Content-Type header）
    let mime = mime_guess::from_path(params.uri.split('/').last().unwrap_or("file"))
        .first_or_octet_stream();

    // 支持 fd: 路径：从 FD 缓存获取文件句柄
    let tokio_file: tokio::fs::File;
    if params.uri.starts_with("fd:") {
        let msg_id_str = &params.uri["fd:".len()..];
        match msg_id_str.parse::<i64>() {
            Ok(msg_id) => match crate::android_fd::duplicate_cached_file(msg_id) {
                Some((f, _name, _size)) => {
                    tokio_file = f;
                }
                None => {
                    println!("[Media] FD 缓存未命中: msg_id={}", msg_id);
                    return (StatusCode::GONE, "FD cache miss").into_response();
                }
            },
            Err(_) => {
                println!("[Media] 无效的 fd: 路径: {}", params.uri);
                return (StatusCode::BAD_REQUEST, "Invalid fd: path").into_response();
            }
        }
    } else {
        // content:// URI：通过 JNI 获取 FD
        use crate::android_fd::AndroidFile;
        let android_file = match AndroidFile::from_content_uri(&params.uri) {
            Ok(f) => f,
            Err(e) => {
                println!("[Media] 获取 FD 失败（权限过期或文件不存在）: {}", e);
                return (StatusCode::FORBIDDEN, e).into_response();
            }
        };
        let std_file = android_file.into_file();
        tokio_file = tokio::fs::File::from_std(std_file);
    }

    // 流式返回
    let stream = tokio_util::io::ReaderStream::new(tokio_file);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "private, max-age=3600")
        .body(body)
        .unwrap()
        .into_response()
}

/// 桌面与 headless Web 只允许读取数据库指向的已接收文件。
#[cfg(not(target_os = "android"))]
async fn serve_media_http(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Query(params): Query<MediaQuery>,
) -> ApiResponse {
    received_file_response(&state.pool, params.message_id, false).await
}
