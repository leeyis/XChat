use super::*;
use crate::{commands::PeerState, db::DbState};
use tauri::{Manager, State, WebviewWindow};

fn toolbar_label(id: &str) -> String {
    format!("remote-toolbar-{id}")
}

fn viewer_label(id: &str) -> String {
    format!("remote-viewer-{id}")
}

#[cfg(any(test, not(any(target_os = "android", target_os = "ios"))))]
#[derive(Debug, PartialEq)]
struct ViewerSize {
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
}

#[cfg(any(test, not(any(target_os = "android", target_os = "ios"))))]
fn viewer_size(
    work_width: u32,
    work_height: u32,
    scale: f64,
    frame_width: u32,
    frame_height: u32,
) -> Option<ViewerSize> {
    if !scale.is_finite() || scale <= 0.0 || work_width == 0 || work_height == 0 {
        return None;
    }
    // Work area and native frame measurements are physical pixels. Reserve 12
    // logical pixels on each side, then convert only the content size to logical.
    let width = ((work_width as f64 - frame_width as f64) / scale - 24.0)
        .floor()
        .clamp(1.0, 1280.0);
    let height = ((work_height as f64 - frame_height as f64) / scale - 24.0)
        .floor()
        .clamp(1.0, 800.0);
    Some(ViewerSize {
        width,
        height,
        min_width: width.min(860.0),
        min_height: height.min(600.0),
    })
}

#[cfg(any(test, not(any(target_os = "android", target_os = "ios"))))]
fn viewer_position(origin: i32, work_size: u32, outer_size: u32) -> i32 {
    // Keep the global monitor origin in physical coordinates, including negative
    // origins and mixed-DPI displays; dividing the origin by scale moves monitors.
    origin.saturating_add((work_size.saturating_sub(outer_size) / 2) as i32)
}

fn session_window(label: &str, id: Option<&str>) -> Result<bool> {
    if label == "main" {
        return Ok(false);
    }
    if id.is_some_and(|id| !id.is_empty() && label == viewer_label(id)) {
        return Ok(true);
    }
    Err("只允许主窗口或本次远程桌面窗口执行此操作".into())
}

fn scoped_viewer_poll(result: serde_json::Value, id: &str) -> serde_json::Value {
    if result["owned"].as_bool() == Some(true)
        && result["session"]["id"].as_str() == Some(id)
        && result["session"]["local_host"].as_bool() == Some(false)
    {
        result
    } else {
        serde_json::json!({"session":null,"owned":false,"signals":[]})
    }
}

fn owned_viewer(hub: &Hub, actor: &str, id: &str) -> Result<View> {
    let state = hub.lock();
    if !state.actors.contains_key(actor) {
        return Err("远程页面身份已失效".into());
    }
    let view = &state
        .session
        .as_ref()
        .filter(|session| {
            session.view.id == id
                && session.owner.as_deref() == Some(actor)
                && !session.view.local_host
        })
        .ok_or("远程桌面窗口与当前协助会话不匹配")?
        .view;
    if !view.accepted() {
        return Err("对方尚未同意或远程协助已结束".into());
    }
    Ok(view.clone())
}

fn main_window(window: &WebviewWindow) -> Result<()> {
    if window.label() != "main" {
        return Err("此功能只能从主窗口发起".into());
    }
    Ok(())
}

fn local_system_info() -> serde_json::Value {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
    // This runs once at bootstrap on a blocking worker; do not enumerate processes
    // or expose machine/user names, serials, or network addresses to a viewer.
    let system = System::new_with_specifics(
        RefreshKind::new()
            .with_cpu(CpuRefreshKind::new())
            .with_memory(MemoryRefreshKind::new().with_ram()),
    );
    let cpu = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim())
        .filter(|brand| !brand.is_empty());
    serde_json::json!({
        "os": System::name().unwrap_or_else(|| std::env::consts::OS.into()),
        "os_version": System::long_os_version().or_else(System::os_version),
        "architecture": std::env::consts::ARCH,
        "client_version": env!("CARGO_PKG_VERSION"),
        "cpu": cpu,
        "memory_bytes": system.total_memory(),
    })
}

#[tauri::command]
pub async fn remote_bootstrap(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
) -> Result<serde_json::Value> {
    main_window(&window)?;
    let mut result = hub(&state.pool, &peers.manager).await?.bootstrap(true)?;
    result["local_info"] = tokio::task::spawn_blocking(local_system_info)
        .await
        .unwrap_or_else(|error| {
            log::warn!("Unable to read remote host system information: {error}");
            serde_json::json!({
                "os": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "client_version": env!("CARGO_PKG_VERSION"),
            })
        });
    Ok(result)
}
#[tauri::command]
pub async fn remote_poll(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    request: http::Poll,
) -> Result<serde_json::Value> {
    let viewer = session_window(window.label(), request.id.as_deref())?;
    let hub = hub(&state.pool, &peers.manager).await?;
    let result = if viewer {
        let id = request.id.as_deref().unwrap_or_default();
        scoped_viewer_poll(hub.poll_viewer(&request.actor, id, request.after)?, id)
    } else {
        hub.poll(&request.actor, request.id.as_deref(), request.after)?
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if !viewer
        && result["session"]["phase"]
            .as_str()
            .is_some_and(|p| !["waiting", "connecting", "active"].contains(&p))
    {
        if let Some(id) = result["session"]["id"].as_str() {
            if let Some(toolbar) = window.app_handle().get_webview_window(&toolbar_label(id)) {
                let _ = toolbar.close();
            }
            if let Some(viewer) = window.app_handle().get_webview_window(&viewer_label(id)) {
                let _ = viewer.close();
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
        .shadow(false)
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn viewer(main: &WebviewWindow, hub: Arc<Hub>, actor: &str, view: &View) -> Result<()> {
    let app = main.app_handle();
    let label = viewer_label(&view.id);
    if let Some(viewer) = app.get_webview_window(&label) {
        viewer.unminimize().map_err(|e| e.to_string())?;
        viewer.show().map_err(|e| e.to_string())?;
        viewer.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let url = format!(
        "index.html?view=remote-viewer&actor={}&session={}",
        urlencoding::encode(actor),
        urlencoding::encode(&view.id)
    );
    let monitor = main
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| main.primary_monitor().ok().flatten());
    let viewer = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::App(url.into()))
        .title(format!("{} 的远程桌面 · Xchat", view.peer_name))
        .inner_size(860.0, 600.0)
        .min_inner_size(860.0, 600.0)
        .visible(false)
        .focused(false)
        .resizable(true)
        .build()
        .map_err(|e| format!("无法打开远程桌面窗口：{e}"))?;
    let actor = actor.to_owned();
    let id = view.id.clone();
    viewer.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let hub = hub.clone();
            let actor = actor.clone();
            let id = id.clone();
            tauri::async_runtime::spawn(async move {
                // Hub::action checks this exact ID and owner again. Closing an
                // older window cannot stop a newer session for the same actor.
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
    let placement = (|| -> Result<()> {
        if let Some(monitor) = monitor {
            let work = monitor.work_area();
            // Move the hidden window first so the OS selects this monitor's DPI.
            // Measure its real title bar/frame before choosing the content size.
            viewer
                .set_position(work.position)
                .map_err(|e| e.to_string())?;
            let scale = viewer.scale_factor().unwrap_or(monitor.scale_factor());
            let outer = viewer.outer_size().map_err(|e| e.to_string())?;
            let inner = viewer.inner_size().map_err(|e| e.to_string())?;
            if let Some(size) = viewer_size(
                work.size.width,
                work.size.height,
                scale,
                outer.width.saturating_sub(inner.width),
                outer.height.saturating_sub(inner.height),
            ) {
                viewer
                    .set_min_size(Some(tauri::LogicalSize::new(
                        size.min_width,
                        size.min_height,
                    )))
                    .map_err(|e| e.to_string())?;
                viewer
                    .set_size(tauri::LogicalSize::new(size.width, size.height))
                    .map_err(|e| e.to_string())?;
                let outer = viewer.outer_size().map_err(|e| e.to_string())?;
                viewer
                    .set_position(tauri::PhysicalPosition::new(
                        viewer_position(work.position.x, work.size.width, outer.width),
                        viewer_position(work.position.y, work.size.height, outer.height),
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        viewer.show().map_err(|e| e.to_string())?;
        viewer.set_focus().map_err(|e| e.to_string())
    })();
    if let Err(error) = placement {
        // Destroyed is already wired to the exact session, including setup errors.
        let _ = viewer.close();
        return Err(format!("无法放置远程桌面窗口：{error}"));
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn viewer(_: &WebviewWindow, _: Arc<Hub>, _: &str, _: &View) -> Result<()> {
    Err("当前平台不支持独立远程桌面窗口".into())
}

#[tauri::command]
pub async fn remote_open_viewer(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
    id: String,
) -> Result<()> {
    main_window(&window)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    let view = owned_viewer(&hub, &actor, &id)?;
    viewer(&window, hub, &actor, &view)
}

#[tauri::command]
pub async fn remote_focus_main(window: WebviewWindow, id: String) -> Result<()> {
    if window.label() != viewer_label(&id) {
        return Err("只允许本次远程桌面窗口返回聊天".into());
    }
    let main = window
        .app_handle()
        .get_webview_window("main")
        .ok_or("聊天窗口已关闭")?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    main.unminimize().map_err(|e| e.to_string())?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())
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
    visible_toolbar_handle(window.app_handle(), id)
}
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(super) fn visible_toolbar_handle(app: &tauri::AppHandle, id: &str) -> Result<()> {
    let toolbar = app
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
    let viewer = session_window(window.label(), Some(&request.id))?;
    let hub = hub(&state.pool, &peers.manager).await?;
    if viewer {
        owned_viewer(&hub, &request.actor, &request.id)?;
    }
    let view = hub
        .action(&request.actor, &request.id, request.action)
        .await?;
    ensure_toolbar(&window, &hub, &request.actor, &view).await?;
    Ok(view)
}
#[tauri::command]
pub async fn remote_capture_stream(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
    id: String,
    revision: u64,
) -> Result<capture_stream::Descriptor> {
    main_window(&window)?;
    visible_toolbar(&window, &id)?;
    let origin = window.url().map_err(|error| error.to_string())?
        .origin().ascii_serialization();
    let (port, generation) = crate::web_server::remote_capture_endpoint()?;
    hub(&state.pool, &peers.manager).await?
        .capture_stream_descriptor(actor, id, revision, origin, port, generation)
}
#[tauri::command]
pub async fn remote_frame(
    window: WebviewWindow,
    state: State<'_, DbState>,
    peers: State<'_, PeerState>,
    actor: String,
    id: String,
    revision: u64,
    format: Option<String>,
    request_id: Option<String>,
    request_keyframe: Option<bool>,
) -> Result<tauri::ipc::Response> {
    main_window(&window)?;
    let shared = format.as_deref() == Some("rgba-shared-v1");
    #[cfg(target_os = "windows")]
    let expected_url = if shared {
        Some(window.url().map_err(|error| error.to_string())?.to_string())
    } else {
        None
    };
    visible_toolbar(&window, &id)?;
    let hub = hub(&state.pool, &peers.manager).await?;
    if shared
        && !request_id.as_deref().is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err("共享屏幕帧请求标识无效".into());
    }
    let bytes = hub
        .frame(
            &actor,
            &id,
            revision,
            if shared { Some("rgba-v1") } else { format.as_deref() },
            request_keyframe.unwrap_or(false),
        )
        .await?;
    #[cfg(target_os = "windows")]
    let bytes = if shared {
        capture_shared::publish(
            &window,
            hub,
            actor,
            id,
            revision,
            request_id.unwrap_or_default(),
            expected_url.unwrap_or_default(),
            bytes,
        )
        .await?
    } else {
        bytes
    };
    Ok(tauri::ipc::Response::new(bytes))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_fits_retina_work_area_with_a_real_native_title_bar() {
        let size = viewer_size(2560, 1456, 2.0, 0, 56).unwrap();
        assert_eq!(size.width, 1256.0);
        assert_eq!(size.height, 676.0);
        assert_eq!((size.min_width, size.min_height), (860.0, 600.0));
        assert!(size.height * 2.0 + 56.0 + 48.0 <= 1456.0);
        assert_eq!(viewer_position(-2560, 2560, 2512), -2536);
        assert_eq!(viewer_position(48, 1456, 1408), 72);
    }

    #[test]
    fn viewer_minimum_shrinks_to_a_small_scaled_work_area() {
        let size = viewer_size(1200, 900, 1.5, 24, 60).unwrap();
        assert_eq!(size.width, 760.0);
        assert_eq!(size.height, 536.0);
        assert_eq!((size.min_width, size.min_height), (760.0, 536.0));
        assert!(viewer_size(0, 900, 1.5, 24, 60).is_none());
        assert!(viewer_size(1200, 900, f64::NAN, 24, 60).is_none());
    }

    #[test]
    fn viewer_commands_are_scoped_to_their_exact_session_window() {
        assert!(!session_window("main", None).unwrap());
        assert!(session_window("remote-viewer-old", Some("old")).unwrap());
        assert!(session_window("remote-viewer-old", Some("new")).is_err());
        assert!(session_window("remote-viewer-old", None).is_err());
        assert!(session_window("remote-toolbar-old", Some("old")).is_err());
        assert!(session_window("remote-viewer-", Some("")).is_err());
    }

    #[test]
    fn old_or_unowned_viewer_polls_cannot_expose_a_fresh_session_or_grant() {
        let poll = serde_json::json!({
            "session":{"id":"new","local_host":false,"grant":"new-secret","phase":"active"},
            "owned":true,
            "signals":[{"sequence":1,"body":{"type":"description","sdp":"private-sdp"}}]
        });
        let hidden = scoped_viewer_poll(poll.clone(), "old");
        assert!(hidden["session"].is_null());
        assert_eq!(hidden["signals"], serde_json::json!([]));
        assert_eq!(hidden["owned"], false);
        assert_eq!(scoped_viewer_poll(poll.clone(), "new"), poll);
        let mut unowned = poll.clone();
        unowned["owned"] = false.into();
        assert!(scoped_viewer_poll(unowned, "new")["session"].is_null());
        let mut host = poll;
        host["session"]["local_host"] = true.into();
        assert!(scoped_viewer_poll(host, "new")["session"].is_null());
    }
}
