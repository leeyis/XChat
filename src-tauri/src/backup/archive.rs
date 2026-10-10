//! A bounded, streamed container. Entry names never become filesystem paths.
use super::{Job, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAGIC: &[u8; 16] = b"XCHATBACKUP\0\x01\0\0\0";
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const BUFFER: usize = 256 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub message_key: Option<String>,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub created_at: i64,
    pub app_version: String,
    pub source_id: String,
    pub settings: bool,
    pub attachments: bool,
    pub messages: u64,
    pub entries: Vec<Entry>,
}

pub struct Verified {
    pub manifest: Manifest,
    pub files: Vec<PathBuf>,
}

/// Copy exactly the declared payload; cancellation and hashes are checked per chunk.
async fn copy_exact(
    input: &mut tokio::fs::File,
    mut output: Option<&mut tokio::fs::File>,
    bytes: u64,
    job: &Job,
) -> Result<String> {
    let mut buffer = vec![0u8; BUFFER];
    let mut left = bytes;
    let mut digest = Sha256::new();
    while left > 0 {
        job.check_cancelled()?;
        let size = left.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..size]).await?;
        if let Some(file) = output.as_mut() {
            file.write_all(&buffer[..size]).await?;
        }
        digest.update(&buffer[..size]);
        left -= size as u64;
        job.advance(size as u64);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub async fn copy_file(source: &Path, destination: &Path, job: &Job) -> Result<Entry> {
    let mut input = tokio::fs::File::open(source).await?;
    let before = input.metadata().await?;
    if !before.is_file() {
        return Err("附件不是普通文件".into());
    }
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await?;
    let sha256 = copy_exact(&mut input, Some(&mut output), before.len(), job).await?;
    let after = input.metadata().await?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("附件在备份过程中发生变化，请稍后重试".into());
    }
    output.sync_all().await?;
    Ok(Entry {
        message_key: None,
        bytes: before.len(),
        sha256,
    })
}

pub async fn hash_file(path: &Path, job: &Job) -> Result<Entry> {
    let mut input = tokio::fs::File::open(path).await?;
    let bytes = input.metadata().await?.len();
    let sha256 = copy_exact(&mut input, None, bytes, job).await?;
    Ok(Entry {
        message_key: None,
        bytes,
        sha256,
    })
}

pub async fn write(path: &Path, manifest: &Manifest, files: &[PathBuf], job: &Job) -> Result<()> {
    if files.len() != manifest.entries.len() {
        return Err("备份清单与文件不一致".into());
    }
    let encoded = serde_json::to_vec(manifest)?;
    if encoded.len() as u64 > MAX_MANIFEST {
        return Err("备份附件清单过大，请分批备份".into());
    }
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
    output.write_all(MAGIC).await?;
    output
        .write_all(&(encoded.len() as u64).to_le_bytes())
        .await?;
    output.write_all(&Sha256::digest(&encoded)).await?;
    output.write_all(&encoded).await?;
    job.phase(
        "正在写入备份文件",
        manifest.entries.iter().map(|e| e.bytes).sum(),
    );
    for (entry, file) in manifest.entries.iter().zip(files) {
        let mut input = tokio::fs::File::open(file).await?;
        if copy_exact(&mut input, Some(&mut output), entry.bytes, job).await? != entry.sha256 {
            return Err("备份暂存文件校验失败".into());
        }
    }
    output.sync_all().await?;
    Ok(())
}

/// With a destination, verified payloads are extracted into newly generated numeric paths.
pub async fn verify(path: &Path, destination: Option<&Path>, job: &Job) -> Result<Verified> {
    let mut input = tokio::fs::File::open(path).await?;
    let length = input.metadata().await?.len();
    let mut magic = [0u8; 16];
    input
        .read_exact(&mut magic)
        .await
        .map_err(|_| "备份文件不完整")?;
    if &magic != MAGIC {
        return Err("不是受支持的 XChat 备份，或备份版本不兼容".into());
    }
    let manifest_bytes = input.read_u64_le().await?;
    if manifest_bytes == 0
        || manifest_bytes > MAX_MANIFEST
        || manifest_bytes.saturating_add(56) > length
    {
        return Err("备份清单长度无效或文件不完整".into());
    }
    let mut expected_manifest = [0u8; 32];
    input.read_exact(&mut expected_manifest).await?;
    let mut encoded = vec![0u8; manifest_bytes as usize];
    input.read_exact(&mut encoded).await?;
    if Sha256::digest(&encoded)[..] != expected_manifest {
        return Err("备份清单摘要不匹配".into());
    }
    let manifest: Manifest = serde_json::from_slice(&encoded).map_err(|_| "备份清单已损坏")?;
    if manifest.version != 1
        || manifest.source_id.is_empty()
        || manifest.source_id.len() > 128
        || manifest.entries.is_empty()
        || manifest.entries.len() > 100_001
        || manifest.entries[0].message_key.is_some()
    {
        return Err("备份版本或内容清单不受支持".into());
    }
    let mut size = manifest_bytes + 56;
    let mut keys = HashSet::new();
    for (index, entry) in manifest.entries.iter().enumerate() {
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("备份摘要无效".into());
        }
        if index > 0
            && (!manifest.attachments
                || entry.message_key.as_ref().is_none_or(|key| {
                    key.is_empty() || key.len() > 256 || !keys.insert(key.clone())
                }))
        {
            return Err("备份附件标识无效或重复".into());
        }
        size = size.checked_add(entry.bytes).ok_or("备份长度溢出")?;
    }
    if size != length {
        return Err("备份文件不完整或包含额外数据".into());
    }
    if let Some(root) = destination {
        super::require_space(root, length).await?;
    }
    job.phase("正在校验备份完整性", length);
    let mut files = Vec::new();
    for (index, entry) in manifest.entries.iter().enumerate() {
        let path = destination.map(|root| root.join(format!("entry-{index}")));
        let mut output = match path.as_ref() {
            Some(path) => Some(
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .await?,
            ),
            None => None,
        };
        if copy_exact(&mut input, output.as_mut(), entry.bytes, job).await? != entry.sha256 {
            return Err(format!("备份第 {} 项摘要不匹配，当前聊天记录未改变", index + 1).into());
        }
        if let Some(output) = output.as_mut() {
            output.sync_all().await?;
        }
        if let Some(path) = path {
            files.push(path);
        }
    }
    Ok(Verified { manifest, files })
}
