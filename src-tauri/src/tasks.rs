//! The task center is a projection of durable deliveries and transfers, not another queue.
use crate::{db, peers::PeerManager};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

pub async fn init_schema(pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS file_sources(message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
        bytes_total INTEGER NOT NULL, modified_ns TEXT NOT NULL, sha256 TEXT)")
        .execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS task_hidden(message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE)")
        .execute(pool).await?;
    Ok(())
}

fn modified(meta: &std::fs::Metadata) -> String {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|n| n.as_nanos().to_string()).unwrap_or_default()
}

pub(crate) async fn remember_source(pool: &Pool<Sqlite>, message_id: i64, path: &str) -> Result<(), String> {
    let meta = tokio::fs::metadata(path).await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT OR IGNORE INTO file_sources(message_id,bytes_total,modified_ns) VALUES(?,?,?)")
        .bind(message_id).bind(meta.len() as i64).bind(modified(&meta)).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn source_unchanged(pool: &Pool<Sqlite>, message_id: i64, path: &str) -> Result<(), String> {
    let source: Option<(i64, String)> = sqlx::query_as("SELECT bytes_total,modified_ns FROM file_sources WHERE message_id=?")
        .bind(message_id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    if let Some((size, stamp)) = source {
        let meta = tokio::fs::metadata(path).await.map_err(|_| "源文件不存在，请从任务中心重新选择")?;
        if meta.len() as i64 != size || modified(&meta) != stamp { return Err("源文件已发生变化，请从任务中心重新选择并校验".into()); }
    }
    Ok(())
}

pub(crate) async fn pin_digest(pool: &Pool<Sqlite>, message_id: i64, path: &str, digest: &str) -> Result<(), String> {
    source_unchanged(pool, message_id, path).await?;
    sqlx::query("UPDATE file_sources SET sha256=? WHERE message_id=? AND sha256 IS NULL")
        .bind(digest).bind(message_id).execute(pool).await.map_err(|e| e.to_string())?;
    let stored: Option<String> = sqlx::query_scalar("SELECT sha256 FROM file_sources WHERE message_id=?")
        .bind(message_id).fetch_optional(pool).await.map_err(|e| e.to_string())?.flatten();
    if stored.as_deref().is_some_and(|value| value != digest) { return Err("源文件内容与原任务不一致".into()); }
    Ok(())
}

/// Restore to a managed copy so a picked external file is never silently substituted later.
pub async fn replace_source(pool: &Pool<Sqlite>, message_id: i64, source_path: &str) -> Result<(), String> {
    let message = db::get_file_message_by_id(pool, message_id).await?.ok_or("文件任务不存在")?;
    if message.sender_id != db::get_user_id(pool).await? { return Err("只能重新选择本机发送的源文件".into()); }
    let expected: Option<String> = sqlx::query_scalar("SELECT sha256 FROM file_sources WHERE message_id=?")
        .bind(message_id).fetch_optional(pool).await.map_err(|e| e.to_string())?.flatten();
    let expected = expected.ok_or("原任务尚未留下完整摘要，无法证明文件相同；请保留原任务并重新发送文件")?;
    let meta = tokio::fs::metadata(source_path).await.map_err(|e| e.to_string())?;
    if !meta.is_file() || message.file_size != Some(meta.len() as i64) { return Err("文件大小与原任务不同，不能续传".into()); }
    let root = std::path::PathBuf::from(db::get_download_path(pool).await?).join(".xchat-restored-sources").join(uuid::Uuid::new_v4().to_string());
    tokio::fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
    let name = std::path::Path::new(&message.content).file_name().ok_or("原文件名无效")?;
    let destination = root.join(name);
    let result = async {
        tokio::fs::copy(source_path, &destination).await.map_err(|e| e.to_string())?;
        let digest = crate::network::conversation_file::sha256_file(&destination).await?;
        if digest != expected { return Err("文件内容与原任务不同，不能续传；请选择原文件".to_string()); }
        let meta = tokio::fs::metadata(&destination).await.map_err(|e| e.to_string())?;
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        let updated = sqlx::query("UPDATE messages SET file_path=? WHERE id=? AND NOT EXISTS(SELECT 1 FROM transfers WHERE message_id=?
            AND status NOT IN ('failed','cancelled','completed','rejected','expired'))")
            .bind(destination.to_string_lossy().as_ref()).bind(message_id).bind(message_id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        if updated.rows_affected() == 0 { return Err("任务仍在运行，请先取消未完成传输，再重新选择".into()); }
        sqlx::query("UPDATE file_sources SET modified_ns=? WHERE message_id=?").bind(modified(&meta)).bind(message_id)
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }.await;
    if result.is_err() { let _ = tokio::fs::remove_file(&destination).await; let _ = tokio::fs::remove_dir(&root).await; }
    result
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Recipient {
    #[serde(skip)]
    pub message_id: i64,
    pub peer_id: String,
    pub name: String,
    pub state: String,
    pub error: Option<String>,
    pub bytes_total: i64,
    pub bytes_transferred: i64,
    pub attempt_count: i64,
    pub next_retry_at: i64,
    pub transfer_id: Option<String>,
    pub delivered_at: Option<i64>,
}

#[derive(Serialize)]
pub struct Task {
    pub message: db::MessageRecord,
    pub recipients: Vec<Recipient>,
    pub source_available: bool,
}

#[derive(Serialize)]
pub struct TaskPage { pub tasks: Vec<Task>, pub next_before: Option<i64> }

pub async fn list(pool: &Pool<Sqlite>, before: Option<i64>) -> Result<TaskPage, String> {
    let self_id = db::get_user_id(pool).await?;
    let messages = sqlx::query_as::<_, db::MessageRecord>(
        "SELECT m.* FROM messages m WHERE m.id < ? AND m.client_message_id IS NOT NULL AND COALESCE(m.status,'')!='restored'
         AND NOT EXISTS(SELECT 1 FROM task_hidden h WHERE h.message_id=m.id)
         AND NOT EXISTS(SELECT 1 FROM restored_messages h WHERE h.message_client_id=m.client_message_id)
         AND (EXISTS(SELECT 1 FROM transfers t WHERE t.message_id = m.id)
           OR (m.sender_id IN (?, 'me') AND m.msg_type IN ('text','quote','announcement')
               AND EXISTS(SELECT 1 FROM message_receipts r WHERE r.message_client_id = m.client_message_id)))
         ORDER BY m.id DESC LIMIT 101")
        .bind(before.unwrap_or(i64::MAX)).bind(self_id).fetch_all(pool).await.map_err(|e| e.to_string())?;
    let next_before = (messages.len() > 100).then(|| messages[99].id);
    if messages.is_empty() { return Ok(TaskPage { tasks: vec![], next_before: None }); }
    // IDs originate as SQLite i64 values. Two bounded queries load all recipient rows for this page.
    let ids = messages.iter().take(100).map(|m| m.id.to_string()).collect::<Vec<_>>().join(",");
    let mut recipients_by_message: std::collections::HashMap<i64, Vec<Recipient>> = std::collections::HashMap::new();
    let transfers = sqlx::query_as::<_, Recipient>(&format!("WITH latest AS (SELECT t.*, ROW_NUMBER() OVER
        (PARTITION BY t.message_id,t.peer_id,t.direction ORDER BY t.created_at DESC,t.rowid DESC) position
        FROM transfers t WHERE t.message_id IN ({ids}))
        SELECT t.message_id, t.peer_id, COALESCE(NULLIF(u.remark,''),u.name,t.peer_id) name,
        t.status state,t.error,t.bytes_total,t.bytes_transferred,0 attempt_count,0 next_retry_at,t.id transfer_id,NULL delivered_at
        FROM latest t LEFT JOIN users u ON u.id=t.peer_id WHERE t.position=1 ORDER BY t.created_at,t.id"))
        .fetch_all(pool).await.map_err(|e| e.to_string())?;
    let deliveries = sqlx::query_as::<_, Recipient>(&format!("SELECT m.id message_id, r.reader_id peer_id,COALESCE(NULLIF(u.remark,''),u.name,r.reader_id) name,
        CASE WHEN a.state='cancelled' OR m.status='recalled' THEN 'cancelled' WHEN r.read_at IS NOT NULL THEN 'read'
          WHEN r.delivered_at IS NOT NULL THEN 'completed' ELSE COALESCE(a.state,'waiting_connection') END state,
        a.last_error error,0 bytes_total,0 bytes_transferred,COALESCE(a.attempt_count,0) attempt_count,
        COALESCE(a.next_retry_at,0) next_retry_at,NULL transfer_id,r.delivered_at
        FROM messages m JOIN message_receipts r ON r.message_client_id=m.client_message_id
        LEFT JOIN message_delivery_attempts a ON a.message_client_id=r.message_client_id AND a.reader_id=r.reader_id
        LEFT JOIN users u ON u.id=r.reader_id WHERE m.id IN ({ids}) AND m.msg_type NOT IN ('file','voice') ORDER BY r.reader_id"))
        .fetch_all(pool).await.map_err(|e| e.to_string())?;
    for recipient in transfers.into_iter().chain(deliveries) {
        recipients_by_message.entry(recipient.message_id).or_default().push(recipient);
    }
    let mut tasks = Vec::new();
    for message in messages.into_iter().take(100) {
        let recipients = recipients_by_message.remove(&message.id).unwrap_or_default();
        let source_available = match message.file_path.as_deref() {
            Some(path) => tokio::fs::metadata(path).await.is_ok_and(|meta| meta.is_file()),
            None => false,
        };
        tasks.push(Task { message, recipients, source_available });
    }
    Ok(TaskPage { tasks, next_before })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAction {
    pub message_id: i64,
    pub action: String,
    pub peer_id: Option<String>,
}

pub async fn act(pool: &Pool<Sqlite>, peers: &PeerManager, request: TaskAction) -> Result<(), String> {
    let message = db::get_message_by_id(pool, request.message_id).await?.ok_or("任务不存在")?;
    if crate::backup::is_history(pool, message.client_message_id.as_deref().unwrap_or_default()).await? {
        return Err("恢复的历史不属于发送任务；需要再次发送时，请转发为新消息".into());
    }
    if request.action == "dismiss" {
        let changed = sqlx::query("INSERT OR IGNORE INTO task_hidden(message_id) SELECT ?
            WHERE NOT EXISTS(SELECT 1 FROM transfers WHERE message_id=? AND status NOT IN ('completed','cancelled','rejected','expired'))
              AND NOT EXISTS(SELECT 1 FROM message_receipts r LEFT JOIN message_delivery_attempts a
                ON a.message_client_id=r.message_client_id AND a.reader_id=r.reader_id
                WHERE r.message_client_id=? AND r.delivered_at IS NULL AND r.read_at IS NULL
                  AND COALESCE(a.state,'')!='cancelled' AND ? NOT IN ('file','voice'))")
            .bind(message.id).bind(message.id).bind(&message.client_message_id).bind(&message.msg_type)
            .execute(pool).await.map_err(|e| e.to_string())?;
        if changed.rows_affected()==0 && !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM task_hidden WHERE message_id=?)")
            .bind(message.id).fetch_one(pool).await.map_err(|e| e.to_string())? { return Err("未完成任务不能清理".into()); }
        return Ok(());
    }
    if !matches!(request.action.as_str(), "cancel" | "retry") { return Err("任务操作无效".into()); }
    if matches!(message.msg_type.as_str(), "file" | "voice") {
        if request.action == "retry" {
            // Existing retry skips completed recipients and keeps stable message/transfer IDs.
            crate::network::conversation_file::retry_message(pool, peers, message.id).await?;
        } else {
            let transfers = sqlx::query_as::<_, (String, String)>("SELECT id,peer_id FROM transfers WHERE message_id = ?
                AND status IN ('queued','waiting_peer','offering','awaiting_acceptance','transferring','receiving','uploading','downloading')")
                .bind(message.id).fetch_all(pool).await.map_err(|e| e.to_string())?;
            for (id, peer) in transfers {
                if request.peer_id.as_ref().is_none_or(|wanted| wanted == &peer) {
                    crate::workspace::cancel_transfer(pool, &id).await?;
                }
            }
        }
        return Ok(());
    }
    if message.sender_id != db::get_user_id(pool).await? && message.sender_id != "me" { return Err("只能操作本机发送任务".into()); }
    if !matches!(message.msg_type.as_str(), "text" | "quote" | "announcement") { return Err("任务类型无效".into()); }
    if request.action == "retry" {
        sqlx::query("DELETE FROM task_hidden WHERE message_id=?").bind(message.id).execute(pool).await.map_err(|e| e.to_string())?;
    }
    let id = message.client_message_id.ok_or("任务缺少消息 ID")?;
    let receipts = db::get_message_receipts(pool, &id).await?;
    for receipt in receipts {
        if receipt.delivered_at.is_some() || receipt.read_at.is_some()
            || request.peer_id.as_ref().is_some_and(|peer| peer != &receipt.reader_id) { continue; }
        if request.action == "cancel" {
            // Revoking the lease also fences an already running writer/late timeout.
            sqlx::query("INSERT INTO message_delivery_attempts(message_client_id,reader_id,state)
                SELECT ?,?,'cancelled' WHERE EXISTS(SELECT 1 FROM message_receipts WHERE message_client_id=? AND reader_id=? AND delivered_at IS NULL AND read_at IS NULL)
                ON CONFLICT(message_client_id,reader_id) DO UPDATE SET state='cancelled',lease_token=NULL,lease_until=0,next_retry_at=0,last_error=NULL")
                .bind(&id).bind(&receipt.reader_id).bind(&id).bind(&receipt.reader_id)
                .execute(pool).await.map_err(|e| e.to_string())?;
        } else {
            // A cancelled task requires this explicit user action to become eligible again.
            sqlx::query("UPDATE message_delivery_attempts SET state='waiting_connection',next_retry_at=0 WHERE message_client_id=? AND reader_id=? AND state='cancelled'")
                .bind(&id).bind(&receipt.reader_id).execute(pool).await.map_err(|e| e.to_string())?;
            let pool = pool.clone(); let peers = peers.clone(); let id = id.clone();
            tokio::spawn(async move { let _ = crate::workspace::deliver_stored_message(&pool, &peers, &receipt.reader_id, &id, true).await; });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancellation_revokes_claim_and_survives_late_ack() {
        let root = std::env::temp_dir().join(format!("xchat-task-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(root.clone())).await.unwrap();
        let self_id = db::get_user_id(&pool).await.unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "task-peer").await.unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO messages(sender_id,receiver_id,content,msg_type,timestamp,status,conversation_id,client_message_id) VALUES(?,?,'task','text',1,'pending',?,?)")
            .bind(self_id).bind("task-peer").bind(&conversation.id).bind(&id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO message_receipts(message_client_id,reader_id,updated_at) VALUES(?,'task-peer',1)")
            .bind(&id).execute(&pool).await.unwrap();
        let message = db::get_message_by_client_id(&pool, &id).await.unwrap().unwrap();
        let attempt = db::claim_message_delivery(&pool, &id, "task-peer", true).await.unwrap().unwrap();
        act(&pool, &PeerManager::new(), TaskAction { message_id: message.id, action: "cancel".into(), peer_id: None }).await.unwrap();
        assert!(db::claim_message_delivery(&pool, &id, "task-peer", true).await.unwrap().is_none());
        assert!(!db::message_delivery_is_current(&pool, &attempt).await.unwrap());
        db::finish_message_delivery(&pool, &attempt, Some("late timeout")).await.unwrap();
        db::save_message_receipt(&pool, &id, "task-peer", Some(2), None).await.unwrap();
        let tasks = list(&pool, None).await.unwrap();
        assert_eq!(tasks.tasks[0].recipients[0].state, "cancelled");
        assert_eq!(tasks.tasks[0].recipients[0].delivered_at, Some(2));
        act(&pool, &PeerManager::new(), TaskAction { message_id: message.id, action: "dismiss".into(), peer_id: None }).await.unwrap();
        assert!(list(&pool, None).await.unwrap().tasks.is_empty());
        assert!(db::get_message_by_id(&pool, message.id).await.unwrap().is_some());
        pool.close().await;
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn replacement_requires_original_digest_and_preserves_message_identity() {
        let root = std::env::temp_dir().join(format!("xchat-source-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(root.clone())).await.unwrap();
        let downloads = root.join("downloads");
        db::update_download_path(&pool, downloads.to_string_lossy().into()).await.unwrap();
        let peers = PeerManager::new();
        let conversation = db::ensure_direct_conversation(&pool, "source-peer").await.unwrap();
        let original = root.join("original.bin");
        tokio::fs::write(&original, b"original content").await.unwrap();
        let queued = crate::network::conversation_file::send_path(&pool, &peers, &conversation.id, original.to_str().unwrap()).await.unwrap();
        let digest = crate::network::conversation_file::sha256_file(&original).await.unwrap();
        pin_digest(&pool, queued.message.id, original.to_str().unwrap(), &digest).await.unwrap();
        let picked = root.join("picked.bin");
        tokio::fs::write(&picked, b"original content").await.unwrap();
        assert!(replace_source(&pool, queued.message.id, picked.to_str().unwrap()).await.is_err(), "active task must not swap source");
        crate::workspace::cancel_transfer(&pool, &queued.transfers[0].id).await.unwrap();
        tokio::fs::write(&picked, b"modified content").await.unwrap();
        assert!(replace_source(&pool, queued.message.id, picked.to_str().unwrap()).await.is_err(), "same size is not same content");
        tokio::fs::write(&picked, b"original content").await.unwrap();
        tokio::fs::remove_file(&original).await.unwrap();
        replace_source(&pool, queued.message.id, picked.to_str().unwrap()).await.unwrap();
        let restored = db::get_message_by_id(&pool, queued.message.id).await.unwrap().unwrap();
        assert_eq!(restored.client_message_id, queued.message.client_message_id);
        assert_eq!(tokio::fs::read(restored.file_path.as_ref().unwrap()).await.unwrap(), b"original content");
        let retried = crate::network::conversation_file::retry_message(&pool, &peers, queued.message.id).await.unwrap();
        assert_eq!(retried.message.id, queued.message.id);
        assert_eq!(retried.transfers[0].status, "waiting_peer");
        let projected = list(&pool, None).await.unwrap();
        assert_eq!(projected.tasks[0].recipients.len(), 1, "past transfer attempts are not extra recipients");
        assert_eq!(projected.tasks[0].recipients[0].state, "waiting_peer");
        pool.close().await;
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
