use super::*;
use crate::db::DbState;
use tauri::State;

#[tauri::command]
pub async fn get_backup_overview(
    state: State<'_, DbState>,
) -> std::result::Result<serde_json::Value, String> {
    overview(&state.pool).await
}
#[tauri::command]
pub async fn start_local_backup(
    state: State<'_, DbState>,
    request: CreateRequest,
) -> std::result::Result<JobView, String> {
    create(&state.pool, request).await
}
#[tauri::command]
pub async fn get_backup_job(
    state: State<'_, DbState>,
    id: String,
) -> std::result::Result<JobView, String> {
    job_status(&state.pool, &id).await
}
#[tauri::command]
pub async fn cancel_backup_job(
    state: State<'_, DbState>,
    id: String,
) -> std::result::Result<JobView, String> {
    cancel(&state.pool, &id).await
}
#[tauri::command]
pub async fn prepare_backup_restore(
    state: State<'_, DbState>,
    path: Option<String>,
    backup_id: Option<String>,
) -> std::result::Result<JobView, String> {
    let path = match (path, backup_id) {
        (Some(path), None) => PathBuf::from(path),
        (None, Some(id)) => file_path(&state.pool, &id).await?,
        _ => return Err("请选择一个备份文件".into()),
    };
    prepare(&state.pool, path, false).await
}
#[tauri::command]
pub async fn restore_backup(
    state: State<'_, DbState>,
    id: String,
    include_settings: bool,
) -> std::result::Result<JobView, String> {
    commit_restore(&state.pool, &id, include_settings).await
}
