use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

const PNG_MIME: &str = "image/png";
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
struct CaptureFile {
    session_id: String,
    conversation_id: Option<String>,
    path: PathBuf,
    file_name: String,
    file_size: u64,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct CaptureState {
    editor: Option<CaptureFile>,
    pin: Option<CaptureFile>,
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
pub struct CaptureSessionSummary {
    pub session_id: String,
    pub conversation_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedAttachment {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub mime_type: String,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedCapture {
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceMedia {
    pub data_url: String,
    pub mime_type: String,
    pub file_name: String,
    pub file_size: u64,
}

fn state() -> &'static Mutex<CaptureState> {
    static STATE: OnceLock<Mutex<CaptureState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CaptureState::default()))
}

fn lock_state() -> Result<std::sync::MutexGuard<'static, CaptureState>, String> {
    state().lock().map_err(|_| "截图状态不可用".to_string())
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err("仅支持 PNG 图片".to_string());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("图片不能超过 64 MiB".to_string());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || width > 32_768 || height > 32_768 {
        return Err("PNG 图片尺寸无效".to_string());
    }
    Ok((width, height))
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn decode_base64(value: &str) -> Result<Vec<u8>, String> {
    let bytes = value.as_bytes();
    if bytes.len() % 4 != 0 || bytes.len() > (MAX_IMAGE_BYTES * 4 / 3) + 8 {
        return Err("图片数据无效".to_string());
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunks = bytes.chunks_exact(4);
    let chunk_count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        let a = base64_value(chunk[0]).ok_or_else(|| "图片数据无效".to_string())?;
        let b = base64_value(chunk[1]).ok_or_else(|| "图片数据无效".to_string())?;
        let last = index + 1 == chunk_count;
        let c = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' {
                return Err("图片数据无效".to_string());
            }
            None
        } else {
            Some(base64_value(chunk[2]).ok_or_else(|| "图片数据无效".to_string())?)
        };
        let d = if chunk[3] == b'=' {
            if !last {
                return Err("图片数据无效".to_string());
            }
            None
        } else {
            Some(base64_value(chunk[3]).ok_or_else(|| "图片数据无效".to_string())?)
        };
        if c.is_none() && d.is_some() {
            return Err("图片数据无效".to_string());
        }
        output.push((a << 2) | (b >> 4));
        if let Some(c) = c {
            output.push((b << 4) | (c >> 2));
            if let Some(d) = d {
                output.push((c << 6) | d);
            }
        }
    }
    Ok(output)
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn png_from_data_url(data_url: &str) -> Result<(Vec<u8>, u32, u32), String> {
    let (header, payload) = data_url
        .split_once(',')
        .ok_or_else(|| "图片数据无效".to_string())?;
    if header != "data:image/png;base64" {
        return Err("仅支持 PNG 图片".to_string());
    }
    let bytes = decode_base64(payload)?;
    let (width, height) = png_dimensions(&bytes)?;
    Ok((bytes, width, height))
}

fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", encode_base64(bytes))
}

fn capture_summary(capture: &CaptureFile) -> CaptureSessionSummary {
    CaptureSessionSummary {
        session_id: capture.session_id.clone(),
        conversation_id: capture.conversation_id.clone(),
    }
}

fn window_size(width: u32, height: u32, max_width: f64, max_height: f64) -> (f64, f64) {
    let scale = (max_width / width as f64)
        .min(max_height / height as f64)
        .min(1.0);
    (width as f64 * scale, height as f64 * scale)
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

fn restore_main_window(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("[Capture] 清理临时图片失败: {error}");
        }
    }
}

fn clear_editor() {
    if let Ok(mut state) = lock_state() {
        if let Some(capture) = state.editor.take() {
            remove_file(&capture.path);
        }
    }
}

fn clear_pin() {
    if let Ok(mut state) = lock_state() {
        if let Some(capture) = state.pin.take() {
            remove_file(&capture.path);
        }
    }
}

async fn read_capture(capture: CaptureFile) -> Result<PendingCapture, String> {
    let bytes = tokio::fs::read(&capture.path)
        .await
        .map_err(|error| format!("截图已不可用: {error}"))?;
    png_dimensions(&bytes)?;
    Ok(PendingCapture {
        session_id: capture.session_id,
        conversation_id: capture.conversation_id,
        data_url: data_url(PNG_MIME, &bytes),
        mime_type: PNG_MIME.to_string(),
        file_name: capture.file_name,
        file_size: capture.file_size,
        width: capture.width,
        height: capture.height,
    })
}

pub async fn start(
    app: &tauri::AppHandle,
    conversation_id: String,
) -> Result<CaptureSessionSummary, String> {
    if conversation_id.trim().is_empty() || conversation_id.len() > 256 {
        return Err("无效的会话 ID".to_string());
    }
    if let Some(window) = app.get_webview_window("capture-editor") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let capture = lock_state().and_then(|state| {
            state
                .editor
                .as_ref()
                .map(capture_summary)
                .ok_or_else(|| "截图编辑器状态不可用".to_string())
        });
        if capture.is_err() {
            let _ = window.close();
            restore_main_window(app);
        }
        return capture;
    }

    #[cfg(target_os = "macos")]
    {
        let main = app
            .get_webview_window("main")
            .ok_or_else(|| "主窗口不可用".to_string())?;
        let monitor = main
            .current_monitor()
            .map_err(|error| format!("无法读取当前显示器: {error}"))?
            .ok_or_else(|| "当前显示器不可用".to_string())?;
        let monitors = main
            .available_monitors()
            .map_err(|error| format!("无法读取显示器列表: {error}"))?;
        let current_geometry = (
            monitor.position().x,
            monitor.position().y,
            monitor.size().width,
            monitor.size().height,
        );
        let monitor_geometries = monitors
            .iter()
            .map(|item| {
                (
                    item.position().x,
                    item.position().y,
                    item.size().width,
                    item.size().height,
                )
            })
            .collect::<Vec<_>>();
        let display_number =
            display_number_for_geometry(current_geometry, &monitor_geometries).to_string();
        main.hide()
            .map_err(|error| format!("隐藏主窗口失败: {error}"))?;
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;

        let result = async {
            let capture_dir = std::env::temp_dir().join("xchat-captures");
            tokio::fs::create_dir_all(&capture_dir)
                .await
                .map_err(|error| format!("创建截图缓存目录失败: {error}"))?;
            let session_id = uuid::Uuid::new_v4().to_string();
            let path = capture_dir.join(format!("{session_id}.png"));
            let mut temp_path = TempCapturePath::new(path.clone());
            let command_path = path.clone();
            let status = tokio::task::spawn_blocking(move || {
                std::process::Command::new("/usr/sbin/screencapture")
                    .args(["-x", "-D"])
                    .arg(display_number)
                    .arg(&command_path)
                    .status()
            })
            .await
            .map_err(|error| format!("启动系统截图失败: {error}"))?
            .map_err(|error| format!("启动系统截图失败: {error}"))?;
            let bytes = tokio::fs::read(&path).await.unwrap_or_default();
            if !status.success() || bytes.is_empty() {
                remove_file(&path);
                return Err(
                    "无法读取屏幕，请在系统设置中允许 Xchat 使用屏幕录制权限".to_string(),
                );
            }
            let (width, height) = png_dimensions(&bytes)?;
            let capture = CaptureFile {
                session_id,
                conversation_id: Some(conversation_id),
                path,
                file_name: "capture.png".to_string(),
                file_size: bytes.len() as u64,
                width,
                height,
            };
            clear_editor();
            lock_state()?.editor = Some(capture.clone());
            temp_path.disarm();
            let scale_factor = monitor.scale_factor();
            let window_width = monitor.size().width as f64 / scale_factor;
            let window_height = monitor.size().height as f64 / scale_factor;
            let position_x = monitor.position().x as f64 / scale_factor;
            let position_y = monitor.position().y as f64 / scale_factor;
            let window = match WebviewWindowBuilder::new(
                app,
                "capture-editor",
                WebviewUrl::App("index.html?view=capture-editor".into()),
            )
            .title("Xchat 截图")
            .inner_size(window_width, window_height)
            .position(position_x, position_y)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .build()
            {
                Ok(window) => window,
                Err(error) => {
                    clear_editor();
                    return Err(format!("打开截图编辑器失败: {error}"));
                }
            };
            let app_for_close = app.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    clear_editor();
                    restore_main_window(&app_for_close);
                }
            });
            if let Err(error) = window.set_position(tauri::Position::Physical(*monitor.position()))
            {
                let _ = window.close();
                return Err(format!("定位截图窗口失败: {error}"));
            }
            if let Err(error) = window.set_size(tauri::Size::Physical(*monitor.size())) {
                let _ = window.close();
                return Err(format!("调整截图窗口失败: {error}"));
            }
            if let Err(error) = window.show() {
                let _ = window.close();
                return Err(format!("显示截图窗口失败: {error}"));
            }
            let _ = window.set_focus();
            Ok(capture_summary(&capture))
        }
        .await;
        if result.is_err() {
            clear_editor();
            restore_main_window(app);
        }
        result
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("capture_unsupported".to_string())
    }
}

pub async fn pending_for_window(window_label: &str) -> Result<PendingCapture, String> {
    let capture = {
        let state = lock_state()?;
        match window_label {
            "capture-editor" => state.editor.clone(),
            "capture-pin" => state.pin.clone(),
            _ => return Err("当前窗口无权读取截图".to_string()),
        }
        .ok_or_else(|| "没有待处理的截图".to_string())?
    };
    read_capture(capture).await
}

async fn managed_outbox(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let outbox = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("应用数据目录不可用: {error}"))?
        .join("media")
        .join("outbox");
    tokio::fs::create_dir_all(&outbox)
        .await
        .map_err(|error| format!("创建图片目录失败: {error}"))?;
    Ok(outbox)
}

async fn write_managed_png(
    app: &tauri::AppHandle,
    bytes: &[u8],
    conversation_id: Option<String>,
) -> Result<ManagedAttachment, String> {
    png_dimensions(bytes)?;
    let outbox = managed_outbox(app).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let path = outbox.join(format!("{id}.png"));
    let pending_path = outbox.join(format!(".{id}.tmp"));
    if let Err(error) = tokio::fs::write(&pending_path, bytes).await {
        remove_file(&pending_path);
        return Err(format!("保存图片失败: {error}"));
    }
    if let Err(error) = tokio::fs::rename(&pending_path, &path).await {
        remove_file(&pending_path);
        return Err(format!("保存图片失败: {error}"));
    }
    Ok(ManagedAttachment {
        file_path: path.to_string_lossy().into_owned(),
        file_name: format!("{id}.png"),
        file_size: bytes.len() as u64,
        mime_type: PNG_MIME.to_string(),
        conversation_id,
    })
}

pub async fn finish(app: &tauri::AppHandle, data_url: String) -> Result<ManagedAttachment, String> {
    let (bytes, _, _) = png_from_data_url(&data_url)?;
    let capture = lock_state()?
        .editor
        .clone()
        .ok_or_else(|| "没有待处理的截图".to_string())?;
    let attachment = write_managed_png(app, &bytes, capture.conversation_id.clone()).await?;
    {
        let mut state = lock_state()?;
        if state.editor.as_ref().map(|item| &item.session_id) != Some(&capture.session_id) {
            remove_file(Path::new(&attachment.file_path));
            return Err("截图会话已变化".to_string());
        }
        state.editor = None;
    }
    remove_file(&capture.path);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("capture-ready", &attachment);
    }
    if let Some(window) = app.get_webview_window("capture-editor") {
        let _ = window.close();
    }
    restore_main_window(app);
    Ok(attachment)
}

pub async fn save(
    app: &tauri::AppHandle,
    data_url: String,
) -> Result<Option<SavedCapture>, String> {
    let (bytes, _, _) = png_from_data_url(&data_url)?;
    let session_id = lock_state()?
        .editor
        .as_ref()
        .map(|capture| capture.session_id.clone())
        .ok_or_else(|| "没有待处理的截图".to_string())?;
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("PNG", &["png"])
        .set_file_name(format!(
            "Xchat-{}.png",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ))
        .set_title("保存截图");
    if let Some(window) = app.get_webview_window("capture-editor") {
        dialog = dialog.set_parent(&window);
    }
    let Some(selected) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    if lock_state()?
        .editor
        .as_ref()
        .map(|capture| &capture.session_id)
        != Some(&session_id)
    {
        return Err("截图会话已变化".to_string());
    }
    let mut path = selected
        .into_path()
        .map_err(|_| "保存路径不可用".to_string())?;
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| !extension.eq_ignore_ascii_case("png"))
        .unwrap_or(true)
    {
        path.set_extension("png");
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "保存路径不可用".to_string())?;
    let pending = path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    if let Err(error) = tokio::fs::write(&pending, &bytes).await {
        remove_file(&pending);
        return Err(format!("保存截图失败: {error}"));
    }
    if let Err(error) = tokio::fs::rename(&pending, &path).await {
        remove_file(&pending);
        return Err(format!("保存截图失败: {error}"));
    }
    Ok(Some(SavedCapture {
        file_path: path.to_string_lossy().into_owned(),
    }))
}

pub fn cancel(app: &tauri::AppHandle) -> Result<(), String> {
    clear_editor();
    let result = if let Some(window) = app.get_webview_window("capture-editor") {
        window
            .close()
            .map_err(|error| format!("关闭截图编辑器失败: {error}"))
    } else {
        Ok(())
    };
    restore_main_window(app);
    result
}

pub async fn pin(
    app: &tauri::AppHandle,
    data_url: String,
) -> Result<CaptureSessionSummary, String> {
    let (bytes, width, height) = png_from_data_url(&data_url)?;
    let editor = lock_state()?
        .editor
        .clone()
        .ok_or_else(|| "没有待处理的截图".to_string())?;
    let capture_dir = std::env::temp_dir().join("xchat-captures");
    tokio::fs::create_dir_all(&capture_dir)
        .await
        .map_err(|error| format!("创建截图缓存目录失败: {error}"))?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let path = capture_dir.join(format!("pin-{session_id}.png"));
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|error| format!("保存钉图失败: {error}"))?;
    let pin = CaptureFile {
        session_id,
        conversation_id: editor.conversation_id.clone(),
        path,
        file_name: "capture.png".to_string(),
        file_size: bytes.len() as u64,
        width,
        height,
    };
    clear_pin();
    {
        let mut state = lock_state()?;
        state.pin = Some(pin.clone());
    }

    let (window_width, window_height) = window_size(width, height, 960.0, 720.0);
    if let Some(window) = app.get_webview_window("capture-pin") {
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            window_width,
            window_height,
        )));
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("capture-pin-updated", capture_summary(&pin));
    } else {
        let window = match WebviewWindowBuilder::new(
            app,
            "capture-pin",
            WebviewUrl::App("index.html?view=capture-pin".into()),
        )
        .title("Xchat 钉图")
        .inner_size(window_width, window_height)
        .center()
        .resizable(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        {
            Ok(window) => window,
            Err(error) => {
                clear_pin();
                return Err(format!("打开钉图窗口失败: {error}"));
            }
        };
        window.on_window_event(|event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                clear_pin();
            }
        });
    }
    {
        let mut state = lock_state()?;
        if state.editor.as_ref().map(|item| &item.session_id) == Some(&editor.session_id) {
            state.editor = None;
        }
    }
    remove_file(&editor.path);
    if let Some(window) = app.get_webview_window("capture-editor") {
        let _ = window.close();
    }
    restore_main_window(app);
    Ok(capture_summary(&pin))
}

pub async fn stage_image(
    app: &tauri::AppHandle,
    data_url: String,
    _file_name: Option<String>,
) -> Result<ManagedAttachment, String> {
    let (bytes, _, _) = png_from_data_url(&data_url)?;
    write_managed_png(app, &bytes, None).await
}

fn trusted_managed_path(outbox: &Path, requested: &Path) -> Result<PathBuf, String> {
    let outbox = outbox
        .canonicalize()
        .map_err(|error| format!("图片目录不可用: {error}"))?;
    let requested = requested
        .canonicalize()
        .map_err(|_| "受管图片已不存在".to_string())?;
    if !requested.is_file() || !requested.starts_with(outbox) {
        return Err("拒绝删除受管目录之外的文件".to_string());
    }
    Ok(requested)
}

pub async fn discard_staged(app: &tauri::AppHandle, file_path: String) -> Result<(), String> {
    let outbox = managed_outbox(app).await?;
    let path = trusted_managed_path(&outbox, Path::new(&file_path))?;
    tokio::fs::remove_file(path)
        .await
        .map_err(|error| format!("删除草稿图片失败: {error}"))
}

pub async fn read_workspace_image(path: &Path) -> Result<WorkspaceMedia, String> {
    let mime_type = mime_guess::from_path(path)
        .first_raw()
        .filter(|mime| mime.starts_with("image/"))
        .ok_or_else(|| "该消息不是可预览图片".to_string())?
        .to_string();
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| "本地图片不可用".to_string())?;
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        return Err("图片不能超过 64 MiB".to_string());
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| "本地图片不可用".to_string())?;
    Ok(WorkspaceMedia {
        data_url: data_url(&mime_type, &bytes),
        mime_type,
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image")
            .to_string(),
        file_size: bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_round_trip_and_reject_non_png() {
        let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";
        let url = data_url(PNG_MIME, bytes);
        let (decoded, width, height) = png_from_data_url(&url).unwrap();
        assert_eq!(decoded, bytes);
        assert_eq!((width, height), (1, 1));
        assert!(png_from_data_url("data:text/plain;base64,SGVsbG8=").is_err());
    }

    #[test]
    fn managed_cleanup_rejects_paths_outside_outbox() {
        let root = std::env::temp_dir().join(format!("xchat-outbox-test-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("xchat-outside-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let inside = root.join("inside.png");
        std::fs::write(&inside, b"inside").unwrap();
        std::fs::write(&outside, b"outside").unwrap();

        assert_eq!(
            trusted_managed_path(&root, &inside).unwrap(),
            inside.canonicalize().unwrap()
        );
        assert!(trusted_managed_path(&root, &outside).is_err());

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_file(outside).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn display_numbers_follow_the_current_monitor_geometry() {
        let monitors = [
            (0, 0, 1728, 1117),
            (1728, 0, 2560, 1440),
            (-1920, 0, 1920, 1080),
        ];
        assert_eq!(
            display_number_for_geometry((1728, 0, 2560, 1440), &monitors),
            2,
        );
        assert_eq!(display_number_for_geometry((999, 999, 1, 1), &monitors), 1,);
    }
}
