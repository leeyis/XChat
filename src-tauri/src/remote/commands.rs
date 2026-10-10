use super::*;
use crate::{commands::PeerState, db::DbState};
use tauri::{Manager, State, WebviewWindow};

fn toolbar_label(id: &str) -> String {
    format!("remote-toolbar-{id}")
}

fn main_window(window: &WebviewWindow) -> Result<()> {
    if window.label() != "main" {
        return Err("此功能只能从主窗口发起".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn remote_bootstrap(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
) -> Result<serde_json::Value> {
    main_window(&window)?;
    hub(&state.pool, &peers.manager).await?.bootstrap(true)
}
#[tauri::command]
pub async fn remote_poll(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    request: http::Poll,
) -> Result<serde_json::Value> {
    main_window(&window)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    let result = hub.poll(&request.actor, request.id.as_deref(), request.after)?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if result["session"]["phase"]
        .as_str()
        .is_some_and(|p| !["waiting", "connecting", "active"].contains(&p))
    {
        if let Some(id) = result["session"]["id"].as_str() {
            if let Some(toolbar) = window.app_handle().get_webview_window(&toolbar_label(id)) {
                let _ = toolbar.close();
            }
        }
    }
    Ok(result)
}
#[tauri::command]
pub async fn remote_screens(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
) -> Result<Vec<Screen>> {
    main_window(&window)?;
    hub(&state.pool, &peers.manager)
        .await?
        .native_actor(&actor)?;
    tokio::task::spawn_blocking(platform::screens)
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn toolbar(app: &tauri::AppHandle, hub: Arc<Hub>, actor: &str, id: &str) -> Result<()> {
    let label = toolbar_label(id);
    if let Some(toolbar) = app.get_webview_window(&label) {
        toolbar.show().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let url = format!(
        "index.html?view=remote-toolbar&actor={}&session={}",
        urlencoding::encode(actor),
        urlencoding::encode(id)
    );
    let toolbar = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::App(url.into()))
        .title("XChat 屏幕共享")
        .inner_size(730.0, 78.0)
        .position(100.0, 16.0)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|e| format!("无法显示共享控制条：{e}"))?;
    let actor = actor.to_owned();
    let id = id.to_owned();
    toolbar.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let hub = hub.clone();
            let actor = actor.clone();
            let id = id.clone();
            tauri::async_runtime::spawn(async move {
                let _ = hub
                    .action(
                        &actor,
                        &id,
                        Action::Stop {
                            reason: "ended".into(),
                        },
                    )
                    .await;
            });
        }
    });
    Ok(())
}
#[cfg(any(target_os = "android", target_os = "ios"))]
fn toolbar(_: &tauri::AppHandle, _: Arc<Hub>, _: &str, _: &str) -> Result<()> {
    Err("当前平台不支持原生屏幕共享".into())
}

async fn ensure_toolbar(
    window: &WebviewWindow,
    hub: &Arc<Hub>,
    actor: &str,
    view: &View,
) -> Result<()> {
    if view.local_host && view.native_host && view.live() {
        if let Err(error) = toolbar(window.app_handle(), hub.clone(), actor, &view.id) {
            let _ = hub
                .action(
                    actor,
                    &view.id,
                    Action::Stop {
                        reason: "ended".into(),
                    },
                )
                .await;
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn visible_toolbar(window: &WebviewWindow, id: &str) -> Result<()> {
    let toolbar = window
        .app_handle()
        .get_webview_window(&toolbar_label(id))
        .ok_or("remote_state_changed: 共享控制条正在初始化或已关闭")?;
    if !toolbar.is_visible().map_err(|e| e.to_string())?
        || toolbar.is_minimized().map_err(|e| e.to_string())?
    {
        return Err("共享控制条不可见，共享已停止".into());
    }
    Ok(())
}
#[cfg(any(target_os = "android", target_os = "ios"))]
fn visible_toolbar(_: &WebviewWindow, _: &str) -> Result<()> {
    Err("当前平台不支持原生屏幕共享".into())
}
#[tauri::command]
pub async fn remote_start(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    request: http::Start,
) -> Result<View> {
    main_window(&window)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    let view = hub
        .start(&request.actor, &request.peer_id, request.invitation)
        .await?;
    ensure_toolbar(&window, &hub, &request.actor, &view).await?;
    Ok(view)
}
#[tauri::command]
pub async fn remote_action(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    request: http::Act,
) -> Result<View> {
    main_window(&window)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    let view = hub
        .action(&request.actor, &request.id, request.action)
        .await?;
    ensure_toolbar(&window, &hub, &request.actor, &view).await?;
    Ok(view)
}
#[tauri::command]
pub async fn remote_frame(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
    id: String,
    revision: u64,
) -> Result<tauri::ipc::Response> {
    main_window(&window)?;
    visible_toolbar(&window, &id)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    Ok(tauri::ipc::Response::new(
        hub.frame(&actor, &id, revision).await?,
    ))
}
#[tauri::command]
pub async fn remote_input(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
    id: String,
    packet: input::Packet,
) -> Result<()> {
    main_window(&window)?;
    visible_toolbar(&window, &id)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    tokio::task::spawn_blocking(move || hub.input(&actor, &id, packet))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn remote_toolbar(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
    id: String,
    action: Option<Action>,
) -> Result<serde_json::Value> {
    if window.label() != toolbar_label(&id) {
        return Err("只允许共享控制条执行此操作".into());
    }
    let hub = hub(&state.pool, &peers.manager).await?;
    if let Some(action) = action {
        if !matches!(
            action,
            Action::Control { allow: false }
                | Action::Pause { .. }
                | Action::Stop { .. }
                | Action::Muted { .. }
        ) {
            return Err("共享控制条不提供此操作".into());
        }
        hub.action(&actor, &id, action).await?;
    }
    // Toolbar observation does not renew the main renderer's ownership lease.
    let state = hub.lock();
    let view = state
        .session
        .as_ref()
        .filter(|s| s.view.id == id && s.owner.as_deref() == Some(&actor))
        .map(|s| s.view.clone());
    Ok(serde_json::json!({"session":view}))
}
