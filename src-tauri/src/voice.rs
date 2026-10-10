//! Voice messages use the durable attachment queue with explicit media metadata.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite, SqliteConnection};
use std::path::PathBuf;

pub const MAX_VOICE_BYTES: usize = 8 * 1024 * 1024;
static SEND_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub struct VoiceMetadata {
    pub duration_ms: i64,
    pub mime_type: String,
}

impl VoiceMetadata {
    pub fn extension(&self) -> Result<&'static str, String> {
        if !(1000..=60_000).contains(&self.duration_ms) {
            return Err("语音时长必须在 1–60 秒之间".into());
        }
        match self.mime_type.as_str() {
            "audio/webm" => Ok("webm"),
            "audio/ogg" => Ok("ogg"),
            "audio/mp4" => Ok("m4a"),
            "audio/wav" => Ok("wav"),
            _ => Err("不支持的语音格式".into()),
        }
    }

    pub fn validate(&self, size: u64) -> Result<(), String> {
        self.extension()?;
        if size == 0 || size > MAX_VOICE_BYTES as u64 {
            return Err("语音文件大小无效（最大 8 MiB）".into());
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendVoiceRequest {
    pub conversation_id: String,
    pub recording_id: String,
    pub duration_ms: i64,
    pub data_url: String,
}

pub async fn init_schema(pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS voice_metadata (
        message_client_id TEXT PRIMARY KEY REFERENCES messages(client_message_id) ON DELETE CASCADE,
        duration_ms INTEGER NOT NULL CHECK(duration_ms BETWEEN 1000 AND 60000),
        mime_type TEXT NOT NULL)")
        .execute(pool).await?;
    Ok(())
}

pub async fn metadata(pool: &Pool<Sqlite>, id: &str) -> Result<Option<VoiceMetadata>, String> {
    sqlx::query_as("SELECT duration_ms, mime_type FROM voice_metadata WHERE message_client_id = ?")
        .bind(id).fetch_optional(pool).await.map_err(|e| e.to_string())
}

pub async fn save_on(connection: &mut SqliteConnection, id: &str, voice: &VoiceMetadata) -> Result<(), String> {
    voice.extension()?;
    sqlx::query("INSERT INTO voice_metadata (message_client_id, duration_ms, mime_type)
        VALUES (?, ?, ?) ON CONFLICT(message_client_id) DO NOTHING")
        .bind(id).bind(voice.duration_ms).bind(&voice.mime_type)
        .execute(&mut *connection).await.map_err(|e| e.to_string())?;
    let stored: VoiceMetadata = sqlx::query_as("SELECT duration_ms, mime_type FROM voice_metadata WHERE message_client_id = ?")
        .bind(id).fetch_one(connection).await.map_err(|e| e.to_string())?;
    if &stored != voice { return Err("语音元数据与已有消息冲突".into()); }
    Ok(())
}

fn decode(request: &SendVoiceRequest) -> Result<(Vec<u8>, VoiceMetadata), String> {
    if request.data_url.len() > MAX_VOICE_BYTES * 4 / 3 + 256 {
        return Err("语音文件过大".into());
    }
    let (header, encoded) = request.data_url.split_once(',').ok_or("语音内容无效")?;
    let mime_type = header.strip_prefix("data:").and_then(|h| h.strip_suffix(";base64"))
        .ok_or("语音编码无效")?.split(';').next().unwrap_or_default().to_string();
    let voice = VoiceMetadata { duration_ms: request.duration_ms, mime_type };
    voice.extension()?;
    let bytes = STANDARD.decode(encoded).map_err(|_| "语音 Base64 内容无效")?;
    voice.validate(bytes.len() as u64)?;
    validate_container(&bytes, &voice.mime_type)?;
    Ok((bytes, voice))
}

pub fn validate_container(bytes: &[u8], mime: &str) -> Result<(), String> {
    let valid = match mime {
        "audio/webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        "audio/ogg" => bytes.starts_with(b"OggS"),
        "audio/mp4" => bytes.get(4..8) == Some(b"ftyp".as_slice()),
        "audio/wav" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE".as_slice()),
        _ => false,
    };
    if !valid { return Err("语音内容与声明格式不一致".into()); }
    Ok(())
}

pub async fn send(pool: &Pool<Sqlite>, peers: &crate::peers::PeerManager, request: SendVoiceRequest)
    -> Result<crate::network::conversation_file::ConversationFileSendResult, String>
{
    let (bytes, voice) = decode(&request)?;
    send_bytes(pool, peers, &request.conversation_id, &request.recording_id, voice, bytes).await
}

pub async fn send_bytes(pool: &Pool<Sqlite>, peers: &crate::peers::PeerManager, conversation_id: &str,
    recording_id: &str, voice: VoiceMetadata, bytes: Vec<u8>)
    -> Result<crate::network::conversation_file::ConversationFileSendResult, String>
{
    let id = uuid::Uuid::parse_str(recording_id).map_err(|_| "录音 ID 无效")?.to_string();
    voice.validate(bytes.len() as u64)?;
    validate_container(&bytes, &voice.mime_type)?;
    let _guard = SEND_LOCK.lock().await;
    let existing = crate::db::get_message_by_client_id(pool, &id).await?;
    if let Some(message) = &existing {
        if message.conversation_id.as_deref() != Some(conversation_id) || message.msg_type != "voice"
            || message.sender_id != crate::db::get_user_id(pool).await?
            || metadata(pool, &id).await?.as_ref() != Some(&voice) {
            return Err("录音 ID 与已有消息冲突".into());
        }
    }
    let root = PathBuf::from(crate::db::get_download_path(pool).await?);
    tokio::fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
    let root = tokio::fs::canonicalize(root).await.map_err(|e| e.to_string())?;
    let outbox = root.join(".xchat-voice");
    tokio::fs::create_dir_all(&outbox).await.map_err(|e| e.to_string())?;
    let outbox = tokio::fs::canonicalize(outbox).await.map_err(|e| e.to_string())?;
    if !outbox.starts_with(&root) { return Err("语音发件箱路径无效".into()); }
    // Once enqueued, the original managed path remains the stable retry source.
    let path = existing.as_ref().and_then(|m| m.file_path.as_ref()).map(PathBuf::from)
        .unwrap_or_else(|| outbox.join(format!("语音-{id}.{}", voice.extension().unwrap())));
    if path.exists() {
        let stored = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
        if stored != bytes { return Err("录音 ID 与已有音频内容冲突".into()); }
    } else {
        if existing.is_some() { return Err("该语音已入队但本地文件缺失，请从任务中心恢复".into()); }
        let temp = outbox.join(format!("{id}.{}.tmp", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&temp).await.map_err(|e| e.to_string())?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes).await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        tokio::fs::rename(&temp, &path).await.map_err(|e| e.to_string())?;
    }
    crate::network::conversation_file::send_path_identified(pool, peers, conversation_id,
        path.to_str().ok_or("语音路径无效")?, &id, Some(&voice)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio() -> Vec<u8> { b"RIFF\x00\x00\x00\x00WAVEfmt voice test data".to_vec() }

    #[test]
    fn voice_bounds_and_format_are_validated() {
        let mut voice = VoiceMetadata { duration_ms: 1500, mime_type: "audio/wav".into() };
        assert!(voice.validate(30).is_ok());
        assert!(voice.validate(0).is_err());
        assert!(voice.validate(MAX_VOICE_BYTES as u64 + 1).is_err());
        voice.duration_ms = 60_001; assert!(voice.validate(30).is_err());
        voice.duration_ms = 999; assert!(voice.validate(30).is_err());
        assert!(validate_container(b"<script>", "audio/wav").is_err());
        assert!(validate_container(&audio(), "audio/wav").is_ok());
    }

    #[tokio::test]
    async fn voice_queue_is_atomic_idempotent_and_rejects_recording_id_conflicts() {
        let root = std::env::temp_dir().join(format!("xchat-voice-test-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(root.clone())).await.unwrap();
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('download_path', ?)")
            .bind(root.join("downloads").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        let conversation = crate::db::ensure_direct_conversation(&pool, "voice-peer").await.unwrap();
        let peers = crate::peers::PeerManager::new();
        let id = uuid::Uuid::new_v4().to_string();
        let voice = VoiceMetadata { duration_ms: 1500, mime_type: "audio/wav".into() };
        let first = send_bytes(&pool, &peers, &conversation.id, &id, voice.clone(), audio()).await.unwrap();
        let retry = send_bytes(&pool, &peers, &conversation.id, &id, voice.clone(), audio()).await.unwrap();
        assert_eq!(first.message.id, retry.message.id);
        assert_eq!(first.message.msg_type, "voice");
        assert_eq!(retry.transfers.len(), 1);
        assert_eq!(retry.transfers[0].status, "waiting_peer");
        assert_eq!(metadata(&pool, &id).await.unwrap(), Some(voice.clone()));
        assert!(crate::db::get_file_message_by_id(&pool, first.message.id).await.unwrap().is_some());
        let mut changed = voice.clone(); changed.duration_ms = 2000;
        assert!(send_bytes(&pool, &peers, &conversation.id, &id, changed, audio()).await.is_err());
        let mut bytes = audio(); bytes.push(7);
        assert!(send_bytes(&pool, &peers, &conversation.id, &id, voice.clone(), bytes).await.is_err());
        let second = uuid::Uuid::new_v4().to_string();
        sqlx::query("CREATE TRIGGER voice_fail BEFORE INSERT ON transfers BEGIN SELECT RAISE(ABORT, 'injected'); END")
            .execute(&pool).await.unwrap();
        assert!(send_bytes(&pool, &peers, &conversation.id, &second, voice, audio()).await.is_err());
        assert!(crate::db::get_message_by_client_id(&pool, &second).await.unwrap().is_none());
        assert!(metadata(&pool, &second).await.unwrap().is_none());
        pool.close().await;
        assert!(root.starts_with(std::env::temp_dir()) && root.file_name().unwrap().to_string_lossy().starts_with("xchat-voice-test-"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
