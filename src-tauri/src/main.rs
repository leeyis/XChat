// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use lanchat::db;
use lanchat::peers::PeerManager;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: Option<u16>,
    #[arg(long)]
    db_path: Option<String>,
}

fn show_main_window(app: &tauri::AppHandle) {
    lanchat::commands::stop_desktop_attention(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    // Workaround: WebKitGTK DMABUF renderer + NVIDIA + Wayland 导致
    // Gdk-Message: Error 71 (protocol error) dispatching to Wayland display.
    // 见 https://v2.tauri.app/develop/debug/linux-graphics/
    #[cfg(target_os = "linux")]
    std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");

    let args = Args::parse();

    let cli_port = args.port;
    let cli_db_path = args.db_path;

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 当尝试启动第二个实例时，显示已存在的窗口
            show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = lanchat::capture_editor::start(&app, None).await {
                            eprintln!("[CaptureShortcut] {error}");
                        }
                    });
                })
                .build(),
        );

    #[cfg(any(target_os = "macos", target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_notification::init());

    let app = builder
        .invoke_handler(tauri::generate_handler![
            lanchat::commands::close_android_fd,
            lanchat::commands::get_my_name,
            lanchat::commands::get_my_id,
            lanchat::commands::update_my_name,
            lanchat::commands::get_peers,
            lanchat::commands::send_message,
            lanchat::commands::get_chat_history,
            lanchat::commands::get_chat_history_with_offset,
            lanchat::commands::send_file,
            lanchat::commands::get_settings,
            lanchat::commands::update_settings,
            lanchat::commands::get_language,
            lanchat::commands::set_language,
            lanchat::commands::get_theme_list,
            lanchat::commands::get_theme_css,
            lanchat::commands::save_current_theme,
            lanchat::commands::get_current_theme,
            lanchat::commands::get_default_download_path,
            lanchat::commands::request_storage_permission,
            lanchat::commands::save_file_message,
            lanchat::commands::open_file_location,
            lanchat::commands::set_android_shared_files,
            lanchat::commands::get_android_shared_files,
            lanchat::commands::clear_android_shared_files,
            lanchat::commands::send_file_from_fd,
            lanchat::commands::share_file_to_other_app,
            lanchat::commands::open_file_in_android,
            lanchat::commands::get_media_token,
            lanchat::commands::read_clipboard_files,
            lanchat::commands::delete_messages,
            lanchat::commands::clear_chat_history,
            lanchat::commands::delete_user_complete,
            lanchat::commands::get_custom_peers,
            lanchat::commands::test_custom_peer,
            lanchat::commands::add_custom_peer,
            lanchat::commands::remove_custom_peer,
            lanchat::commands::show_notification,
            lanchat::commands::clear_notification,
            lanchat::commands::request_file,
            lanchat::commands::get_notifications_enabled,
            lanchat::commands::set_notifications_enabled,
            lanchat::commands::open_saf_picker,
            lanchat::commands::start_tray_flash,
            lanchat::commands::stop_tray_flash,
            lanchat::commands::request_permission_on_android,
            lanchat::commands::get_workspace_snapshot,
            lanchat::commands::update_workspace_preference,
            lanchat::commands::create_group,
            lanchat::commands::update_group,
            lanchat::commands::recall_conversation_message,
            lanchat::commands::forward_conversation_message,
            lanchat::commands::save_conversation_file_as,
            lanchat::commands::send_conversation_message,
            lanchat::commands::react_to_conversation_message,
            lanchat::commands::send_strong_reminder,
            lanchat::commands::show_strong_reminder,
            lanchat::commands::open_strong_reminder,
            lanchat::commands::dismiss_strong_reminder,
            lanchat::commands::send_conversation_file,
            lanchat::commands::retry_conversation_file,
            lanchat::commands::get_conversation_messages,
            lanchat::commands::mark_messages_read,
            lanchat::commands::search_workspace_messages,
            lanchat::commands::update_conversation_state,
            lanchat::commands::clear_conversation_history,
            lanchat::commands::get_file_center,
            lanchat::commands::get_transfers,
            lanchat::commands::cancel_transfer,
            lanchat::commands::update_device_metadata,
            lanchat::commands::delete_local_file,
            lanchat::commands::open_workspace_file,
            lanchat::commands::reveal_workspace_file,
            lanchat::commands::start_capture_editor,
            lanchat::commands::get_pending_capture,
            lanchat::commands::finish_capture_editor,
            lanchat::commands::save_capture_editor,
            lanchat::commands::copy_capture_editor,
            lanchat::commands::cancel_capture_editor,
            lanchat::commands::pin_capture,
            lanchat::commands::copy_pinned_capture,
            lanchat::commands::save_pinned_capture,
            lanchat::commands::resize_pinned_capture,
            lanchat::commands::set_pinned_capture_shadow,
            lanchat::commands::close_pinned_capture,
            lanchat::commands::stage_image_attachment,
            lanchat::commands::discard_staged_attachment,
            lanchat::commands::read_workspace_media,
            lanchat::commands::pick_workspace_directory,
            lanchat::commands::copy_file_message_content,
            lanchat::commands::refresh_local_ips,
            lanchat::commands::get_all_local_ips,
            lanchat::commands::set_local_ip,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // 获取主窗口并设置关闭事件处理
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            // 主窗口关闭仅隐藏到系统托盘，退出必须使用托盘菜单。
                            api.prevent_close();
                            let _ = window_clone.hide();
                        }
                        tauri::WindowEvent::Focused(true) => {
                            lanchat::commands::stop_desktop_attention(&app_handle);
                        }
                        _ => {}
                    }
                });
            }

            // 先初始化 DB，再创建托盘（托盘菜单需要读 DB）
            let db_dir = cli_db_path.or_else(|| {
                let cfg = lanchat::config_file::read_config();
                cfg.db_path
                    .as_ref()
                    .map(|p| lanchat::config_file::resolve_db_dir(p).to_string_lossy().to_string())
            });

            let pool = tauri::async_runtime::block_on(async {
                if let Some(dir) = db_dir {
                    lanchat::db::init_db_standalone(Some(std::path::PathBuf::from(dir))).await.expect("DB error")
                } else {
                    db::init_db(&handle).await.expect("DB error")
                }
            });

            let my_name = tauri::async_runtime::block_on(async {
                db::get_username(&pool).await.unwrap_or_else(|_| "Unknown".into())
            });

            let my_id = tauri::async_runtime::block_on(async {
                db::get_user_id(&pool).await.expect("无法获取或生成用户 ID")
            });

            let port: u16 = cli_port.unwrap_or_else(|| lanchat::config_file::get_port_from_config().unwrap_or(8888));

            let notif_enabled = tauri::async_runtime::block_on(async {
                lanchat::db::get_notifications_enabled(&pool).await
            });
            let capture_shortcut = tauri::async_runtime::block_on(async {
                lanchat::db::get_setting(&pool, "capture_shortcut")
                    .await
                    .ok()
                    .flatten()
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "Ctrl/⌘ ⇧ A".to_string())
            });

            handle.manage(db::DbState { pool: pool.clone() });
            handle.manage(lanchat::commands::TrayFlashState::default());
            handle.manage(lanchat::capture_shortcut::CaptureShortcutState::default());
            if let Err(error) =
                lanchat::capture_shortcut::register(&handle, &capture_shortcut)
            {
                eprintln!("[CaptureShortcut] {error}");
            }
            println!("[Main] 我的用户名: {}", my_name);
            println!("[Main] 我的 ID: {}", my_id);
            println!("[Main] 服务端口: {}", port);

            // 读取语言设置用于托盘菜单
            let tray_lang = lanchat::config_file::get_lang_from_config()
                .or_else(|| std::env::var("LANG").ok().map(|l| if l.starts_with("zh") { "zh".to_string() } else { "en".to_string() }))
                .unwrap_or_else(|| "zh".to_string());
            let (show_text, notif_text, quit_text) = if tray_lang == "en" {
                ("Show Window", "Enable Notifications", "Quit")
            } else {
                ("显示窗口", "开启通知", "退出")
            };

            // 创建托盘菜单（现在 DB 已就绪）
            let show_item = MenuItem::with_id(app, "show", show_text, true, None::<&str>)?;
            let toggle_notif = tauri::menu::CheckMenuItem::with_id(
                app, "toggle_notif", notif_text, true, notif_enabled, None::<&str>,
            )?;
            let toggle_notif_clone = toggle_notif.clone();
            let notif_enabled_atomic = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(notif_enabled));
            let quit_item = MenuItem::with_id(app, "quit", quit_text, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &toggle_notif, &quit_item])?;

            // 注册托盘菜单项到状态（供 set_language 热更新）
            #[cfg(all(feature = "desktop", not(target_os = "android")))]
            handle.manage(lanchat::commands::TrayMenuItems {
                show_item: show_item.clone(),
                toggle_notif: toggle_notif.clone(),
                quit_item: quit_item.clone(),
            });

            // 创建托盘图标
            let _tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Xchat")
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => {
                        show_main_window(app);
                        let _ = app.emit("open-latest-unread", ());
                    }
                    "toggle_notif" => {
                        let current = notif_enabled_atomic.load(std::sync::atomic::Ordering::Relaxed);
                        let new_state = !current;
                        notif_enabled_atomic.store(new_state, std::sync::atomic::Ordering::Relaxed);
                        let _ = toggle_notif_clone.set_checked(new_state);
                        // 持久化到 DB
                        let pool = {
                            let state = app.state::<lanchat::db::DbState>();
                            state.pool.clone()
                        };
                        tauri::async_runtime::block_on(async {
                            let _ = lanchat::db::set_notifications_enabled(&pool, new_state).await;
                        });
                        // 通知 JS 刷新缓存
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.emit("notifications-changed", new_state);
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button,
                        button_state,
                        ..
                    } = event
                    {
                        if button == tauri::tray::MouseButton::Left
                            && button_state == tauri::tray::MouseButtonState::Up
                        {
                            let app = tray.app_handle();
                            show_main_window(app);
                            let _ = app.emit("open-latest-unread", ());
                        }
                    }
                })
                .build(app)?;

            // 创建 PeerManager 并启动服务（DB 已就绪，无需再次 block_on）
            let peer_manager = Arc::new(PeerManager::new());
            let _ = tauri::async_runtime::block_on(async {
                if let Err(e) = peer_manager.load_from_db(&pool).await {
                    eprintln!("[Main] 加载历史用户失败: {}", e);
                }
                Ok::<(), ()>(())
            });
            handle.manage(lanchat::commands::PeerState {
                manager: peer_manager.clone(),
            });
            handle.manage(lanchat::commands::AndroidShareState::new());

            // 在 Tokio runtime 上下文中启动后台线程
            tauri::async_runtime::block_on(async {
                let h1 = handle.clone();
                let id1 = my_id.clone();
                let name1 = my_name.clone();
                let peer_manager_clone = peer_manager.clone();
                let pool_for_discovery = pool.clone();
                tokio::spawn(async move {
                    println!("[Main] 开启监听线程...");
                    lanchat::network::discovery::start_listening(
                        port, id1, name1, Some(h1), peer_manager_clone, pool_for_discovery,
                    )
                    .await;
                });

                let id2 = my_id.clone();
                let pool2 = pool.clone();
                tokio::spawn(async move {
                    println!("[Main] 开启广播线程...");
                    lanchat::network::discovery::start_announcing(port, id2, pool2).await;
                });

                let peer_manager_for_watchdog = peer_manager.clone();
                let pool_for_watchdog = pool.clone();
                let handle_for_watchdog = handle.clone();
                tokio::spawn(async move {
                    println!("[Main] 开启离线看门狗...");
                    lanchat::network::discovery::start_offline_watchdog(
                        peer_manager_for_watchdog,
                        pool_for_watchdog,
                        Some(handle_for_watchdog),
                    )
                    .await;
                });

                let pool_clone = pool.clone();
                let peer_manager_clone = peer_manager.clone();
                let handle_clone = handle.clone();
                tokio::spawn(async move {
                    println!("[Main] 启动 HTTP 服务器在端口 {}...", port);
                    lanchat::web_server::start_server(
                        port, port, pool_clone, peer_manager_clone, Some(handle_clone),
                    )
                    .await;
                });
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
