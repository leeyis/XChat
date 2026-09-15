//! 跨平台的图片落盘与读取工具。
//!
//! 这些能力原本长在 `capture_editor` 里，但移动端同样在用（看图预览、
//! 附件图片暂存），因此单独成模块，让 `capture_editor` 得以整体只编译到桌面端。

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

pub const PNG_MIME: &str = "image/png";
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ManagedAttachment {
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub mime_type: String,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceMedia {
    pub data_url: String,
    pub mime_type: String,
    pub file_name: String,
    pub file_size: u64,
}

pub(crate) fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
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

pub(crate) fn png_from_data_url(data_url: &str) -> Result<(Vec<u8>, u32, u32), String> {
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

pub(crate) fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", encode_base64(bytes))
}

pub(crate) fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("[Capture] 清理临时图片失败: {error}");
        }
    }
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

pub(crate) async fn write_managed_png(
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

pub async fn stage_image(
    app: &tauri::AppHandle,
    data_url: String,
    _file_name: Option<String>,
) -> Result<ManagedAttachment, String> {
    let (bytes, _, _) = png_from_data_url(&data_url)?;
    write_managed_png(app, &bytes, None).await
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
}
