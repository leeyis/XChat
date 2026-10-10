//! The archive stores a deliberately small, fixed SQLite schema, never live queues or grants.
use super::{archive, Job, Result};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
    Connection, Row, SqliteConnection,
};
use std::path::{Path, PathBuf};

const SCHEMA: &[&str] = &[
    "CREATE TABLE conversations(id TEXT PRIMARY KEY,kind TEXT NOT NULL,peer_id TEXT,title TEXT,created_by TEXT,pinned INTEGER NOT NULL,draft TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,version INTEGER NOT NULL)",
    "CREATE TABLE members(conversation_id TEXT NOT NULL,peer_id TEXT NOT NULL,display_name TEXT NOT NULL,role TEXT NOT NULL,joined_at INTEGER NOT NULL,PRIMARY KEY(conversation_id,peer_id))",
    "CREATE TABLE contacts(id TEXT PRIMARY KEY,name TEXT NOT NULL,remark TEXT)",
    "CREATE TABLE messages(client_message_id TEXT PRIMARY KEY,original_id INTEGER NOT NULL,sender_id TEXT NOT NULL,receiver_id TEXT,content TEXT NOT NULL,msg_type TEXT NOT NULL,timestamp INTEGER NOT NULL,file_size INTEGER,status TEXT,conversation_id TEXT)",
    "CREATE TABLE receipts(message_client_id TEXT NOT NULL,reader_id TEXT NOT NULL,mentioned INTEGER NOT NULL,delivered_at INTEGER,read_at INTEGER,updated_at INTEGER NOT NULL,PRIMARY KEY(message_client_id,reader_id))",
    "CREATE TABLE reactions(message_client_id TEXT NOT NULL,reactor_id TEXT NOT NULL,emoji TEXT NOT NULL,updated_at INTEGER NOT NULL,PRIMARY KEY(message_client_id,reactor_id))",
    "CREATE TABLE voice(message_client_id TEXT PRIMARY KEY,duration_ms INTEGER NOT NULL,mime_type TEXT NOT NULL)",
    "CREATE TABLE preferences(key TEXT PRIMARY KEY,value TEXT NOT NULL)",
];
pub const PREFERENCES: &str = "'current_theme','avatar','notifications_enabled','auto_download','file_transfer.max_parallel_channels.v1'";

pub async fn connect(path: &Path, create: bool) -> Result<SqliteConnection> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create)
        .read_only(!create)
        .journal_mode(SqliteJournalMode::Delete)
        .pragma("trusted_schema", "OFF")
        .pragma("busy_timeout", "10000");
    Ok(SqliteConnection::connect_with(&options).await?)
}

pub async fn export(
    snapshot: &Path,
    destination: &Path,
    source_id: &str,
    settings: bool,
    job: &Job,
) -> Result<()> {
    let mut connection = connect(destination, true).await?;
    for sql in SCHEMA {
        sqlx::query(sql).execute(&mut connection).await?;
    }
    sqlx::query("ATTACH DATABASE ? AS source")
        .bind(snapshot.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await?;
    let mut tx = connection.begin().await?;
    for sql in [
        "INSERT INTO conversations SELECT id,kind,peer_id,title,created_by,pinned,draft,created_at,updated_at,version FROM source.conversations",
        "INSERT INTO members SELECT conversation_id,peer_id,display_name,role,joined_at FROM source.conversation_members",
        "INSERT INTO contacts SELECT id,COALESCE(name,id),remark FROM source.users",
        "INSERT INTO receipts SELECT message_client_id,reader_id,mentioned,delivered_at,read_at,updated_at FROM source.message_receipts WHERE EXISTS(SELECT 1 FROM source.messages m WHERE m.client_message_id=message_client_id)",
        "INSERT INTO reactions SELECT message_client_id,reactor_id,emoji,updated_at FROM source.message_reactions WHERE EXISTS(SELECT 1 FROM source.messages m WHERE m.client_message_id=message_client_id)",
        "INSERT INTO voice SELECT message_client_id,duration_ms,mime_type FROM source.voice_metadata",
    ] {
        job.check_cancelled()?;
        sqlx::query(sql).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO messages SELECT COALESCE(client_message_id,'legacy:'||?||':'||id),id,sender_id,receiver_id,COALESCE(content,''),msg_type,timestamp,file_size,status,conversation_id FROM source.messages")
        .bind(source_id).execute(&mut *tx).await?;
    if settings {
        sqlx::query(&format!("INSERT INTO preferences SELECT key,value FROM source.settings WHERE key IN ({PREFERENCES}) AND value IS NOT NULL AND length(value)<=4096"))
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    connection.close().await?;
    Ok(())
}

pub async fn validate(path: &Path, manifest: &archive::Manifest) -> Result<()> {
    let mut connection = connect(path, false).await?;
    // Reject views, triggers, virtual tables and any schema supplied by a different format.
    let schema: Vec<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&mut connection)
    .await?;
    if schema.len() != SCHEMA.len() || schema.iter().any(|sql| !SCHEMA.contains(&sql.as_str())) {
        return Err("备份数据库结构不受支持或已被修改".into());
    }
    let check: String = sqlx::query_scalar("PRAGMA integrity_check(1)")
        .fetch_one(&mut connection)
        .await?;
    if check != "ok" {
        return Err("备份数据库完整性校验失败".into());
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut connection)
        .await?;
    if count as u64 != manifest.messages {
        return Err("备份记录数与清单不同".into());
    }
    let invalid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE client_message_id IS NULL OR length(client_message_id) NOT BETWEEN 1 AND 256 OR length(sender_id)>128 OR length(content)>16777216 OR msg_type NOT IN ('text','quote','announcement','file','voice') OR file_size<0 OR conversation_id NOT IN (SELECT id FROM conversations))
        OR EXISTS(SELECT 1 FROM conversations WHERE kind NOT IN ('direct','group') OR length(id)>512 OR (kind='direct' AND (peer_id IS NULL OR peer_id='')))
        OR EXISTS(SELECT 1 FROM members WHERE conversation_id NOT IN (SELECT id FROM conversations) OR length(peer_id)>128)
        OR EXISTS(SELECT 1 FROM voice WHERE duration_ms NOT BETWEEN 1000 AND 60000 OR mime_type NOT IN ('audio/webm','audio/ogg','audio/mp4','audio/wav') OR message_client_id NOT IN (SELECT client_message_id FROM messages WHERE msg_type='voice'))
        OR EXISTS(SELECT 1 FROM messages WHERE msg_type='voice' AND client_message_id NOT IN (SELECT message_client_id FROM voice))")
        .fetch_one(&mut connection).await?;
    if invalid {
        return Err("备份包含无效记录或不完整的会话关系".into());
    }
    for entry in manifest.entries.iter().skip(1) {
        let size: Option<i64> = sqlx::query_scalar("SELECT file_size FROM messages WHERE client_message_id=? AND msg_type IN ('file','voice')")
            .bind(&entry.message_key).fetch_optional(&mut connection).await?.flatten();
        if size != Some(entry.bytes as i64) {
            return Err("备份附件与聊天记录不一致".into());
        }
    }
    let unexpected: bool = sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM preferences WHERE key NOT IN ({PREFERENCES}) OR length(value)>4096)"))
        .fetch_one(&mut connection).await?;
    if unexpected
        || (!manifest.settings
            && sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM preferences")
                .fetch_one(&mut connection)
                .await?
                > 0)
    {
        return Err("备份包含不允许迁移的设置".into());
    }
    connection.close().await?;
    Ok(())
}

pub async fn attachments(snapshot: &Path, source_id: &str) -> Result<Vec<(String, PathBuf, u64)>> {
    let mut connection = connect(snapshot, false).await?;
    let rows = sqlx::query("SELECT COALESCE(client_message_id,'legacy:'||?||':'||id) key,file_path,file_size FROM messages WHERE msg_type IN ('file','voice') AND file_path IS NOT NULL AND file_size>=0 AND COALESCE(file_status,'') NOT IN ('receiving','downloading')")
        .bind(source_id).fetch_all(&mut connection).await?;
    let mut files = Vec::new();
    for row in rows {
        let path = PathBuf::from(row.try_get::<String, _>("file_path")?);
        let size = row.try_get::<i64, _>("file_size")? as u64;
        if tokio::fs::metadata(&path)
            .await
            .is_ok_and(|meta| meta.is_file() && meta.len() == size)
        {
            files.push((row.try_get("key")?, path, size));
        }
    }
    connection.close().await?;
    Ok(files)
}
