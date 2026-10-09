// lib.rs
#[cfg(feature = "desktop")]
pub mod commands;
// 截图编辑器依赖桌面窗口 API 与 xcap，移动端不编译。
#[cfg(all(feature = "desktop", not(any(target_os = "android", target_os = "ios"))))]
pub mod capture_editor;
#[cfg(all(feature = "desktop", not(any(target_os = "android", target_os = "ios"))))]
pub mod capture_shortcut;

#[cfg(feature = "desktop")]
use std::sync::OnceLock;
#[cfg(feature = "desktop")]
use tauri::AppHandle;

/// 全局 AppHandle 缓存，供 JNI 回调发射 Tauri 事件
#[cfg(feature = "desktop")]
pub static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub mod android_fd;
pub mod config_file;
pub mod db;
// 依赖 tauri（desktop feature 才启用），web 模式不需要。
#[cfg(feature = "desktop")]
pub mod managed_image;
pub mod models;
pub mod media;
pub mod network;
pub mod peers;
pub mod utils;
pub mod web_server;
pub mod workspace;

// 仅在桌面端编译时包含 Tauri 运行函数
#[cfg(feature = "desktop")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::sync::Arc;
    use tauri::Manager;

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_sql::Builder::default().build());

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new()
        .with_handler(capture_shortcut::handle_shortcut).build());

    #[cfg(any(target_os = "macos", target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_notification::init());

    builder
        .invoke_handler(tauri::generate_handler![
            commands::close_android_fd,
            commands::get_my_name,
            commands::get_my_id,
            commands::update_my_name,
            commands::get_peers,
            commands::refresh_peer_connection,
            commands::rediscover_peers,
            commands::send_message,
            commands::get_chat_history,
            commands::get_chat_history_with_offset,
            commands::send_file,
            commands::get_settings,
            commands::update_settings,
            commands::get_language,
            commands::set_language,
            commands::get_theme_list,
            commands::get_theme_css,
            commands::save_current_theme,
            commands::get_current_theme,
            commands::get_default_download_path,
            commands::request_storage_permission,
            commands::save_file_message,
            commands::open_file_location,
            commands::set_android_shared_files,
            commands::get_android_shared_files,
            commands::clear_android_shared_files,
            commands::send_file_from_fd,
            commands::share_file_to_other_app,
            commands::open_file_in_android,
            commands::get_media_token,
            commands::get_workspace_media_source,
            commands::delete_messages,
            commands::clear_chat_history,
            commands::request_file,
            commands::delete_user_complete,
            commands::get_custom_peers,
            commands::test_custom_peer,
            commands::add_custom_peer,
            commands::remove_custom_peer,
            commands::show_notification,
            commands::clear_notification,
            commands::get_notifications_enabled,
            commands::set_notifications_enabled,
            commands::request_permission_on_android,
            commands::start_tray_flash,
            commands::stop_tray_flash,
            commands::open_saf_picker,
            commands::open_camera_capture,
            commands::start_voice_recording,
            commands::stop_voice_recording,
            commands::get_workspace_snapshot,
            commands::sync_workspace,
            commands::update_workspace_preference,
            commands::create_group,
            commands::update_group,
            commands::recall_conversation_message,
            commands::forward_conversation_message,
            commands::save_conversation_file_as,
            commands::send_conversation_message,
            commands::react_to_conversation_message,
            commands::send_strong_reminder,
            commands::show_strong_reminder,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::open_strong_reminder,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::dismiss_strong_reminder,
            commands::send_conversation_file,
            commands::retry_conversation_file,
            commands::get_conversation_messages,
            commands::mark_messages_read,
            commands::search_workspace_messages,
            commands::update_conversation_state,
            commands::clear_conversation_history,
            commands::get_file_center,
            commands::get_transfers,
            commands::cancel_transfer,
            commands::update_device_metadata,
            commands::delete_local_file,
            commands::open_workspace_file,
            commands::reveal_workspace_file,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::start_capture_editor,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::get_pending_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::finish_capture_editor,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::save_capture_editor,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::copy_capture_editor,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::cancel_capture_editor,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::pin_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::copy_pinned_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::save_pinned_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::resize_pinned_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::set_pinned_capture_shadow,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::close_pinned_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::update_pinned_capture,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::open_pinned_capture_overlay,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::ready_pinned_capture_overlay,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::close_pinned_capture_overlay,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::list_pinned_captures,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::recover_pinned_captures,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::set_capture_pin_group,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::set_capture_preferences,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::cancel_capture_start,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::read_capture_clipboard,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            commands::write_capture_text,
            commands::stage_image_attachment,
            commands::discard_staged_attachment,
            commands::read_workspace_media,
            commands::pick_workspace_directory,
            commands::copy_file_message_content,
            commands::refresh_local_ips,
            commands::get_all_local_ips,
            commands::set_local_ip,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            APP_HANDLE.set(handle.clone()).ok();

            tauri::async_runtime::block_on(async move {
                println!("[Lib] 正在初始化数据库...");
                let pool = db::init_db(&handle).await.expect("DB error");
                let my_name = db::get_username(&pool)
                    .await
                    .unwrap_or_else(|_| "Unknown".into());

                let my_id = db::get_user_id(&pool).await.expect("无法获取或生成用户 ID");

                // 读端口：Android 端走数据库，其他平台走配置文件
                #[cfg(target_os = "android")]
                let port: u16 = crate::db::get_port(&pool).await.unwrap_or(8888);
                #[cfg(not(target_os = "android"))]
                let port: u16 = crate::config_file::get_port_from_config().unwrap_or(8888);
                println!("[Lib] 服务端口: {}", port);

                handle.manage(db::DbState { pool: pool.clone() });
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                {
                    handle.manage(capture_shortcut::CaptureShortcutState::default());
                    let shortcut = db::get_setting(&pool, "capture_shortcut").await.ok().flatten()
                        .filter(|value| !value.is_empty()).unwrap_or_else(|| "Ctrl/⌘ ⇧ A".to_string());
                    if let Err(error) = capture_shortcut::register(&handle, &shortcut) { eprintln!("[CaptureShortcut] {error}"); }
                    if let Err(error) = capture_shortcut::register_workspace(&handle) { eprintln!("[CaptureShortcut] {error}"); }
                }
                println!("[Lib] 我的用户名: {}", my_name);
                println!("[Lib] 我的 ID: {}", my_id);

                // 创建全局用户管理器
                let peer_manager = Arc::new(peers::PeerManager::new());

                // 从数据库加载历史用户
                if let Err(e) = peer_manager.load_from_db(&pool).await {
                    eprintln!("[Lib] 加载历史用户失败: {}", e);
                }

                // 将 PeerManager 注册到 Tauri 状态管理
                handle.manage(commands::PeerState {
                    manager: peer_manager.clone(),
                });

                // 注册 Android 分享状态
                handle.manage(commands::AndroidShareState::new());
                handle.manage(commands::TrayFlashState::default());

                handle.manage(network::runtime::NetworkRuntime::start(
                    network::runtime::RuntimeConfig {
                        port, user_id: my_id, username: my_name, pool,
                        peer_manager, app_handle: Some(handle.clone()),
                    },
                ));
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(runtime) = app.try_state::<network::runtime::NetworkRuntime>() {
                    tauri::async_runtime::block_on(runtime.shutdown());
                }
            }
        });
}
