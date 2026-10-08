use crate::managed_image::{
    data_url, png_dimensions, png_from_data_url, remove_file, write_managed_png, ManagedAttachment,
    PNG_MIME,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

const PIN_PREFIX: &str = "capture-pin-";
const PIN_MENU_PREFIX: &str = "capture-menu-";
const PIN_EDIT_PREFIX: &str = "capture-edit-";

struct CaptureTrace {
    scope: &'static str,
    timing: Option<(u64, std::time::Instant, std::time::Instant)>,
}

impl CaptureTrace {
    fn new(scope: &'static str) -> Self {
        static NEXT_TRACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let timing = (std::env::var("XCHAT_CAPTURE_TRACE").ok().as_deref() == Some("1")).then(|| {
            let now = std::time::Instant::now();
            (
                NEXT_TRACE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                now,
                now,
            )
        });
        Self { scope, timing }
    }

    fn mark(&mut self, phase: &'static str) {
        if let Some((id, started, previous)) = &mut self.timing {
            let now = std::time::Instant::now();
            eprintln!(
                "[CaptureTrace] {}#{id} {phase} phase_ms={:.2} total_ms={:.2}",
                self.scope,
                now.duration_since(*previous).as_secs_f64() * 1000.0,
                now.duration_since(*started).as_secs_f64() * 1000.0,
            );
            *previous = now;
        }
    }
}

impl Drop for CaptureTrace {
    fn drop(&mut self) {
        // Include the unfinished final phase when an operation returns early.
        self.mark("end");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PinView {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: u16,
    pub flip_x: i8,
    pub flip_y: i8,
    pub opacity: f64,
    pub shadow: bool,
    pub hidden: bool,
    pub through: bool,
    pub thumbnail: bool,
    pub group: String,
}

impl Default for PinView {
    fn default() -> Self {
        Self {
            x: 80.0,
            y: 80.0,
            scale: 1.0,
            rotation: 0,
            flip_x: 1,
            flip_y: 1,
            opacity: 1.0,
            shadow: true,
            hidden: false,
            through: false,
            thumbnail: false,
            group: "默认".to_string(),
        }
    }
}

impl PinView {
    fn validate(&self) -> Result<(), String> {
        validate_pin_scale(self.scale)?;
        if !self.x.is_finite()
            || !self.y.is_finite()
            || self.x.abs() > 1_000_000.0
            || self.y.abs() > 1_000_000.0
            || !matches!(self.rotation, 0 | 90 | 180 | 270)
            || !matches!(self.flip_x, -1 | 1)
            || !matches!(self.flip_y, -1 | 1)
            || !self.opacity.is_finite()
            || !(0.15..=1.0).contains(&self.opacity)
            || self.group.len() > 256
        {
            return Err("贴图显示参数无效".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CaptureRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub title: String,
}

#[derive(Clone)]
struct CaptureFile {
    session_id: String,
    conversation_id: Option<String>,
    path: PathBuf,
    file_name: String,
    file_size: u64,
    width: u32,
    height: u32,
    regions: Vec<CaptureRegion>,
}

#[derive(Clone)]
struct PinnedCapture {
    capture: CaptureFile,
    view: PinView,
    overlay: bool,
    applying: bool,
}

#[derive(Clone)]
struct CaptureReturnState {
    session_id: String,
    visible: bool,
    minimized: bool,
    focused: bool,
    tools: Vec<String>,
}

#[derive(Default)]
struct CaptureState {
    editor: Option<CaptureFile>,
    pins: BTreeMap<String, PinnedCapture>,
    pin_overlays: BTreeMap<String, PinOverlay>,
    restore: Option<CaptureReturnState>,
    starting: bool,
    starting_id: Option<String>,
    start_cancelled: bool,
    countdown_remaining: u64,
    active_group: Option<String>,
    delay_seconds: u64,
    capture_cursor: bool,
}

struct TempCapturePath(Option<PathBuf>);
impl TempCapturePath {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }
    fn disarm(&mut self) {
        self.0 = None;
    }
}
impl Drop for TempCapturePath {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            remove_file(&path);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePreferences {
    pub delay_seconds: u64,
    pub capture_cursor: bool,
    pub cursor_supported: bool,
}

pub fn set_preferences(
    caller: &str,
    delay_seconds: Option<u64>,
    capture_cursor: Option<bool>,
) -> Result<CapturePreferences, String> {
    if caller != "main" {
        return Err("当前窗口无权更改截图配置".to_string());
    }
    let mut state = lock_state()?;
    apply_preferences(
        &mut state,
        delay_seconds,
        capture_cursor,
        cfg!(any(target_os = "macos", all(target_os = "windows", feature = "xcap"))),
    )
}

fn apply_preferences(
    state: &mut CaptureState,
    delay_seconds: Option<u64>,
    capture_cursor: Option<bool>,
    cursor_supported: bool,
) -> Result<CapturePreferences, String> {
    if let Some(delay) = delay_seconds {
        if !matches!(delay, 0 | 3 | 5) {
            return Err("截图延时无效".to_string());
        }
    }
    if capture_cursor == Some(true) && !cursor_supported {
        return Err("当前平台暂不支持在截图中包含鼠标指针".to_string());
    }
    // Validate the complete update before changing either preference.
    if let Some(delay) = delay_seconds {
        state.delay_seconds = delay;
    }
    if let Some(capture_cursor) = capture_cursor {
        state.capture_cursor = capture_cursor;
    }
    Ok(CapturePreferences {
        delay_seconds: state.delay_seconds,
        capture_cursor: state.capture_cursor,
        cursor_supported,
    })
}

struct StartingCapture {
    app: tauri::AppHandle,
    id: String,
}
impl Drop for StartingCapture {
    fn drop(&mut self) {
        if let Ok(mut state) = lock_state() {
            if state.starting_id.as_deref() == Some(self.id.as_str()) {
                state.starting = false;
                state.starting_id = None;
                state.start_cancelled = false;
                state.countdown_remaining = 0;
            }
        }
        let _ = emit_countdown(&self.app, &self.id, 0);
    }
}

fn emit_countdown(app: &tauri::AppHandle, id: &str, remaining: u64) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        main.emit(
            "capture-countdown",
            serde_json::json!({"remaining": remaining, "session_id": id}),
        )
        .map_err(|error| format!("同步截图倒计时失败: {error}"))?;
    }
    Ok(())
}

fn mark_start_cancelled(state: &mut CaptureState, requested: Option<&str>) -> Option<String> {
    if !state.starting || state.countdown_remaining == 0 || state.editor.is_some() {
        return None;
    }
    let id = state.starting_id.as_ref()?;
    if requested.is_some_and(|requested| requested != id) {
        return None;
    }
    state.start_cancelled = true;
    state.countdown_remaining = 0;
    Some(id.clone())
}

pub fn cancel_start(app: &tauri::AppHandle, requested: Option<&str>) -> Result<bool, String> {
    let cancelled = {
        let mut state = lock_state()?;
        mark_start_cancelled(&mut state, requested)
    };
    if let Some(id) = cancelled {
        emit_countdown(app, &id, 0)?;
        return Ok(true);
    }
    Ok(false)
}

async fn countdown(app: &tauri::AppHandle, id: &str, delay: u64) -> Result<(), String> {
    for remaining in (1..=delay).rev() {
        {
            let mut state = lock_state()?;
            if state.start_cancelled {
                return Err("capture_cancelled".to_string());
            }
            state.countdown_remaining = remaining;
        }
        emit_countdown(app, id, remaining)?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    {
        let mut state = lock_state()?;
        if state.start_cancelled {
            return Err("capture_cancelled".to_string());
        }
        state.countdown_remaining = 0;
    }
    emit_countdown(app, id, 0)
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureSessionSummary {
    pub session_id: String,
    pub conversation_id: Option<String>,
    pub pin_id: Option<String>,
    pub view: Option<PinView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingCapture {
    pub session_id: String,
    pub conversation_id: Option<String>,
    pub data_url: String,
    pub mime_type: String,
    pub file_name: String,
    pub file_size: u64,
    pub width: u32,
    pub height: u32,
    pub pin_id: Option<String>,
    pub view: Option<PinView>,
    pub regions: Vec<CaptureRegion>,
    pub edit_viewport: Option<PinEditViewport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinEditViewport {
    x: f64,
    y: f64,
    pixel_ratio: f64,
}

#[derive(Clone)]
struct PinOverlay {
    pin_id: String,
    mode: String,
    edit_viewport: Option<PinEditViewport>,
    ready: bool,
    requested: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedCapture {
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CaptureClipboard {
    pub data_url: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PinViewEvent {
    pin_id: String,
    view: PinView,
}

fn state() -> &'static Mutex<CaptureState> {
    static STATE: OnceLock<Mutex<CaptureState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CaptureState::default()))
}
fn lock_state() -> Result<std::sync::MutexGuard<'static, CaptureState>, String> {
    state().lock().map_err(|_| "截图状态不可用".to_string())
}
fn pin_operations() -> &'static tokio::sync::Mutex<()> {
    static OPERATIONS: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    OPERATIONS.get_or_init(|| tokio::sync::Mutex::new(()))
}
fn capture_summary(capture: &CaptureFile, view: Option<PinView>) -> CaptureSessionSummary {
    CaptureSessionSummary {
        session_id: capture.session_id.clone(),
        conversation_id: capture.conversation_id.clone(),
        pin_id: view.as_ref().map(|_| capture.session_id.clone()),
        view,
    }
}
fn validate_pin_scale(scale: f64) -> Result<f64, String> {
    if !scale.is_finite() || !(0.1..=8.0).contains(&scale) {
        return Err("贴图缩放比例无效".to_string());
    }
    Ok(scale)
}
fn validated_pin_id(id: &str) -> Result<String, String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "贴图 ID 无效".to_string())?;
    let canonical = parsed.to_string();
    if id != canonical {
        return Err("贴图 ID 必须是标准 UUID".to_string());
    }
    Ok(canonical)
}
/// A pin window can never address another pin, even when a caller supplies an ID.
fn scoped_pin_id(caller: &str, requested: Option<&str>) -> Result<String, String> {
    let own_id = caller.strip_prefix(PIN_PREFIX).or_else(|| {
        caller
            .strip_prefix(PIN_MENU_PREFIX)
            .or_else(|| caller.strip_prefix(PIN_EDIT_PREFIX))
            .and_then(|suffix| suffix.get(..36))
    });
    if let Some(own_id) = own_id {
        let own_id = validated_pin_id(own_id)?;
        if requested.is_some_and(|id| id != own_id) {
            return Err("当前窗口无权操作其他贴图".to_string());
        }
        return Ok(own_id);
    }
    if caller == "main" {
        return requested
            .ok_or_else(|| "请选择要操作的贴图".to_string())
            .and_then(validated_pin_id);
    }
    Err("当前窗口无权操作贴图".to_string())
}
fn pinned(caller: &str, pin_id: Option<&str>) -> Result<PinnedCapture, String> {
    let id = scoped_pin_id(caller, pin_id)?;
    lock_state()?
        .pins
        .get(&id)
        .cloned()
        .ok_or_else(|| "贴图已不存在".to_string())
}
fn clear_pin(id: &str) {
    let previous = lock_state()
        .ok()
        .and_then(|mut state| state.pins.remove(id));
    if let Some(pin) = previous {
        remove_file(&pin.capture.path);
    }
}
fn clear_editor() {
    let previous = lock_state().ok().and_then(|mut state| state.editor.take());
    if let Some(capture) = previous {
        remove_file(&capture.path);
    }
}

fn clear_editor_session(session_id: &str) {
    let previous = lock_state().ok().and_then(|mut state| {
        if state
            .editor
            .as_ref()
            .is_some_and(|capture| capture.session_id == session_id)
        {
            state.editor.take()
        } else {
            None
        }
    });
    if let Some(capture) = previous {
        remove_file(&capture.path);
    }
}

fn take_editor_if_matches(
    state: &mut CaptureState,
    expected: &CaptureFile,
) -> Result<CaptureFile, String> {
    if !state.editor.as_ref().is_some_and(|current| {
        current.session_id == expected.session_id && current.path == expected.path
    }) {
        return Err("截图编辑会话已变化".to_string());
    }
    state
        .editor
        .take()
        .ok_or_else(|| "没有待处理的截图".to_string())
}

fn restore_capture_windows(app: &tauri::AppHandle, session_id: &str) -> Result<(), String> {
    let restore = lock_state()?
        .restore
        .clone()
        .filter(|restore| restore.session_id == session_id);
    let Some(restore) = restore else {
        return Ok(());
    };
    for label in &restore.tools {
        // A context menu dismissed for capture must stay dismissed afterwards.
        if label.starts_with(PIN_MENU_PREFIX) {
            continue;
        }
        let should_show = if let Some(id) = label.strip_prefix(PIN_PREFIX) {
            let state = lock_state()?;
            state.pins.get(id).is_some_and(|pin| {
                pin_is_visible(&pin.view, state.active_group.as_deref().unwrap_or("默认"))
            })
        } else {
            true
        };
        if should_show {
            if let Some(window) = app.get_webview_window(label) {
                window
                    .show()
                    .map_err(|error| format!("恢复工具窗口失败: {error}"))?;
            }
        }
    }
    if let Some(main) = app.get_webview_window("main") {
        if restore.visible {
            main.show()
                .map_err(|error| format!("恢复主窗口失败: {error}"))?;
            if restore.minimized {
                main.minimize()
                    .map_err(|error| format!("恢复主窗口最小化状态失败: {error}"))?;
            } else if restore.focused {
                main.set_focus()
                    .map_err(|error| format!("恢复主窗口焦点失败: {error}"))?;
            }
        }
    }
    let mut state = lock_state()?;
    if state
        .restore
        .as_ref()
        .is_some_and(|restore| restore.session_id == session_id)
    {
        state.restore = None;
    }
    Ok(())
}

fn show_main(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不可用".to_string())?;
    main.show()
        .map_err(|error| format!("显示主窗口失败: {error}"))?;
    main.unminimize()
        .map_err(|error| format!("恢复主窗口失败: {error}"))?;
    main.set_focus()
        .map_err(|error| format!("聚焦主窗口失败: {error}"))?;
    Ok(main)
}

#[cfg(target_os = "macos")]
fn display_number_for_geometry(
    current: (i32, i32, u32, u32),
    monitors: &[(i32, i32, u32, u32)],
) -> usize {
    monitors
        .iter()
        .position(|monitor| *monitor == current)
        .map(|index| index + 1)
        .unwrap_or(1)
}

#[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
fn grab_monitor_png(origin: (i32, i32), size: (u32, u32), path: &Path) -> Result<(), String> {
    grab_monitor_png_with_cursor(origin, size, path, false)
}

#[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
fn grab_monitor_png_with_cursor(
    origin: (i32, i32),
    size: (u32, u32),
    path: &Path,
    capture_cursor: bool,
) -> Result<(), String> {
    let mut trace = CaptureTrace::new("grab");
    #[cfg(not(target_os = "windows"))]
    if capture_cursor {
        return Err("当前平台暂不支持在截图中包含鼠标指针".to_string());
    }
    let captured = (|| {
        let monitor = xcap::Monitor::from_point(origin.0, origin.1)
            .or_else(|_| {
                xcap::Monitor::from_point(
                    origin.0.saturating_add(size.0 as i32 / 2),
                    origin.1.saturating_add(size.1 as i32 / 2),
                )
            })
            .map_err(|error| format!("无法定位当前显示器: {error}"))?;
        trace.mark("monitor");
        let result = monitor
            .capture_image()
            .map_err(|error| format!("无法读取屏幕: {error}"));
        trace.mark("capture");
        result
    })();
    #[cfg(target_os = "windows")]
    let mut image = match captured {
        Ok(image) => image,
        Err(primary) => {
            eprintln!("[Capture] xcap 抓屏失败，尝试屏幕 DC: {primary}");
            let fallback = capture_monitor_gdi(origin, size);
            trace.mark("capture_fallback");
            fallback.map_err(|fallback| {
                format!("当前桌面无法截图，请确认 XChat 所在桌面可见且允许截图后重试。{primary}；备用抓屏失败: {fallback}")
            })?
        }
    };
    #[cfg(not(target_os = "windows"))]
    let image = captured?;
    if image.width() == 0 || image.height() == 0 {
        return Err("屏幕抓取结果为空".to_string());
    }
    trace.mark("pixels_ready");
    #[cfg(target_os = "windows")]
    if capture_cursor {
        include_windows_cursor(&mut image, origin, size)?;
    }
    trace.mark("cursor");
    let bytes = encode_capture_png(&image)?;
    trace.mark("png_encode");
    std::fs::write(path, &bytes).map_err(|error| format!("写入截图失败: {error}"))?;
    trace.mark("png_write");
    Ok(())
}

#[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
fn encode_capture_png(image: &xcap::image::RgbaImage) -> Result<Vec<u8>, String> {
    use xcap::image::{
        codecs::png::{CompressionType, FilterType, PngEncoder},
        ExtendedColorType, ImageEncoder,
    };
    let mut bytes = Vec::new();
    // A fixed Sub filter is still lossless, while avoiding adaptive per-row
    // trials on the latency-sensitive path before the editor can be displayed.
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::Sub)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("编码截图失败: {error}"))?;
    Ok(bytes)
}

#[cfg(all(target_os = "windows", feature = "xcap"))]
fn cursor_draw_position(
    origin: (i32, i32),
    size: (u32, u32),
    screen: (i32, i32),
    hotspot: (u32, u32),
) -> Option<(i32, i32)> {
    let x = i64::from(screen.0) - i64::from(origin.0);
    let y = i64::from(screen.1) - i64::from(origin.1);
    // Only the display containing the pointer owns the cursor, even if its image
    // extends across the boundary. Keep signed offsets so GDI clips the hotspot.
    if x < 0 || y < 0 || x >= i64::from(size.0) || y >= i64::from(size.1) {
        return None;
    }
    Some((
        i32::try_from(x - i64::from(hotspot.0)).ok()?,
        i32::try_from(y - i64::from(hotspot.1)).ok()?,
    ))
}

#[cfg(all(target_os = "windows", feature = "xcap"))]
fn include_windows_cursor(
    image: &mut xcap::image::RgbaImage,
    origin: (i32, i32),
    size: (u32, u32),
) -> Result<(), String> {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Graphics::Gdi::DeleteObject,
        UI::{
            HiDpi::{
                SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            WindowsAndMessaging::{
                CopyIcon, DestroyIcon, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING,
                CURSOR_SUPPRESSED, HICON, ICONINFO,
            },
        },
    };
    struct CursorResources {
        icon: HICON,
        info: ICONINFO,
        dpi: DPI_AWARENESS_CONTEXT,
    }
    impl Drop for CursorResources {
        fn drop(&mut self) {
            unsafe {
                // GetIconInfo creates both bitmaps; CopyIcon owns only the copy.
                // Never destroy the shared handle returned by GetCursorInfo.
                if !self.info.hbmColor.is_null() {
                    DeleteObject(self.info.hbmColor);
                }
                if !self.info.hbmMask.is_null() {
                    DeleteObject(self.info.hbmMask);
                }
                if !self.icon.is_null() {
                    DestroyIcon(self.icon);
                }
                if !self.dpi.is_null() {
                    SetThreadDpiAwarenessContext(self.dpi);
                }
            }
        }
    }
    let mut resources = CursorResources {
        icon: ptr::null_mut(),
        info: ICONINFO::default(),
        dpi: ptr::null_mut(),
    };
    let failure = |operation: &str| {
        format!("读取鼠标指针失败（{operation}）: {}", std::io::Error::last_os_error())
    };
    // SAFETY: all outputs are initialized and owned handles stay on this worker.
    // Per-monitor awareness keeps CURSORINFO and the captured pixels in physical
    // coordinates. GetIconInfo's hotspot is not DPI virtualized by Windows.
    unsafe {
        resources.dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if resources.dpi.is_null() {
            return Err(failure("SetThreadDpiAwarenessContext"));
        }
        let mut cursor = CURSORINFO {
            cbSize: mem::size_of::<CURSORINFO>() as u32,
            ..CURSORINFO::default()
        };
        if GetCursorInfo(&mut cursor) == 0 {
            return Err(failure("GetCursorInfo"));
        }
        if cursor.flags & (CURSOR_SHOWING | CURSOR_SUPPRESSED) != CURSOR_SHOWING {
            return Ok(());
        }
        let screen = (cursor.ptScreenPos.x, cursor.ptScreenPos.y);
        if cursor_draw_position(origin, size, screen, (0, 0)).is_none() {
            return Ok(());
        }
        if image.dimensions() != size {
            return Err("屏幕像素与显示器尺寸不一致，无法准确包含鼠标指针".to_string());
        }
        resources.icon = CopyIcon(cursor.hCursor);
        if resources.icon.is_null() {
            return Err(failure("CopyIcon"));
        }
        if GetIconInfo(resources.icon, &mut resources.info) == 0 {
            return Err(failure("GetIconInfo"));
        }
        let position = cursor_draw_position(
            origin,
            size,
            screen,
            (resources.info.xHotspot, resources.info.yHotspot),
        )
        .ok_or_else(|| "鼠标指针热点坐标无效".to_string())?;
        draw_cursor_icon(image, resources.icon, position)
    }
}

#[cfg(all(target_os = "windows", feature = "xcap"))]
fn cursor_composite_region(
    image: (u32, u32),
    cursor: (u32, u32),
    position: (i32, i32),
) -> Option<(u32, u32, u32, u32)> {
    let left = i64::from(position.0).max(0);
    let top = i64::from(position.1).max(0);
    let right = (i64::from(position.0) + i64::from(cursor.0)).min(i64::from(image.0));
    let bottom = (i64::from(position.1) + i64::from(cursor.1)).min(i64::from(image.1));
    (right > left && bottom > top).then(|| {
        (
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    })
}

#[cfg(all(target_os = "windows", feature = "xcap"))]
fn draw_cursor_icon(
    image: &mut xcap::image::RgbaImage,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    position: (i32, i32),
) -> Result<(), String> {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetObjectW,
            SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
            HDC, HGDIOBJ,
        },
        UI::WindowsAndMessaging::{DrawIconEx, GetIconInfo, ICONINFO, DI_NOMIRROR, DI_NORMAL},
    };
    struct Canvas {
        memory: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
        cursor_info: ICONINFO,
    }
    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe {
                if !self.previous.is_null() && self.previous as isize != -1 {
                    SelectObject(self.memory, self.previous);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap);
                }
                if !self.cursor_info.hbmColor.is_null() {
                    DeleteObject(self.cursor_info.hbmColor);
                }
                if !self.cursor_info.hbmMask.is_null() {
                    DeleteObject(self.cursor_info.hbmMask);
                }
                if !self.memory.is_null() {
                    DeleteDC(self.memory);
                }
            }
        }
    }
    let (image_width, image_height) = image.dimensions();
    if image_width == 0
        || image_height == 0
        || image_width > 32_768
        || image_height > 32_768
        || u64::from(image_width) * u64::from(image_height) * 4 > 512 * 1024 * 1024
    {
        return Err("截图像素尺寸无效或过大".to_string());
    }
    let mut canvas = Canvas {
        memory: ptr::null_mut(),
        bitmap: ptr::null_mut(),
        previous: ptr::null_mut(),
        cursor_info: ICONINFO::default(),
    };
    let failure = |operation: &str| {
        format!("绘制鼠标指针失败（{operation}）: {}", std::io::Error::last_os_error())
    };
    // SAFETY: the top-down 32-bit DIB is bounded by the validated source image.
    // Canvas owns all allocated handles; GdiFlush precedes reading drawn pixels.
    unsafe {
        if GetIconInfo(icon, &mut canvas.cursor_info) == 0 {
            return Err(failure("GetIconInfo"));
        }
        let monochrome = canvas.cursor_info.hbmColor.is_null();
        let bitmap = if monochrome {
            canvas.cursor_info.hbmMask
        } else {
            canvas.cursor_info.hbmColor
        };
        let mut bitmap_info = BITMAP::default();
        if GetObjectW(
            bitmap,
            mem::size_of::<BITMAP>() as i32,
            (&mut bitmap_info as *mut BITMAP).cast(),
        ) == 0 {
            return Err(failure("GetObjectW"));
        }
        // Monochrome resources stack the AND and XOR masks vertically.
        let cursor_height = bitmap_info.bmHeight / if monochrome { 2 } else { 1 };
        if bitmap_info.bmWidth <= 0
            || cursor_height <= 0
            || (monochrome && bitmap_info.bmHeight % 2 != 0)
        {
            return Err("鼠标指针位图尺寸无效".to_string());
        }
        let Some((left, top, width, height)) = cursor_composite_region(
            (image_width, image_height),
            (bitmap_info.bmWidth as u32, cursor_height as u32),
            position,
        ) else {
            return Ok(());
        };
        let row_bytes = width as usize * 4;
        let byte_count = row_bytes * height as usize;
        canvas.memory = CreateCompatibleDC(ptr::null_mut());
        if canvas.memory.is_null() {
            return Err(failure("CreateCompatibleDC"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: byte_count as u32,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut bits = ptr::null_mut();
        canvas.bitmap = CreateDIBSection(
            canvas.memory,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        );
        if canvas.bitmap.is_null() || bits.is_null() {
            return Err(failure("CreateDIBSection"));
        }
        canvas.previous = SelectObject(canvas.memory, canvas.bitmap);
        if canvas.previous.is_null() || canvas.previous as isize == -1 {
            return Err(failure("SelectObject"));
        }
        {
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), byte_count);
            for (row, target_row) in pixels.chunks_exact_mut(row_bytes).enumerate() {
                let start = ((top as usize + row) * image_width as usize + left as usize) * 4;
                let source_row = &image.as_raw()[start..start + row_bytes];
                for (target, source) in target_row
                    .chunks_exact_mut(4)
                    .zip(source_row.chunks_exact(4))
                {
                    target.copy_from_slice(&[source[2], source[1], source[0], 255]);
                }
            }
        }
        // Drawing over the captured background handles alpha cursors AND classic
        // monochrome AND/XOR masks. Zero dimensions use the cursor's actual size.
        if DrawIconEx(
            canvas.memory,
            position.0 - left as i32,
            position.1 - top as i32,
            icon,
            0,
            0,
            0,
            ptr::null_mut(),
            DI_NORMAL | DI_NOMIRROR,
        ) == 0
        {
            return Err(failure("DrawIconEx"));
        }
        if GdiFlush() == 0 {
            return Err(failure("GdiFlush"));
        }
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), byte_count);
        let image_pixels: &mut [u8] = image.as_mut();
        for (row, source_row) in pixels.chunks_exact(row_bytes).enumerate() {
            let start = ((top as usize + row) * image_width as usize + left as usize) * 4;
            let target_row = &mut image_pixels[start..start + row_bytes];
            for (target, source) in target_row
                .chunks_exact_mut(4)
                .zip(source_row.chunks_exact(4))
            {
                target.copy_from_slice(&[source[2], source[1], source[0], 255]);
            }
        }
    }
    Ok(())
}

/// xcap 0.9 uses a desktop-window DC on Windows. Some desktop/session combinations
/// return invalid handles for that route; use a screen DC and owned DIB as a fallback.
/// All handles and pixels stay on this blocking worker thread.
#[cfg(all(target_os = "windows", feature = "xcap"))]
fn capture_monitor_gdi(
    origin: (i32, i32),
    size: (u32, u32),
) -> Result<xcap::image::RgbaImage, String> {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Graphics::Gdi::{
            BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC,
            ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
            DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
        },
        UI::HiDpi::{
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        },
    };
    struct Resources {
        screen: HDC,
        memory: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
        dpi: DPI_AWARENESS_CONTEXT,
    }
    impl Drop for Resources {
        fn drop(&mut self) {
            // Restore the selected bitmap before deleting it; release each handle
            // using the matching API on the same thread that acquired it.
            unsafe {
                if !self.previous.is_null()
                    && self.previous as isize != -1
                    && !self.memory.is_null()
                {
                    SelectObject(self.memory, self.previous);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap);
                }
                if !self.memory.is_null() {
                    DeleteDC(self.memory);
                }
                if !self.screen.is_null() {
                    ReleaseDC(ptr::null_mut(), self.screen);
                }
                if !self.dpi.is_null() {
                    SetThreadDpiAwarenessContext(self.dpi);
                }
            }
        }
    }
    let (width, height) = size;
    let byte_count = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes > 0 && *bytes <= 512 * 1024 * 1024)
        .ok_or_else(|| "显示器像素尺寸无效或过大".to_string())? as usize;
    if width > 32_768 || height > 32_768 {
        return Err("显示器像素尺寸无效".to_string());
    }
    let mut resources = Resources {
        screen: ptr::null_mut(),
        memory: ptr::null_mut(),
        bitmap: ptr::null_mut(),
        previous: ptr::null_mut(),
        dpi: ptr::null_mut(),
    };
    let failure = |operation: &str| format!("{operation}: {}", std::io::Error::last_os_error());
    // SAFETY: dimensions and allocation size are checked above. GDI owns the DIB
    // buffer until Resources drops, and GdiFlush completes writes before reading it.
    let mut pixels = unsafe {
        resources.dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        resources.screen = GetDC(ptr::null_mut());
        if resources.screen.is_null() {
            return Err(failure("GetDC"));
        }
        resources.memory = CreateCompatibleDC(resources.screen);
        if resources.memory.is_null() {
            return Err(failure("CreateCompatibleDC"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: byte_count as u32,
                ..mem::zeroed()
            },
            ..mem::zeroed()
        };
        let mut bits = ptr::null_mut();
        resources.bitmap = CreateDIBSection(
            resources.screen,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        );
        if resources.bitmap.is_null() || bits.is_null() {
            return Err(failure("CreateDIBSection"));
        }
        resources.previous = SelectObject(resources.memory, resources.bitmap);
        if resources.previous.is_null() || resources.previous as isize == -1 {
            return Err(failure("SelectObject"));
        }
        if BitBlt(
            resources.memory,
            0,
            0,
            width as i32,
            height as i32,
            resources.screen,
            origin.0,
            origin.1,
            SRCCOPY | CAPTUREBLT,
        ) == 0
        {
            return Err(failure("BitBlt"));
        }
        if GdiFlush() == 0 {
            return Err(failure("GdiFlush"));
        }
        std::slice::from_raw_parts(bits.cast::<u8>(), byte_count).to_vec()
    };
    // A screen DC provides BGRX rather than meaningful alpha; screenshots are opaque.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    xcap::image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| "读取屏幕像素失败".to_string())
}

#[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
fn window_regions(origin: (i32, i32), size: (u32, u32), image: (u32, u32)) -> Vec<CaptureRegion> {
    let Ok(windows) = xcap::Window::all() else {
        return Vec::new();
    };
    windows
        .into_iter()
        .filter_map(|window| {
            if window.pid().ok() == Some(std::process::id()) || window.is_minimized().ok()? {
                return None;
            }
            let x = window.x().ok()? as i64 - origin.0 as i64;
            let y = window.y().ok()? as i64 - origin.1 as i64;
            let right = (x + window.width().ok()? as i64).clamp(0, size.0 as i64);
            let bottom = (y + window.height().ok()? as i64).clamp(0, size.1 as i64);
            let left = x.clamp(0, size.0 as i64);
            let top = y.clamp(0, size.1 as i64);
            if right <= left || bottom <= top {
                return None;
            }
            let sx = image.0 as f64 / size.0 as f64;
            let sy = image.1 as f64 / size.1 as f64;
            Some(CaptureRegion {
                x: (left as f64 * sx).round() as u32,
                y: (top as f64 * sy).round() as u32,
                width: ((right - left) as f64 * sx).round() as u32,
                height: ((bottom - top) as f64 * sy).round() as u32,
                title: window.title().unwrap_or_default(),
            })
        })
        .filter(|region| region.width > 0 && region.height > 0)
        .collect()
}

fn validate_capture_conversation_id(
    conversation_id: Option<String>,
) -> Result<Option<String>, String> {
    if conversation_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
    {
        return Err("无效的会话 ID".to_string());
    }
    Ok(conversation_id)
}

pub async fn start(
    app: &tauri::AppHandle,
    conversation_id: Option<String>,
    delay: Option<u64>,
) -> Result<CaptureSessionSummary, String> {
    let mut trace = CaptureTrace::new("start");
    let conversation_id = validate_capture_conversation_id(conversation_id)?;
    // A countdown and a global shortcut use the same immutable preference snapshot.
    let (delay, capture_cursor) = {
        let state = lock_state()?;
        (delay.unwrap_or(state.delay_seconds), state.capture_cursor)
    };
    if !matches!(delay, 0 | 3 | 5) {
        return Err("截图延时无效".to_string());
    }
    if let Some(window) = app.get_webview_window("capture-editor") {
        let summary = lock_state()?
            .editor
            .as_ref()
            .map(|item| capture_summary(item, None))
            .ok_or_else(|| "截图编辑器状态不可用".to_string())?;
        window
            .show()
            .map_err(|error| format!("显示截图失败: {error}"))?;
        window
            .set_focus()
            .map_err(|error| format!("聚焦截图失败: {error}"))?;
        return Ok(summary);
    }
    let starting_id = uuid::Uuid::new_v4().to_string();
    {
        let mut state = lock_state()?;
        if state.starting {
            return Err("截图正在启动".to_string());
        }
        state.starting = true;
        state.starting_id = Some(starting_id.clone());
        state.start_cancelled = false;
        state.countdown_remaining = delay;
    }
    let _starting = StartingCapture {
        app: app.clone(),
        id: starting_id.clone(),
    };
    #[cfg(any(
        target_os = "macos",
        all(any(target_os = "windows", target_os = "linux"), feature = "xcap")
    ))]
    {
        countdown(app, &starting_id, delay).await?;
        let main = app
            .get_webview_window("main")
            .ok_or_else(|| "主窗口不可用".to_string())?;
        let monitor = main
            .cursor_position()
            .ok()
            .and_then(|point| main.monitor_from_point(point.x, point.y).ok().flatten())
            .or_else(|| main.current_monitor().ok().flatten())
            .ok_or_else(|| "当前显示器不可用".to_string())?;
        let origin = (monitor.position().x, monitor.position().y);
        let size = (monitor.size().width, monitor.size().height);
        #[cfg(target_os = "macos")]
        let display_number = {
            let geometries = main
                .available_monitors()
                .map_err(|error| format!("读取显示器失败: {error}"))?
                .into_iter()
                .map(|item| {
                    (
                        item.position().x,
                        item.position().y,
                        item.size().width,
                        item.size().height,
                    )
                })
                .collect::<Vec<_>>();
            display_number_for_geometry((origin.0, origin.1, size.0, size.1), &geometries)
                .to_string()
        };
        let mut restore = CaptureReturnState {
            session_id: starting_id.clone(),
            visible: main.is_visible().map_err(|error| error.to_string())?,
            minimized: main.is_minimized().map_err(|error| error.to_string())?,
            focused: main.is_focused().map_err(|error| error.to_string())?,
            tools: Vec::new(),
        };
        for (label, window) in app.webview_windows() {
            if label != "main" && window.is_visible().unwrap_or(false) {
                restore.tools.push(label);
            }
        }
        lock_state()?.restore = Some(restore.clone());
        trace.mark("prepare");
        let result = async {
            if restore.visible {
                main.hide()
                    .map_err(|error| format!("隐藏主窗口失败: {error}"))?;
            }
            for label in &restore.tools {
                if let Some(window) = app.get_webview_window(label) {
                    window
                        .hide()
                        .map_err(|error| format!("隐藏工具窗口失败: {error}"))?;
                }
            }
            trace.mark("hide");
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            trace.mark("settle");
            let capture_dir = std::env::temp_dir().join("xchat-captures");
            tokio::fs::create_dir_all(&capture_dir)
                .await
                .map_err(|error| format!("创建截图缓存失败: {error}"))?;
            trace.mark("cache_dir");
            let session_id = starting_id.clone();
            let path = capture_dir.join(format!("{session_id}.png"));
            let mut temporary = TempCapturePath::new(path.clone());
            let grab_path = path.clone();
            #[cfg(target_os = "macos")]
            tokio::task::spawn_blocking(move || {
                let mut command = std::process::Command::new("/usr/sbin/screencapture");
                if capture_cursor {
                    // The system's non-interactive capture includes the real cursor.
                    command.arg("-C");
                }
                let status = command.args(["-x", "-D"])
                    .arg(display_number)
                    .arg(grab_path)
                    .status()
                    .map_err(|error| format!("启动系统截图失败: {error}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err("无法读取屏幕，请在系统设置中允许 XChat 使用屏幕录制权限".to_string())
                }
            })
            .await
            .map_err(|error| error.to_string())??;
            #[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
            tokio::task::spawn_blocking(move || {
                if capture_cursor {
                    grab_monitor_png_with_cursor(origin, size, &grab_path, true)
                } else {
                    grab_monitor_png(origin, size, &grab_path)
                }
            })
                .await
                .map_err(|error| error.to_string())??;
            trace.mark("grab_total");
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|error| format!("读取截图失败: {error}"))?;
            trace.mark("read_png");
            let (width, height) = png_dimensions(&bytes)?;
            trace.mark("png_header");
            #[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
            let regions =
                tokio::task::spawn_blocking(move || window_regions(origin, size, (width, height)))
                    .await
                    .map_err(|error| error.to_string())?;
            #[cfg(not(all(any(target_os = "windows", target_os = "linux"), feature = "xcap")))]
            let regions = Vec::new();
            trace.mark("regions");
            let capture = CaptureFile {
                session_id,
                conversation_id,
                path,
                file_name: "capture.png".to_string(),
                file_size: bytes.len() as u64,
                width,
                height,
                regions,
            };
            clear_editor();
            lock_state()?.editor = Some(capture.clone());
            temporary.disarm();
            trace.mark("session");
            let factor = monitor.scale_factor();
            let window = WebviewWindowBuilder::new(
                app,
                "capture-editor",
                WebviewUrl::App("index.html?view=capture-editor".into()),
            )
            .title("XChat 截图")
            .inner_size(size.0 as f64 / factor, size.1 as f64 / factor)
            .position(origin.0 as f64 / factor, origin.1 as f64 / factor)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .build()
            .map_err(|error| format!("打开截图编辑器失败: {error}"))?;
            trace.mark("window_build");
            let close_app = app.clone();
            let close_session = capture.session_id.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    clear_editor_session(&close_session);
                    if let Err(error) = restore_capture_windows(&close_app, &close_session) {
                        eprintln!("[Capture] {error}");
                    }
                }
            });
            let show_result = (|| {
                window
                    .set_position(tauri::Position::Physical(*monitor.position()))
                    .map_err(|error| error.to_string())?;
                window
                    .set_size(tauri::Size::Physical(*monitor.size()))
                    .map_err(|error| error.to_string())?;
                window
                    .set_fullscreen(true)
                    .map_err(|error| format!("设置全屏截图失败: {error}"))?;
                window
                    .show()
                    .map_err(|error| format!("显示截图失败: {error}"))?;
                window
                    .set_focus()
                    .map_err(|error| format!("聚焦截图失败: {error}"))
            })();
            trace.mark("window_show");
            if let Err(error) = show_result {
                let _ = window.close();
                return Err(error);
            }
            Ok(capture_summary(&capture, None))
        }
        .await;
        if result.is_err() {
            clear_editor_session(&starting_id);
            if let Err(error) = restore_capture_windows(app, &starting_id) {
                return Err(format!("{}；{error}", result.unwrap_err()));
            }
        }
        result
    }
    #[cfg(not(any(
        target_os = "macos",
        all(any(target_os = "windows", target_os = "linux"), feature = "xcap")
    )))]
    {
        let _ = (app, conversation_id);
        Err("capture_unsupported".to_string())
    }
}

async fn decode_png(value: String) -> Result<(Vec<u8>, u32, u32), String> {
    tokio::task::spawn_blocking(move || png_from_data_url(&value))
        .await
        .map_err(|error| error.to_string())?
}

pub async fn pending_for_window(caller: &str) -> Result<PendingCapture, String> {
    let mut trace = CaptureTrace::new("pending");
    let (capture, view) = if caller == "capture-editor" {
        (
            lock_state()?
                .editor
                .clone()
                .ok_or_else(|| "没有待处理的截图".to_string())?,
            None,
        )
    } else {
        let pin = pinned(caller, None)?;
        (pin.capture, Some(pin.view))
    };
    let edit_viewport = lock_state()?
        .pin_overlays
        .get(caller)
        .and_then(|overlay| overlay.edit_viewport.clone());
    trace.mark("state");
    // A menu only needs the view settings, never the PNG or editable document.
    let bytes = if caller.starts_with(PIN_MENU_PREFIX) {
        Vec::new()
    } else {
        tokio::fs::read(&capture.path)
            .await
            .map_err(|error| format!("截图已不可用: {error}"))?
    };
    trace.mark("read_png");
    tokio::task::spawn_blocking(move || {
        if !bytes.is_empty() {
            png_dimensions(&bytes)?;
        }
        trace.mark("png_header");
        let data_url = if bytes.is_empty() {
            String::new()
        } else {
            data_url(PNG_MIME, &bytes)
        };
        trace.mark("base64");
        Ok(PendingCapture {
            pin_id: view.as_ref().map(|_| capture.session_id.clone()),
            view,
            session_id: capture.session_id,
            conversation_id: capture.conversation_id,
            data_url,
            mime_type: PNG_MIME.to_string(),
            file_name: capture.file_name,
            file_size: capture.file_size,
            width: capture.width,
            height: capture.height,
            regions: capture.regions,
            edit_viewport,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

pub fn list_pins(caller: &str) -> Result<Vec<CaptureSessionSummary>, String> {
    if caller != "main" {
        return Err("当前窗口无权读取贴图列表".to_string());
    }
    Ok(lock_state()?
        .pins
        .values()
        .map(|pin| capture_summary(&pin.capture, Some(pin.view.clone())))
        .collect())
}

/// Menus and annotation tools get their own surface; the image window never
/// changes its frame, position, or size merely to make room for controls.
pub async fn open_pin_overlay(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    mode: &str,
    x: f64,
    y: f64,
) -> Result<(), String> {
    prepare_pin_overlay(app, caller, pin_id, mode, x, y, true).await
}

fn preload_pin_menu(app: &tauri::AppHandle, id: &str) {
    let app = app.clone();
    let id = id.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(error) =
            prepare_pin_overlay(&app, "main", Some(&id), "menu", 0.0, 0.0, false).await
        {
            eprintln!("[CapturePin] 预加载菜单失败: {error}");
        }
    });
}

async fn prepare_pin_overlay(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    mode: &str,
    x: f64,
    y: f64,
    requested: bool,
) -> Result<(), String> {
    if !matches!(mode, "menu" | "edit") || !x.is_finite() || !y.is_finite() {
        return Err("贴图浮层参数无效".to_string());
    }
    let id = scoped_pin_id(caller, pin_id)?;
    let _operation = pin_operations().lock().await;
    let pin = pinned(caller, Some(&id))?;
    let parent = app
        .get_webview_window(&format!("{PIN_PREFIX}{id}"))
        .ok_or_else(|| "贴图窗口不可用".to_string())?;
    let old = lock_state()?
        .pin_overlays
        .iter()
        .filter(|(_, overlay)| overlay.pin_id == id)
        .map(|(label, overlay)| (label.clone(), overlay.mode.clone()))
        .collect::<Vec<_>>();
    let mut cached_menu = None;
    for (label, previous_mode) in old {
        if previous_mode == "edit" {
            if let Some(window) = app.get_webview_window(&label).filter(|_| requested) {
                window.set_focus().map_err(|error| error.to_string())?;
            }
            return Ok(());
        }
        if mode == "menu" {
            if !requested {
                return Ok(());
            }
            cached_menu = app.get_webview_window(&label);
        } else {
            close_pin_overlay_window_locked(app, &label).await?;
        }
    }
    let position = parent.inner_position().map_err(|error| error.to_string())?;
    let monitor = parent
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "当前显示器不可用".to_string())?;
    let factor = monitor.scale_factor();
    let (left, top, width, height, edit_viewport) = if mode == "edit" {
        (
            monitor.position().x,
            monitor.position().y,
            monitor.size().width,
            monitor.size().height,
            Some(PinEditViewport {
                x: (position.x - monitor.position().x) as f64 / factor,
                y: (position.y - monitor.position().y) as f64 / factor,
                pixel_ratio: factor,
            }),
        )
    } else {
        let width = (268.0 * factor).round() as u32;
        let height = (530.0 * factor).round() as u32;
        let left = (position.x as f64 + x * factor).round() as i32;
        let top = (position.y as f64 + y * factor).round() as i32;
        (
            left.clamp(
                monitor.position().x,
                (monitor.position().x + monitor.size().width as i32 - width as i32)
                    .max(monitor.position().x),
            ),
            top.clamp(
                monitor.position().y,
                (monitor.position().y + monitor.size().height as i32 - height as i32)
                    .max(monitor.position().y),
            ),
            width.min(monitor.size().width),
            height.min(monitor.size().height),
            None,
        )
    };
    if let Some(window) = cached_menu {
        window
            .set_position(tauri::PhysicalPosition::new(left, top))
            .and_then(|_| window.set_size(tauri::PhysicalSize::new(width, height)))
            .map_err(|error| error.to_string())?;
        if let Some(overlay) = lock_state()?.pin_overlays.get_mut(window.label()) {
            overlay.requested = true;
        }
        // The existing renderer acknowledges the updated controls before showing.
        // No new WebView, image decoding or document load is on the click path.
        window
            .emit(
                "capture-pin-menu-open",
                capture_summary(&pin.capture, Some(pin.view)),
            )
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let prefix = if mode == "menu" {
        PIN_MENU_PREFIX
    } else {
        PIN_EDIT_PREFIX
    };
    let label = format!("{prefix}{id}-{}", uuid::Uuid::new_v4());
    let window = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(format!("index.html?view=capture-pin-{mode}&pinId={id}").into()),
    )
    .title(if mode == "menu" {
        "XChat 贴图操作"
    } else {
        "XChat 贴图标注"
    })
    .inner_size(width as f64 / factor, height as f64 / factor)
    .decorations(false)
    .transparent(true)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .focused(false)
    .build()
    .map_err(|error| format!("打开贴图浮层失败: {error}"))?;
    let prepared = window
        .set_position(tauri::PhysicalPosition::new(left, top))
        .and_then(|_| window.set_size(tauri::PhysicalSize::new(width, height)));
    if let Err(error) = prepared {
        let _ = window.destroy();
        return Err(error.to_string());
    }
    lock_state()?.pin_overlays.insert(
        label.clone(),
        PinOverlay {
            pin_id: id.clone(),
            mode: mode.to_string(),
            edit_viewport,
            ready: false,
            requested,
        },
    );
    if mode == "edit" {
        lock_state()?
            .pins
            .get_mut(&id)
            .ok_or_else(|| "贴图已不存在".to_string())?
            .overlay = true;
        if let Err(error) = parent.set_ignore_cursor_events(true) {
            lock_state()?.pin_overlays.remove(&label);
            if let Some(pin) = lock_state()?.pins.get_mut(&id) {
                pin.overlay = false;
            }
            let _ = window.destroy();
            return Err(error.to_string());
        }
    }
    let handle = app.clone();
    let event_label = label.clone();
    let menu = mode == "menu";
    window.on_window_event(move |event| {
        let dismiss = menu
            && matches!(event, tauri::WindowEvent::Focused(false))
            && lock_state()
                .ok()
                .and_then(|state| {
                    state
                        .pin_overlays
                        .get(&event_label)
                        .map(|overlay| overlay.ready && overlay.requested)
                })
                .unwrap_or(false);
        if matches!(event, tauri::WindowEvent::Destroyed) || dismiss {
            let handle = handle.clone();
            let label = event_label.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = close_pin_overlay_window(&handle, &label).await {
                    eprintln!("关闭贴图浮层失败: {error}");
                }
            });
        }
    });
    // A failed WebView load must not leave the original pin unreachable.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        let failed = lock_state()
            .ok()
            .and_then(|state| state.pin_overlays.get(&label).map(|overlay| !overlay.ready))
            .unwrap_or(false);
        if failed {
            let _ = close_pin_overlay_window(&handle, &label).await;
            if let Ok(mut state) = lock_state() {
                state.pin_overlays.remove(&label);
            }
            if let Some(window) = handle.get_webview_window(&label) {
                let _ = window.destroy();
            }
        }
    });
    Ok(())
}

pub async fn ready_pin_overlay(app: &tauri::AppHandle, caller: &str) -> Result<(), String> {
    let _operation = pin_operations().lock().await;
    let overlay = lock_state()?
        .pin_overlays
        .get(caller)
        .cloned()
        .ok_or_else(|| "贴图浮层已关闭".to_string())?;
    if !overlay.requested {
        if let Some(overlay) = lock_state()?.pin_overlays.get_mut(caller) {
            overlay.ready = true;
        }
        return Ok(());
    }
    let window = app
        .get_webview_window(caller)
        .ok_or_else(|| "贴图浮层不可用".to_string())?;
    let parent = app.get_webview_window(&format!("{PIN_PREFIX}{}", overlay.pin_id));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = (|| {
            window.show().map_err(|error| error.to_string())?;
            if overlay.mode == "edit" {
                if let Some(parent) = parent {
                    parent.hide().map_err(|error| error.to_string())?;
                }
            }
            window.set_focus().map_err(|error| error.to_string())
        })();
        let _ = sender.send(result);
    })
    .map_err(|error| error.to_string())?;
    receiver.await.map_err(|error| error.to_string())??;
    if let Some(overlay) = lock_state()?.pin_overlays.get_mut(caller) {
        overlay.ready = true;
    }
    Ok(())
}

async fn close_pin_overlay_window(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let _operation = pin_operations().lock().await;
    close_pin_overlay_window_locked(app, label).await
}

async fn close_pin_overlay_window_locked(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(), String> {
    let Some(overlay) = lock_state()?.pin_overlays.get(label).cloned() else {
        return Ok(());
    };
    let pin = lock_state()?.pins.get(&overlay.pin_id).cloned();
    let parent = app.get_webview_window(&format!("{PIN_PREFIX}{}", overlay.pin_id));
    if overlay.mode == "menu" {
        if let Some(current) = lock_state()?.pin_overlays.get_mut(label) {
            current.requested = false;
        }
        if let Some(window) = app.get_webview_window(label) {
            window.hide().map_err(|error| error.to_string())?;
        } else {
            lock_state()?.pin_overlays.remove(label);
        }
        if let Some(parent) = parent {
            let _ = parent.emit("capture-pin-menu-closed", ());
        }
        return Ok(());
    }
    if overlay.mode == "edit" {
        if let (Some(pin), Some(parent)) = (&pin, &parent) {
            // Apply an intentional crop change while the original window is hidden.
            apply_pin_geometry(parent, &pin.capture, &pin.view, false)?;
        }
    }
    lock_state()?.pin_overlays.remove(label);
    if overlay.mode == "menu" {
        if let Some(parent) = &parent {
            let _ = parent.emit("capture-pin-menu-closed", ());
        }
    }
    if overlay.mode == "edit" {
        if let Some(pin) = lock_state()?.pins.get_mut(&overlay.pin_id) {
            pin.overlay = false;
        }
    }
    let window = app.get_webview_window(label);
    let visible = pin.as_ref().is_some_and(|pin| {
        pin_is_visible(&pin.view, &active_group().unwrap_or_else(|_| "默认".into()))
    });
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = (|| {
            if overlay.mode == "edit" && visible {
                if let Some(parent) = parent {
                    parent.show().map_err(|error| error.to_string())?;
                    parent.set_focus().map_err(|error| error.to_string())?;
                }
            }
            if let Some(window) = window {
                window.destroy().map_err(|error| error.to_string())?;
            }
            Ok(())
        })();
        let _ = sender.send(result);
    })
    .map_err(|error| error.to_string())?;
    receiver.await.map_err(|error| error.to_string())?
}

pub async fn close_pin_overlay(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    mode: Option<&str>,
) -> Result<(), String> {
    let id = scoped_pin_id(caller, pin_id)?;
    let _operation = pin_operations().lock().await;
    let labels = lock_state()?
        .pin_overlays
        .iter()
        .filter(|(label, overlay)| {
            overlay.pin_id == id
                && (label.as_str() == caller
                    || mode == Some(overlay.mode.as_str())
                        && (caller == "main" || caller.starts_with(PIN_PREFIX)))
        })
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    for label in labels {
        close_pin_overlay_window_locked(app, &label).await?;
    }
    Ok(())
}

fn current_capture(caller: &str) -> Result<CaptureFile, String> {
    if caller == "capture-editor" {
        lock_state()?
            .editor
            .clone()
            .ok_or_else(|| "没有待处理的截图".to_string())
    } else {
        Ok(pinned(caller, None)?.capture)
    }
}
fn ensure_current(caller: &str, capture: &CaptureFile) -> Result<(), String> {
    let current = current_capture(caller)?;
    if current.session_id != capture.session_id || current.path != capture.path {
        return Err("截图会话已变化".to_string());
    }
    Ok(())
}

pub async fn finish(
    app: &tauri::AppHandle,
    caller: &str,
    value: String,
) -> Result<ManagedAttachment, String> {
    let capture = current_capture(caller)?;
    let conversation_id = capture
        .conversation_id
        .clone()
        .ok_or_else(|| "请先选择一个会话".to_string())?;
    let (bytes, _, _) = decode_png(value).await?;
    let attachment = write_managed_png(app, &bytes, Some(conversation_id)).await?;
    if let Err(error) = ensure_current(caller, &capture) {
        remove_file(Path::new(&attachment.file_path));
        return Err(error);
    }
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不可用".to_string())?;
    if let Err(error) = main.emit("capture-ready", &attachment) {
        remove_file(Path::new(&attachment.file_path));
        return Err(format!("添加截图草稿失败: {error}"));
    }
    if caller == "capture-editor" {
        close_editor_session(app, &capture)?;
    }
    show_main(app)?;
    Ok(attachment)
}

async fn save_png(
    app: &tauri::AppHandle,
    parent: &str,
    bytes: Vec<u8>,
) -> Result<Option<SavedCapture>, String> {
    let app = app.clone();
    let parent = parent.to_string();
    let selected = tokio::task::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .add_filter("PNG", &["png"])
            .set_file_name(format!(
                "XChat-{}.png",
                chrono::Local::now().format("%Y%m%d-%H%M%S")
            ))
            .set_title("保存截图");
        if let Some(window) = app.get_webview_window(&parent) {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_save_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let mut path = selected
        .into_path()
        .map_err(|_| "保存路径不可用".to_string())?;
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        path.set_extension("png");
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "保存路径不可用".to_string())?;
    let pending = path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut temporary = TempCapturePath::new(pending.clone());
    tokio::fs::write(&pending, bytes)
        .await
        .map_err(|error| format!("保存截图失败: {error}"))?;
    tokio::fs::rename(&pending, &path)
        .await
        .map_err(|error| format!("保存截图失败: {error}"))?;
    temporary.disarm();
    Ok(Some(SavedCapture {
        file_path: path.to_string_lossy().into_owned(),
    }))
}

pub async fn save(
    app: &tauri::AppHandle,
    caller: &str,
    value: String,
) -> Result<Option<SavedCapture>, String> {
    if caller != "main" {
        current_capture(caller)?;
    }
    let (bytes, _, _) = decode_png(value).await?;
    save_png(app, caller, bytes).await
}

async fn copy_png(bytes: Vec<u8>, scale: Option<f64>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        use clipboard_rs::{
            common::RustImage, Clipboard, ClipboardContext, FilterType, RustImageData,
        };
        let mut image =
            RustImageData::from_bytes(&bytes).map_err(|error| format!("读取截图失败: {error}"))?;
        if let Some(scale) = scale {
            let scale = validate_pin_scale(scale)?;
            let (width, height) = image.get_size();
            let width = (width as f64 * scale).round().max(1.0) as u32;
            let height = (height as f64 * scale).round().max(1.0) as u32;
            if u64::from(width) * u64::from(height) > 134_217_728 {
                return Err("复制图片尺寸过大".to_string());
            }
            image = image
                .resize(width, height, FilterType::Lanczos3)
                .map_err(|error| format!("缩放截图失败: {error}"))?;
        }
        ClipboardContext::new()
            .map_err(|error| format!("剪贴板不可用: {error}"))?
            .set_image(image)
            .map_err(|error| format!("复制截图失败: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

pub async fn copy_editor(
    app: &tauri::AppHandle,
    caller: &str,
    value: String,
) -> Result<(), String> {
    if caller == "main" {
        let (bytes, _, _) = decode_png(value).await?;
        return copy_png(bytes, None).await;
    }
    if caller != "capture-editor" {
        return Err("当前窗口无权完成截图".to_string());
    }
    let capture = current_capture(caller)?;
    let (bytes, _, _) = decode_png(value).await?;
    copy_png(bytes, None).await?;
    ensure_current(caller, &capture)?;
    close_editor_session(app, &capture)
}

pub async fn copy_pin(
    caller: &str,
    pin_id: Option<&str>,
    scale: Option<f64>,
    value: Option<String>,
) -> Result<(), String> {
    let pin = pinned(caller, pin_id)?;
    let provided = value.is_some();
    let bytes = if let Some(value) = value {
        decode_png(value).await?.0
    } else {
        tokio::fs::read(&pin.capture.path)
            .await
            .map_err(|error| format!("读取贴图失败: {error}"))?
    };
    // A provided PNG already includes the frontend's rotation, flip and requested size.
    copy_png(bytes, if provided { None } else { scale }).await
}

pub async fn save_pin(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    value: Option<String>,
) -> Result<Option<SavedCapture>, String> {
    let pin = pinned(caller, pin_id)?;
    let bytes = if let Some(value) = value {
        decode_png(value).await?.0
    } else {
        tokio::fs::read(&pin.capture.path)
            .await
            .map_err(|error| format!("读取贴图失败: {error}"))?
    };
    save_png(app, caller, bytes).await
}

pub async fn read_clipboard() -> Result<CaptureClipboard, String> {
    tokio::task::spawn_blocking(|| {
        use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext};
        let clipboard =
            ClipboardContext::new().map_err(|error| format!("剪贴板不可用: {error}"))?;
        if let Ok(image) = clipboard.get_image() {
            let png = image
                .to_png()
                .map_err(|error| format!("读取剪贴板图片失败: {error}"))?;
            png_dimensions(png.get_bytes())?;
            return Ok(CaptureClipboard {
                data_url: Some(data_url(PNG_MIME, png.get_bytes())),
                text: None,
            });
        }
        let text = clipboard.get_text().ok().filter(|text| !text.is_empty());
        if text.is_none() {
            return Err("剪贴板中没有图片或文字".to_string());
        }
        Ok(CaptureClipboard {
            data_url: None,
            text,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

pub async fn write_clipboard_text(text: String) -> Result<(), String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("复制内容过大".to_string());
    }
    tokio::task::spawn_blocking(move || {
        use clipboard_rs::{Clipboard, ClipboardContext};
        ClipboardContext::new()
            .map_err(|error| format!("剪贴板不可用: {error}"))?
            .set_text(text)
            .map_err(|error| format!("复制文字失败: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn pin_dimensions(capture: &CaptureFile, view: &PinView, factor: f64) -> (f64, f64) {
    let (width, height) = if matches!(view.rotation, 90 | 270) {
        (capture.height as f64, capture.width as f64)
    } else {
        (capture.width as f64, capture.height as f64)
    };
    let scale = if view.thumbnail {
        view.scale
            .min(160.0 * factor / width)
            .min(120.0 * factor / height)
    } else {
        view.scale
    };
    (
        (width * scale).round().max(1.0),
        (height * scale).round().max(1.0),
    )
}

fn pin_is_visible(view: &PinView, group: &str) -> bool {
    !view.hidden && view.group == group
}
fn active_group() -> Result<String, String> {
    Ok(lock_state()?
        .active_group
        .clone()
        .unwrap_or_else(|| "默认".to_string()))
}

fn pin_scale_factor(window: &tauri::WebviewWindow, view: &PinView) -> f64 {
    window
        .monitor_from_point(view.x, view.y)
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .or_else(|| window.scale_factor().ok())
        .unwrap_or(1.0)
}

/// Restored layouts must remain reachable when the monitor arrangement changes.
fn keep_pin_visible(
    window: &tauri::WebviewWindow,
    capture: &CaptureFile,
    mut view: PinView,
) -> Result<PinView, String> {
    let monitors = window
        .available_monitors()
        .map_err(|error| format!("读取显示器失败: {error}"))?;
    let visible = monitors.iter().any(|monitor| {
        let (width, height) = pin_dimensions(capture, &view, monitor.scale_factor());
        let left = monitor.position().x as f64;
        let top = monitor.position().y as f64;
        let right = left + monitor.size().width as f64;
        let bottom = top + monitor.size().height as f64;
        view.x + width > left + width.min(24.0)
            && view.x < right - width.min(24.0)
            && view.y + height > top + height.min(24.0)
            && view.y < bottom - height.min(24.0)
    });
    if !visible {
        let monitor = window
            .cursor_position()
            .ok()
            .and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten())
            .or_else(|| window.primary_monitor().ok().flatten())
            .or_else(|| monitors.into_iter().next())
            .ok_or_else(|| "没有可用显示器".to_string())?;
        let (width, height) = pin_dimensions(capture, &view, monitor.scale_factor());
        view.x =
            monitor.position().x as f64 + ((monitor.size().width as f64 - width) / 2.0).max(0.0);
        view.y =
            monitor.position().y as f64 + ((monitor.size().height as f64 - height) / 2.0).max(0.0);
    }
    Ok(view)
}

fn apply_pin_geometry(
    window: &tauri::WebviewWindow,
    capture: &CaptureFile,
    view: &PinView,
    update_shadow: bool,
) -> Result<(), String> {
    let factor = pin_scale_factor(window, view);
    let (width, height) = pin_dimensions(capture, view, factor);
    // Windows shadows change the non-client frame. Restore them before setting
    // the client bounds, otherwise a dismissed menu leaves a different size.
    if update_shadow {
        window
            .set_shadow(view.shadow)
            .map_err(|error| format!("设置贴图阴影失败: {error}"))?;
    }
    let position = tauri::PhysicalPosition::new(view.x.round() as i32, view.y.round() as i32);
    if window.outer_position().map_err(|error| error.to_string())? != position {
        window
            .set_position(position)
            .map_err(|error| format!("移动贴图失败: {error}"))?;
    }
    let size = tauri::PhysicalSize::new(width as u32, height as u32);
    if window.inner_size().map_err(|error| error.to_string())? != size {
        window
            .set_size(size)
            .map_err(|error| format!("调整贴图大小失败: {error}"))?;
    }
    window
        .set_ignore_cursor_events(view.through)
        .map_err(|error| format!("设置鼠标穿透失败: {error}"))?;
    Ok(())
}

fn apply_pin_view(
    window: &tauri::WebviewWindow,
    capture: &CaptureFile,
    view: &PinView,
) -> Result<(), String> {
    apply_pin_view_changes(window, capture, view, true)
}

fn apply_pin_view_changes(
    window: &tauri::WebviewWindow,
    capture: &CaptureFile,
    view: &PinView,
    update_shadow: bool,
) -> Result<(), String> {
    apply_pin_geometry(window, capture, view, update_shadow)?;
    let visible = pin_is_visible(view, &active_group()?);
    if window.is_visible().map_err(|error| error.to_string())? != visible {
        if visible {
            window.show()
        } else {
            window.hide()
        }
        .map_err(|error| format!("设置贴图可见性失败: {error}"))?;
    }
    Ok(())
}

pub async fn set_pin_group(
    app: &tauri::AppHandle,
    caller: &str,
    group: String,
) -> Result<String, String> {
    if caller != "main" {
        return Err("当前窗口无权切换贴图组".to_string());
    }
    if group.trim().is_empty() || group.len() > 256 {
        return Err("贴图组名称无效".to_string());
    }
    let _operation = pin_operations().lock().await;
    let previous_group = active_group()?;
    let pins = lock_state()?.pins.values().cloned().collect::<Vec<_>>();
    let set_visibility = |group: &str| -> Result<(), String> {
        for pin in &pins {
            if let Some(window) =
                app.get_webview_window(&format!("{PIN_PREFIX}{}", pin.capture.session_id))
            {
                if pin_is_visible(&pin.view, group) {
                    window.show()
                } else {
                    window.hide()
                }
                .map_err(|error| format!("切换贴图组失败: {error}"))?;
            }
        }
        Ok(())
    };
    if let Err(error) = set_visibility(&group) {
        return match set_visibility(&previous_group) {
            Ok(()) => Err(error),
            Err(restore) => Err(format!("{error}；{restore}")),
        };
    }
    lock_state()?.active_group = Some(group.clone());
    app.emit(
        "capture-pin-group-updated",
        serde_json::json!({"group": group}),
    )
    .map_err(|error| error.to_string())?;
    Ok(group)
}

pub async fn toggle_pin_group(app: &tauri::AppHandle) -> Result<(), String> {
    let _operation = pin_operations().lock().await;
    let group = active_group()?;
    let pins = lock_state()?
        .pins
        .values()
        .filter(|pin| pin.view.group == group && !pin.overlay)
        .cloned()
        .collect::<Vec<_>>();
    if pins.is_empty() {
        return Ok(());
    }
    let hide = pins.iter().any(|pin| !pin.view.hidden && !pin.view.through);
    for pin in pins {
        let mut view = pin.view;
        view.hidden = hide;
        if !hide {
            view.through = false;
        }
        update_pin_view(app, &pin.capture.session_id, view, Some(false))?;
    }
    Ok(())
}

/// Opening the app also unlocks click-through pins if F3 is unavailable.
pub async fn restore_pin_interaction(app: &tauri::AppHandle) -> Result<usize, String> {
    restore_pins(app, false).await
}

/// F3 restores input and hidden pins in place without opening another page.
pub async fn recover_pins(app: &tauri::AppHandle) -> Result<usize, String> {
    let restored = restore_pins(app, true).await?;
    app.emit_to("main", "capture-pin-recover", ())
        .map_err(|error| error.to_string())?;
    Ok(restored)
}

async fn restore_pins(app: &tauri::AppHandle, restore_hidden: bool) -> Result<usize, String> {
    let _operation = pin_operations().lock().await;
    let group = active_group()?;
    let pins = lock_state()?
        .pins
        .values()
        .filter(|pin| {
            !pin.overlay
                && (pin.view.through
                    || restore_hidden && pin.view.hidden && pin.view.group == group)
        })
        .cloned()
        .collect::<Vec<_>>();
    let restored = pins.len();
    for pin in pins {
        let mut view = pin.view;
        view.through = false;
        if restore_hidden && view.group == group {
            view.hidden = false;
        }
        update_pin_view(app, &pin.capture.session_id, view, Some(false))?;
    }
    Ok(restored)
}

fn emit_pin_view(app: &tauri::AppHandle, id: &str, view: &PinView) -> Result<(), String> {
    let payload = PinViewEvent {
        pin_id: id.to_string(),
        view: view.clone(),
    };
    if let Some(window) = app.get_webview_window(&format!("{PIN_PREFIX}{id}")) {
        window
            .emit("capture-pin-view-updated", &payload)
            .map_err(|error| format!("同步贴图视图失败: {error}"))?;
    }
    if let Some(main) = app.get_webview_window("main") {
        main.emit("capture-pin-view-updated", &payload)
            .map_err(|error| format!("同步贴图管理失败: {error}"))?;
    }
    Ok(())
}

fn update_pin_view(
    app: &tauri::AppHandle,
    id: &str,
    view: PinView,
    overlay: Option<bool>,
) -> Result<PinView, String> {
    view.validate()?;
    let previous = lock_state()?
        .pins
        .get(id)
        .cloned()
        .ok_or_else(|| "贴图已不存在".to_string())?;
    let window = app
        .get_webview_window(&format!("{PIN_PREFIX}{id}"))
        .ok_or_else(|| "贴图窗口不可用".to_string())?;
    let view = keep_pin_visible(&window, &previous.capture, view)?;
    if overlay == Some(true) {
        window
            .set_shadow(false)
            .map_err(|error| format!("进入贴图编辑失败: {error}"))?;
        if let Err(error) = window.set_ignore_cursor_events(false) {
            let _ = window.set_shadow(previous.view.shadow);
            return Err(format!("进入贴图编辑失败: {error}"));
        }
        lock_state()?
            .pins
            .get_mut(id)
            .ok_or_else(|| "贴图已不存在".to_string())?
            .overlay = true;
        return Ok(previous.view);
    }
    if previous.overlay && overlay.is_none() {
        return Err("请先结束贴图的临时编辑或菜单操作".to_string());
    }
    lock_state()?
        .pins
        .get_mut(id)
        .ok_or_else(|| "贴图已不存在".to_string())?
        .applying = true;
    let result = apply_pin_view_changes(
        &window,
        &previous.capture,
        &view,
        previous.overlay || previous.view.shadow != view.shadow,
    );
    if let Err(error) = result {
        let rollback = apply_pin_view(&window, &previous.capture, &previous.view);
        if let Some(pin) = lock_state()?.pins.get_mut(id) {
            pin.applying = false;
        }
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}；恢复贴图视图失败: {rollback}")),
        };
    }
    {
        let mut state = lock_state()?;
        let pin = state
            .pins
            .get_mut(id)
            .ok_or_else(|| "贴图已不存在".to_string())?;
        pin.view = view.clone();
        pin.overlay = false;
        pin.applying = false;
    }
    emit_pin_view(app, id, &view)?;
    Ok(view)
}

pub async fn update_pin(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    view: PinView,
    overlay: Option<bool>,
) -> Result<PinView, String> {
    let id = scoped_pin_id(caller, pin_id)?;
    let _operation = pin_operations().lock().await;
    update_pin_view(app, &id, view, overlay)
}

pub async fn resize_pin(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    scale: f64,
) -> Result<f64, String> {
    let id = scoped_pin_id(caller, pin_id)?;
    let _operation = pin_operations().lock().await;
    let mut view = pinned(caller, pin_id)?.view;
    view.scale = validate_pin_scale(scale)?;
    Ok(update_pin_view(app, &id, view, None)?.scale)
}

pub async fn set_pin_shadow(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    enabled: bool,
) -> Result<(), String> {
    let id = scoped_pin_id(caller, pin_id)?;
    let _operation = pin_operations().lock().await;
    let mut view = pinned(caller, pin_id)?.view;
    view.shadow = enabled;
    update_pin_view(app, &id, view, None)?;
    Ok(())
}

pub async fn close_pin(
    app: &tauri::AppHandle,
    caller: &str,
    pin_id: Option<&str>,
    destroy: bool,
) -> Result<(), String> {
    let id = scoped_pin_id(caller, pin_id)?;
    let _operation = pin_operations().lock().await;
    let previous = pinned(caller, pin_id)?;
    if destroy {
        if let Some(window) = app.get_webview_window(&format!("{PIN_PREFIX}{id}")) {
            window
                .destroy()
                .map_err(|error| format!("销毁贴图失败: {error}"))?;
        }
        clear_pin(&id);
    } else {
        let mut view = previous.view;
        view.hidden = true;
        update_pin_view(app, &id, view, Some(false))?;
    }
    if destroy {
        discard_pin_overlays(app, &id)?;
    } else {
        let labels = lock_state()?
            .pin_overlays
            .iter()
            .filter(|(_, overlay)| overlay.pin_id == id)
            .map(|(label, _)| label.clone())
            .collect::<Vec<_>>();
        for label in labels {
            close_pin_overlay_window_locked(app, &label).await?;
        }
    }
    Ok(())
}

fn discard_pin_overlays(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let labels = {
        let mut state = lock_state()?;
        let labels = state
            .pin_overlays
            .iter()
            .filter(|(_, overlay)| overlay.pin_id == id)
            .map(|(label, _)| label.clone())
            .collect::<Vec<_>>();
        for label in &labels {
            state.pin_overlays.remove(label);
        }
        labels
    };
    for label in labels {
        if let Some(window) = app.get_webview_window(&label) {
            window.destroy().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn close_editor_session(app: &tauri::AppHandle, expected: &CaptureFile) -> Result<(), String> {
    let window = app.get_webview_window("capture-editor");
    let capture = {
        let mut state = lock_state()?;
        take_editor_if_matches(&mut state, expected)?
    };
    let result = (|| {
        if let Some(window) = &window {
            window
                .hide()
                .map_err(|error| format!("隐藏截图编辑器失败: {error}"))?;
            restore_capture_windows(app, &capture.session_id)?;
            window
                .destroy()
                .map_err(|error| format!("关闭截图编辑器失败: {error}"))?;
        }
        restore_capture_windows(app, &capture.session_id)
    })();
    if let Err(error) = result {
        let restored = {
            let mut state = lock_state()?;
            if state.editor.is_none()
                && (!state.starting
                    || state.starting_id.as_deref() == Some(capture.session_id.as_str()))
            {
                state.editor = Some(capture.clone());
                true
            } else {
                false
            }
        };
        if restored {
            if let Some(window) = window {
                let _ = window.show();
                let _ = window.set_focus();
            }
        } else {
            remove_file(&capture.path);
        }
        return Err(error);
    }
    remove_file(&capture.path);
    Ok(())
}

pub fn cancel(app: &tauri::AppHandle, caller: &str) -> Result<(), String> {
    if caller != "capture-editor" {
        return Err("当前窗口无权关闭截图编辑器".to_string());
    }
    let capture = current_capture(caller)?;
    close_editor_session(app, &capture)
}

#[derive(Debug, PartialEq, Eq)]
enum PinOrigin {
    Editor(String),
    Existing(String),
    Imported,
}

/// Select the source using the invoking window, never by whichever editor exists.
fn pin_source(
    state: &CaptureState,
    caller: &str,
    requested: Option<&str>,
) -> Result<(String, Option<CaptureFile>, Option<PinView>, PinOrigin), String> {
    if caller == "capture-editor" {
        let source = state
            .editor
            .clone()
            .ok_or_else(|| "没有待处理的截图".to_string())?;
        let id = requested
            .map(validated_pin_id)
            .transpose()?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if state.pins.contains_key(&id) {
            return Err("截图编辑器不能替换其他贴图".to_string());
        }
        return Ok((
            id,
            Some(source.clone()),
            None,
            PinOrigin::Editor(source.session_id),
        ));
    }
    if caller == "main" {
        let id = requested
            .map(validated_pin_id)
            .transpose()?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        return match state.pins.get(&id) {
            Some(pin) => Ok((
                id.clone(),
                Some(pin.capture.clone()),
                Some(pin.view.clone()),
                PinOrigin::Existing(id),
            )),
            None => Ok((id, None, None, PinOrigin::Imported)),
        };
    }
    let id = scoped_pin_id(caller, requested)?;
    let pin = state
        .pins
        .get(&id)
        .ok_or_else(|| "贴图已不存在".to_string())?;
    Ok((
        id.clone(),
        Some(pin.capture.clone()),
        Some(pin.view.clone()),
        PinOrigin::Existing(id),
    ))
}

fn commit_pin(
    state: &mut CaptureState,
    source: Option<&CaptureFile>,
    origin: &PinOrigin,
    pin: PinnedCapture,
) -> Result<Option<PinnedCapture>, String> {
    match origin {
        PinOrigin::Editor(id) => {
            if state.editor.as_ref().map(|capture| &capture.session_id) != Some(id) {
                return Err("截图编辑会话已变化".to_string());
            }
        }
        PinOrigin::Existing(id) => {
            if state.pins.get(id).map(|pin| &pin.capture.path)
                != source.map(|capture| &capture.path)
            {
                return Err("贴图编辑会话已变化".to_string());
            }
        }
        PinOrigin::Imported => {
            if state.pins.contains_key(&pin.capture.session_id) {
                return Err("贴图 ID 已存在".to_string());
            }
        }
    }
    Ok(state.pins.insert(pin.capture.session_id.clone(), pin))
}

pub async fn pin(
    app: &tauri::AppHandle,
    caller: &str,
    value: String,
    pin_id: Option<&str>,
    initial_view: Option<PinView>,
    conversation_id: Option<String>,
) -> Result<CaptureSessionSummary, String> {
    let _operation = pin_operations().lock().await;
    let (id, source, previous_view, origin) = {
        let state = lock_state()?;
        pin_source(&state, caller, pin_id)?
    };
    let conversation_id = validate_capture_conversation_id(conversation_id)?;
    if let Some(view) = &initial_view {
        view.validate()?;
    }
    let has_initial_view = initial_view.is_some();
    let (bytes, width, height) = decode_png(value).await?;
    let capture_dir = std::env::temp_dir().join("xchat-captures");
    tokio::fs::create_dir_all(&capture_dir)
        .await
        .map_err(|error| format!("创建截图缓存失败: {error}"))?;
    // Each revision has a distinct path so in-flight reads never observe partially replaced PNGs.
    let path = capture_dir.join(format!("pin-{id}-{}.png", uuid::Uuid::new_v4()));
    let mut temporary = TempCapturePath::new(path.clone());
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|error| format!("保存贴图失败: {error}"))?;
    let capture = CaptureFile {
        session_id: id.clone(),
        conversation_id: source
            .as_ref()
            .and_then(|source| source.conversation_id.clone())
            .or_else(|| {
                if caller == "main" {
                    conversation_id
                } else {
                    None
                }
            }),
        path,
        file_name: source
            .as_ref()
            .map(|source| source.file_name.clone())
            .unwrap_or_else(|| "capture.png".to_string()),
        file_size: bytes.len() as u64,
        width,
        height,
        regions: Vec::new(),
    };
    let mut view = previous_view.or(initial_view).unwrap_or_else(|| PinView {
        scale: (960.0 / width as f64)
            .min(720.0 / height as f64)
            .min(1.0)
            .max(0.1),
        group: active_group().unwrap_or_else(|_| "默认".to_string()),
        ..PinView::default()
    });
    let label = format!("{PIN_PREFIX}{id}");
    let existing = app.get_webview_window(&label);
    if matches!(origin, PinOrigin::Existing(_)) && existing.is_none() {
        return Err("贴图窗口不可用".to_string());
    }
    let overlay = lock_state()?.pins.get(&id).is_some_and(|pin| pin.overlay);
    let previous = {
        let mut state = lock_state()?;
        commit_pin(
            &mut state,
            source.as_ref(),
            &origin,
            PinnedCapture {
                capture: capture.clone(),
                view: view.clone(),
                overlay,
                applying: true,
            },
        )?
    };
    let created = existing.is_none();
    let window_result = if let Some(window) = existing {
        Ok(window)
    } else {
        let (window_width, window_height) = pin_dimensions(&capture, &view, 1.0);
        WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App(format!("index.html?view=capture-pin&pinId={id}").into()),
        )
        .title("XChat 贴图")
        .inner_size(window_width, window_height)
        .position(view.x, view.y)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(view.shadow)
        .visible(false)
        .build()
        .map_err(|error| format!("打开贴图窗口失败: {error}"))
    };
    let window = match window_result {
        Ok(window) => window,
        Err(error) => {
            let mut state = lock_state()?;
            if let Some(previous) = previous {
                state.pins.insert(id, previous);
            } else {
                state.pins.remove(&id);
            }
            return Err(error);
        }
    };
    let selection_origin = if created && has_initial_view && matches!(origin, PinOrigin::Editor(_))
    {
        Some((view.x.round(), view.y.round()))
    } else {
        None
    };
    if selection_origin.is_some() {
        // A screenshot selection is positioned by its visible client pixels.
        // Saved pin positions still use the native outer-window coordinates.
        if let (Ok(inner), Ok(outer)) = (window.inner_position(), window.outer_position()) {
            view.x -= (inner.x - outer.x) as f64;
            view.y -= (inner.y - outer.y) as f64;
        }
    }
    if created && !has_initial_view {
        let monitor = window
            .cursor_position()
            .ok()
            .and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten())
            .or_else(|| window.current_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            view.scale = ((monitor.size().width as f64 - 80.0) / width as f64)
                .min((monitor.size().height as f64 - 80.0) / height as f64)
                .min(1.0)
                .max(0.1);
            let (window_width, window_height) =
                pin_dimensions(&capture, &view, monitor.scale_factor());
            view.x =
                monitor.position().x as f64 + (monitor.size().width as f64 - window_width) / 2.0;
            view.y =
                monitor.position().y as f64 + (monitor.size().height as f64 - window_height) / 2.0;
        }
    }
    let applied = match keep_pin_visible(&window, &capture, view.clone()) {
        Ok(visible_view) => {
            view = visible_view;
            if overlay {
                Ok(())
            } else if let Some((x, y)) = selection_origin {
                (|| {
                    // Re-read the frame after reaching the destination monitor:
                    // mixed DPI screens can use different non-client offsets.
                    // Align while hidden so the initial show never jumps.
                    apply_pin_geometry(&window, &capture, &view, true)?;
                    let inner = window.inner_position().map_err(|error| error.to_string())?;
                    view.x += x - inner.x as f64;
                    view.y += y - inner.y as f64;
                    apply_pin_view_changes(&window, &capture, &view, false)
                })()
            } else {
                apply_pin_view(&window, &capture, &view)
            }
        }
        Err(error) => Err(error),
    };
    if let Err(error) = applied {
        if created {
            let _ = window.destroy();
        }
        if let Some(previous) = previous {
            let rollback = apply_pin_view(&window, &previous.capture, &previous.view);
            lock_state()?.pins.insert(id, previous);
            if let Err(rollback) = rollback {
                return Err(format!("{error}；{rollback}"));
            }
        } else {
            lock_state()?.pins.remove(&id);
        }
        return Err(error);
    }
    if let Some(current) = lock_state()?.pins.get_mut(&id) {
        current.view = view.clone();
        current.applying = false;
    }
    temporary.disarm();
    if let Some(previous) = previous {
        remove_file(&previous.capture.path);
    }
    if created {
        let event_app = app.clone();
        let event_id = id.clone();
        let event_label = label.clone();
        window.on_window_event(move |event| match event {
            tauri::WindowEvent::Destroyed => {
                let _ = discard_pin_overlays(&event_app, &event_id);
                clear_pin(&event_id);
                if let Some(main) = event_app.get_webview_window("main") {
                    let _ = main.emit(
                        "capture-pin-closed",
                        serde_json::json!({"pin_id": event_id, "destroy": true}),
                    );
                }
            }
            tauri::WindowEvent::Moved(position) => {
                let changed = lock_state().ok().and_then(|mut state| {
                    let pin = state.pins.get_mut(&event_id)?;
                    if pin.overlay || pin.applying {
                        return None;
                    }
                    pin.view.x = position.x as f64;
                    pin.view.y = position.y as f64;
                    Some(pin.view.clone())
                });
                if let Some(view) = changed {
                    let _ = emit_pin_view(&event_app, &event_id, &view);
                }
            }
            tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let pin = lock_state()
                    .ok()
                    .and_then(|state| state.pins.get(&event_id).cloned());
                if let Some(pin) = pin.filter(|pin| !pin.overlay) {
                    if let Some(window) = event_app.get_webview_window(&event_label) {
                        let (width, height) =
                            pin_dimensions(&pin.capture, &pin.view, *scale_factor);
                        if let Err(error) =
                            window.set_size(tauri::PhysicalSize::new(width as u32, height as u32))
                        {
                            eprintln!("[CapturePin] 更新显示密度失败: {error}");
                        }
                        let _ = emit_pin_view(&event_app, &event_id, &pin.view);
                    }
                }
            }
            _ => {}
        });
    }
    let summary = capture_summary(&capture, Some(view.clone()));
    window
        .emit("capture-pin-updated", &summary)
        .map_err(|error| format!("刷新贴图失败: {error}"))?;
    if let Some(main) = app.get_webview_window("main") {
        main.emit("capture-pin-updated", &summary)
            .map_err(|error| format!("刷新贴图管理失败: {error}"))?;
    }
    emit_pin_view(app, &id, &view)?;
    if matches!(origin, PinOrigin::Editor(_)) {
        if let Some(source) = &source {
            close_editor_session(app, source)?;
        }
    }
    if !overlay && pin_is_visible(&view, &active_group()?) && !view.through {
        window
            .set_focus()
            .map_err(|error| format!("聚焦贴图失败: {error}"))?;
    }
    if created {
        preload_pin_menu(app, &id);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIRST: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND: &str = "22222222-2222-4222-8222-222222222222";
    fn file(id: &str) -> CaptureFile {
        CaptureFile {
            session_id: id.to_string(),
            conversation_id: Some("conversation".to_string()),
            path: PathBuf::from(format!("{id}.png")),
            file_name: "capture.png".to_string(),
            file_size: 24,
            width: 640,
            height: 320,
            regions: Vec::new(),
        }
    }
    fn pin(id: &str) -> PinnedCapture {
        PinnedCapture {
            capture: file(id),
            view: PinView::default(),
            overlay: false,
            applying: false,
        }
    }
    #[test]
    fn capture_preferences_validate_atomically_and_report_cursor_capability() {
        let mut state = CaptureState::default();
        let defaults = apply_preferences(&mut state, None, None, false).unwrap();
        assert!(!defaults.capture_cursor);
        assert_eq!(
            serde_json::to_value(defaults).unwrap(),
            serde_json::json!({
                "delaySeconds": 0, "captureCursor": false, "cursorSupported": false,
            })
        );
        assert!(apply_preferences(&mut state, Some(3), Some(true), false).is_err());
        assert_eq!(state.delay_seconds, 0);
        assert!(!state.capture_cursor);
        assert!(apply_preferences(&mut state, Some(1), Some(true), true).is_err());
        assert!(!state.capture_cursor);
        let started = apply_preferences(&mut state, Some(5), Some(true), true).unwrap();
        apply_preferences(&mut state, Some(0), Some(false), true).unwrap();
        assert!(started.capture_cursor);
        assert_eq!(started.delay_seconds, 5);
        assert!(!state.capture_cursor);
    }
    #[cfg(all(any(target_os = "windows", target_os = "linux"), feature = "xcap"))]
    #[test]
    fn capture_png_preserves_every_pixel_and_alpha_value() {
        let image = xcap::image::RgbaImage::from_fn(64, 48, |x, y| {
            xcap::image::Rgba([
                x as u8,
                y as u8,
                (x * 17 + y * 31) as u8,
                if (x + y) % 3 == 0 { 0 } else { 255 },
            ])
        });
        let bytes = encode_capture_png(&image).unwrap();
        let decoded = xcap::image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded, image);
        assert_eq!(png_dimensions(&bytes).unwrap(), image.dimensions());
    }
    #[cfg(all(target_os = "windows", feature = "xcap"))]
    #[test]
    fn cursor_composite_region_clips_all_edges_without_coordinate_overflow() {
        let image = (1920, 1080);
        let cursor = (40, 40);
        assert_eq!(
            cursor_composite_region(image, cursor, (800, 423)),
            Some((800, 423, 40, 40))
        );
        assert_eq!(
            cursor_composite_region(image, cursor, (-10, -8)),
            Some((0, 0, 30, 32))
        );
        assert_eq!(
            cursor_composite_region(image, cursor, (1910, 1070)),
            Some((1910, 1070, 10, 10))
        );
        for position in [
            (1920, 0), (0, 1080), (-40, 0), (0, -40), (i32::MAX, 0), (i32::MIN, 0),
        ] {
            assert_eq!(cursor_composite_region(image, cursor, position), None);
        }
    }
    #[cfg(all(target_os = "windows", feature = "xcap"))]
    #[test]
    fn cursor_hotspots_use_physical_pixels_on_negative_and_adjacent_displays() {
        let origin = (-2400, -1350);
        let size = (2400, 1350);
        assert_eq!(
            cursor_draw_position(origin, size, (-1200, -675), (6, 8)),
            Some((1194, 667))
        );
        assert_eq!(
            cursor_draw_position(origin, size, origin, (6, 8)),
            Some((-6, -8))
        );
        for outside in [(-2401, -1), (-1, -1351), (0, -1), (-1, 0)] {
            assert_eq!(cursor_draw_position(origin, size, outside, (6, 8)), None);
        }
        assert_eq!(
            cursor_draw_position((1920, 0), (2560, 1440), (3200, 720), (10, 10)),
            Some((1270, 710))
        );
    }
    #[cfg(all(target_os = "windows", feature = "xcap"))]
    #[test]
    fn system_cursor_composes_into_pixels_without_changing_the_background() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{LoadCursorW, IDC_ARROW};
        // LoadCursorW returns a shared system resource; this test neither moves
        // the live cursor nor opens a window or captures the desktop.
        let cursor = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) };
        assert!(!cursor.is_null());
        let background = xcap::image::Rgba([71, 123, 219, 255]);
        let mut image = xcap::image::RgbaImage::from_pixel(256, 256, background);
        draw_cursor_icon(&mut image, cursor, (10, 12)).unwrap();
        assert!(image.pixels().any(|pixel| *pixel != background));
        for (x, y, pixel) in image.enumerate_pixels() {
            assert_eq!(pixel[3], 255);
            if x < 10 || y < 12 {
                assert_eq!(*pixel, background);
            }
        }
        assert_eq!(*image.get_pixel(255, 255), background);
        // Drawing a cursor partially outside the image must match cropping the
        // same complete drawing, including the system's transparent/mask pixels.
        let mut reference = xcap::image::RgbaImage::from_pixel(128, 128, background);
        draw_cursor_icon(&mut reference, cursor, (20, 20)).unwrap();
        let mut clipped = xcap::image::RgbaImage::from_pixel(64, 64, background);
        draw_cursor_icon(&mut clipped, cursor, (-4, -4)).unwrap();
        assert_eq!(
            clipped,
            xcap::image::imageops::crop_imm(&reference, 24, 24, 64, 64).to_image()
        );
    }
    #[cfg(all(target_os = "windows", feature = "xcap"))]
    #[test]
    #[ignore = "requires an unlocked interactive Windows desktop"]
    fn windows_captures_the_current_monitor() {
        use windows_sys::Win32::{
            Foundation::POINT,
            Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
            },
        };
        let mut monitor: MONITORINFO = unsafe { std::mem::zeroed() };
        monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let handle = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        assert_ne!(unsafe { GetMonitorInfoW(handle, &mut monitor) }, 0);
        let rect = monitor.rcMonitor;
        let width = (rect.right - rect.left) as u32;
        let height = (rect.bottom - rect.top) as u32;
        let path =
            std::env::temp_dir().join(format!("xchat-screen-smoke-{}.png", uuid::Uuid::new_v4()));
        let _temporary = TempCapturePath::new(path.clone());
        grab_monitor_png((rect.left, rect.top), (width, height), &path).unwrap();
        let image = xcap::image::open(&path).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (width, height));
        assert!(
            image.pixels().any(|pixel| pixel[3] > 0),
            "screen is fully transparent"
        );
        let first = image.get_pixel(0, 0);
        assert!(
            image.pixels().any(|pixel| pixel.0[..3] != first.0[..3]),
            "screen is a single color; inspect the desktop before accepting capture"
        );
        assert_eq!(
            png_dimensions(&std::fs::read(&path).unwrap()).unwrap(),
            (width, height)
        );
    }

    #[cfg(all(target_os = "windows", feature = "xcap"))]
    #[test]
    #[ignore = "requires a visible stationary cursor on an interactive Windows desktop"]
    fn windows_composes_the_live_cursor_on_a_captured_frame() {
        use windows_sys::Win32::{
            Foundation::POINT,
            UI::WindowsAndMessaging::{
                GetCursorInfo, GetPhysicalCursorPos, CURSORINFO, CURSOR_SHOWING, CURSOR_SUPPRESSED,
            },
        };
        let mut cursor = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..CURSORINFO::default()
        };
        let mut point = POINT::default();
        assert_ne!(unsafe { GetCursorInfo(&mut cursor) }, 0);
        assert_eq!(cursor.flags & (CURSOR_SHOWING | CURSOR_SUPPRESSED), CURSOR_SHOWING);
        assert_ne!(unsafe { GetPhysicalCursorPos(&mut point) }, 0);
        let monitor = xcap::Monitor::from_point(point.x, point.y).unwrap();
        let origin = (monitor.x().unwrap(), monitor.y().unwrap());
        let size = (monitor.width().unwrap(), monitor.height().unwrap());
        let path = std::env::temp_dir()
            .join(format!("xchat-cursor-smoke-{}.png", uuid::Uuid::new_v4()));
        let _temporary = TempCapturePath::new(path.clone());
        grab_monitor_png(origin, size, &path).unwrap();
        let mut image = xcap::image::open(&path).unwrap().to_rgba8();
        let before = image.clone();
        include_windows_cursor(&mut image, origin, size).unwrap();
        assert_eq!(image.dimensions(), size);
        assert!(
            image.pixels().zip(before.pixels()).any(|(after, before)| after != before),
            "the live pointer made no pixel change; keep it visible and stationary for this probe"
        );
    }

    #[test]
    fn pins_are_scoped_to_the_invoking_window() {
        assert_eq!(
            scoped_pin_id(&format!("{PIN_PREFIX}{FIRST}"), None).unwrap(),
            FIRST
        );
        assert_eq!(scoped_pin_id("main", Some(SECOND)).unwrap(), SECOND);
        assert!(scoped_pin_id(&format!("{PIN_PREFIX}{FIRST}"), Some(SECOND)).is_err());
        assert!(scoped_pin_id("capture-editor", Some(FIRST)).is_err());
        assert!(scoped_pin_id("main", None).is_err());
        assert!(scoped_pin_id("capture-pin-../../main", None).is_err());
        for prefix in [PIN_MENU_PREFIX, PIN_EDIT_PREFIX] {
            let caller = format!("{prefix}{FIRST}-{SECOND}");
            assert_eq!(scoped_pin_id(&caller, None).unwrap(), FIRST);
            assert!(scoped_pin_id(&caller, Some(SECOND)).is_err());
            assert!(scoped_pin_id(&format!("{prefix}../../main"), None).is_err());
        }
    }
    #[test]
    fn editing_a_pin_preserves_its_id_view_and_unrelated_editor() {
        let mut state = CaptureState::default();
        state.editor = Some(file("editor"));
        let mut first = pin(FIRST);
        first.view.rotation = 90;
        first.view.shadow = false;
        state.pins.insert(FIRST.to_string(), first.clone());
        state.pins.insert(SECOND.to_string(), pin(SECOND));
        let (id, source, view, origin) =
            pin_source(&state, &format!("{PIN_PREFIX}{FIRST}"), None).unwrap();
        assert_eq!(id, FIRST);
        assert_eq!(view.as_ref(), Some(&first.view));
        let mut replacement = first;
        replacement.capture.path = PathBuf::from("revision.png");
        commit_pin(&mut state, source.as_ref(), &origin, replacement).unwrap();
        assert_eq!(state.editor.as_ref().unwrap().session_id, "editor");
        assert_eq!(state.pins.len(), 2);
        assert_eq!(
            state.pins[SECOND].capture.path,
            PathBuf::from(format!("{SECOND}.png"))
        );
        assert!(!state.pins[FIRST].view.shadow);
    }
    #[test]
    fn imports_never_consume_an_unrelated_editor_and_stale_edits_fail() {
        let mut state = CaptureState::default();
        state.editor = Some(file("editor"));
        let (id, source, _, origin) = pin_source(&state, "main", Some(FIRST)).unwrap();
        assert!(source.is_none());
        assert_eq!(origin, PinOrigin::Imported);
        commit_pin(&mut state, None, &origin, pin(&id)).unwrap();
        assert!(state.editor.is_some());
        let mut stale = file(FIRST);
        stale.path = PathBuf::from("stale.png");
        assert!(commit_pin(
            &mut state,
            Some(&stale),
            &PinOrigin::Existing(FIRST.to_string()),
            pin(FIRST)
        )
        .is_err());
        assert!(pin_source(&state, "capture-editor", Some(FIRST)).is_err());
    }
    #[test]
    fn pin_view_defaults_and_validation_match_the_frontend_contract() {
        let view: PinView = serde_json::from_str("{}").unwrap();
        assert!(view.shadow);
        assert_eq!(view.group, "默认");
        assert_eq!(serde_json::to_value(&view).unwrap()["flipX"], 1);
        for scale in [0.1, 1.0, 8.0] {
            assert!(PinView {
                scale,
                ..view.clone()
            }
            .validate()
            .is_ok());
        }
        for scale in [0.09, 8.01, f64::NAN, f64::INFINITY] {
            assert!(PinView {
                scale,
                ..view.clone()
            }
            .validate()
            .is_err());
        }
        assert!(PinView {
            rotation: 45,
            ..view.clone()
        }
        .validate()
        .is_err());
        assert!(PinView {
            opacity: 0.14,
            ..view.clone()
        }
        .validate()
        .is_err());
        assert!(PinView {
            flip_x: 0,
            ..view.clone()
        }
        .validate()
        .is_err());
        assert!(PinView {
            x: f64::NAN,
            ..view
        }
        .validate()
        .is_err());
    }
    #[test]
    fn pin_dimensions_include_rotation_zoom_and_thumbnail_bounds() {
        let capture = file(FIRST);
        let view = PinView {
            scale: 2.0,
            rotation: 90,
            ..PinView::default()
        };
        assert_eq!(pin_dimensions(&capture, &view, 1.0), (640.0, 1280.0));
        assert_eq!(
            pin_dimensions(
                &capture,
                &PinView {
                    thumbnail: true,
                    ..view.clone()
                },
                1.0
            ),
            (60.0, 120.0)
        );
        assert_eq!(
            pin_dimensions(&capture, &PinView::default(), 2.0),
            (640.0, 320.0)
        );
        assert_eq!(
            pin_dimensions(
                &capture,
                &PinView {
                    thumbnail: true,
                    ..view
                },
                2.0
            ),
            (120.0, 240.0)
        );
    }
    #[test]
    fn stale_editor_completion_cannot_take_a_new_session_or_revision() {
        let mut state = CaptureState::default();
        let old = file("old");
        state.editor = Some(file("new"));
        assert!(take_editor_if_matches(&mut state, &old).is_err());
        assert_eq!(state.editor.as_ref().unwrap().session_id, "new");
        let mut revision = file("new");
        revision.path = PathBuf::from("new-revision.png");
        assert!(take_editor_if_matches(&mut state, &revision).is_err());
        let current = state.editor.clone().unwrap();
        assert_eq!(
            take_editor_if_matches(&mut state, &current)
                .unwrap()
                .session_id,
            "new"
        );
        assert!(state.editor.is_none());
    }
    #[test]
    fn countdown_cancellation_is_scoped_and_never_consumes_an_editor() {
        let mut state = CaptureState {
            starting: true,
            starting_id: Some("countdown".to_string()),
            countdown_remaining: 3,
            ..CaptureState::default()
        };
        assert!(mark_start_cancelled(&mut state, Some("older-countdown")).is_none());
        assert_eq!(
            mark_start_cancelled(&mut state, Some("countdown")).as_deref(),
            Some("countdown")
        );
        assert!(state.start_cancelled);
        assert_eq!(state.countdown_remaining, 0);
        state.start_cancelled = false;
        state.countdown_remaining = 2;
        state.editor = Some(file("existing-editor"));
        assert!(mark_start_cancelled(&mut state, None).is_none());
        assert_eq!(state.editor.as_ref().unwrap().session_id, "existing-editor");
    }
    #[test]
    fn groups_preserve_each_pins_hidden_setting() {
        let visible = PinView::default();
        assert!(pin_is_visible(&visible, "默认"));
        assert!(!pin_is_visible(&visible, "设计参考"));
        let hidden = PinView {
            hidden: true,
            ..visible
        };
        assert!(!pin_is_visible(&hidden, "默认"));
        assert!(hidden.hidden);
    }
    #[test]
    fn standalone_capture_and_conversation_validation_remain_distinct() {
        assert_eq!(validate_capture_conversation_id(None).unwrap(), None);
        assert!(validate_capture_conversation_id(Some(" ".to_string())).is_err());
        assert!(validate_capture_conversation_id(Some("x".repeat(257))).is_err());
    }
}
