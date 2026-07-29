use crate::{
    db::{self, ConversationMemberRecord, ConversationRecord, MessageRecord, TransferRecord},
    peers::PeerManager,
};
use serde::Serialize;
use sqlx::{Pool, Sqlite};
use std::{
    collections::hash_map::DefaultHasher,
    collections::{BTreeSet, HashMap},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncReadExt;

use super::{
    protocol::{GroupMember, ProtocolMessage},
    transfer::cancellation_registry,
};

const CHUNK_SIZE: usize = 4 * 1024 * 1024;
type ReceiveTransferLock = tokio::sync::Mutex<()>;
static RECEIVE_TRANSFER_LOCKS: OnceLock<Mutex<HashMap<String, Weak<ReceiveTransferLock>>>> =
    OnceLock::new();
static RESUME_TRANSFER_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub(crate) async fn lock_receive_file(key: &str) -> tokio::sync::OwnedMutexGuard<()> {
    let lock = {
        let locks = RECEIVE_TRANSFER_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut locks = locks.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(ReceiveTransferLock::new(()));
            locks.insert(key.to_string(), Arc::downgrade(&lock));
            lock
        }
    };
    lock.lock_owned().await
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationFileSendResult {
    pub message: MessageRecord,
    pub transfers: Vec<TransferRecord>,
}

#[derive(Debug, Clone)]
struct ValidatedSource {
    path: String,
    file_name: String,
    size: i64,
}

#[derive(Debug, Clone)]
struct UploadJob {
    transfer_id: String,
    peer_addr: String,
    conversation_id: String,
    client_message_id: String,
    message_id: i64,
    source: ValidatedSource,
    group_sync: Option<ProtocolMessage>,
}

enum UploadOutcome {
    Completed(i64),
    AwaitingAcceptance(i64),
    Cancelled(i64),
    Failed(i64, String),
}

pub async fn send_path(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    source_path: &str,
) -> Result<ConversationFileSendResult, String> {
    let conversation_id = conversation_id.trim();
    if conversation_id.is_empty() {
        return Err("conversation id is required".to_string());
    }

    let source = validate_source(source_path).await?;
    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    let members = db::get_conversation_members(pool, conversation_id).await?;
    let my_id = db::get_user_id(pool).await?;
    let recipient_ids = remote_recipient_ids(&conversation, &members, &my_id)?;
    let peers: HashMap<_, _> = peer_manager
        .get_all_peers()
        .into_iter()
        .map(|peer| (peer.id.clone(), peer))
        .collect();
    let online_addresses: HashMap<_, _> = recipient_ids
        .iter()
        .filter_map(|peer_id| {
            peers.get(peer_id).and_then(|peer| {
                (!peer.is_offline && !peer.addr.trim().is_empty())
                    .then(|| (peer_id.clone(), peer.addr.clone()))
            })
        })
        .collect();

    let client_message_id = uuid::Uuid::new_v4().to_string();
    let receiver_id = (conversation.kind == "direct")
        .then_some(conversation.peer_id.as_deref())
        .flatten();
    let message = db::save_conversation_message(
        pool,
        conversation_id,
        &my_id,
        receiver_id,
        &source.file_name,
        "file",
        unix_timestamp(),
        "sent",
        &client_message_id,
    )
    .await?;
    let file_status = if online_addresses.is_empty() {
        "waiting_peer"
    } else {
        "queued"
    };
    let message =
        db::set_file_message_metadata(pool, message.id, &source.path, source.size, file_status)
            .await?;
    db::ensure_message_recipients(pool, &client_message_id, &recipient_ids).await?;

    let group_sync = group_sync_message(&conversation, &members)?;
    let mut transfers = Vec::with_capacity(recipient_ids.len());
    let mut jobs = Vec::with_capacity(online_addresses.len());
    for peer_id in recipient_ids {
        let transfer_id = recipient_transfer_id(&client_message_id, &peer_id);
        let status = if online_addresses.contains_key(&peer_id) {
            "queued"
        } else {
            "waiting_peer"
        };
        let transfer = db::create_transfer(
            pool,
            &transfer_id,
            Some(message.id),
            conversation_id,
            &peer_id,
            "send",
            status,
            source.size,
        )
        .await?;

        if let Some(peer_addr) = online_addresses.get(&peer_id) {
            jobs.push(UploadJob {
                transfer_id,
                peer_addr: peer_addr.clone(),
                conversation_id: conversation_id.to_string(),
                client_message_id: client_message_id.clone(),
                message_id: message.id,
                source: source.clone(),
                group_sync: group_sync.clone(),
            });
        }
        transfers.push(transfer);
    }

    for job in jobs {
        spawn_upload(pool.clone(), job);
    }

    Ok(ConversationFileSendResult { message, transfers })
}

pub async fn resume_waiting_for_peer(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    peer_id: &str,
    peer_addr: &str,
) -> Result<(), String> {
    let peer_id = peer_id.trim();
    let peer_addr = peer_addr.trim();
    if peer_id.is_empty() || peer_addr.is_empty() {
        return Err("peer id and address are required".to_string());
    }
    if !peer_manager
        .get_active_peers()
        .iter()
        .any(|peer| peer.id == peer_id)
    {
        return Err("peer is not online".to_string());
    }

    let transfers = sqlx::query_as::<_, TransferRecord>(
        "SELECT id, message_id, conversation_id, peer_id, direction, status,
                bytes_total, bytes_transferred, error, created_at, updated_at
         FROM transfers
         WHERE peer_id = ? AND direction = 'send' AND status = 'waiting_peer'
         ORDER BY created_at ASC",
    )
    .bind(peer_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("查询待恢复文件传输失败: {error}"))?;

    for transfer in transfers {
        match prepare_resume_job(pool, &transfer, peer_addr).await {
            Ok(job) => {
                let claimed = db::update_transfer(
                    pool,
                    &transfer.id,
                    "queued",
                    transfer.bytes_transferred,
                    None,
                )
                .await?;
                if claimed.status == "queued" {
                    spawn_upload(pool.clone(), job);
                } else if let Some(message_id) = claimed.message_id {
                    refresh_file_status(pool, message_id).await?;
                }
            }
            Err(error) => {
                if let Some(message_id) = transfer.message_id {
                    let _ = update_terminal(
                        pool,
                        message_id,
                        &transfer.id,
                        "failed",
                        transfer.bytes_transferred,
                        Some(&error),
                    )
                    .await;
                } else {
                    let _ = db::update_transfer(
                        pool,
                        &transfer.id,
                        "failed",
                        transfer.bytes_transferred,
                        Some(&error),
                    )
                    .await;
                }
            }
        }
    }
    Ok(())
}

pub async fn resume_transfer(
    pool: &Pool<Sqlite>,
    message_id: i64,
    peer_id: &str,
    peer_addr: &str,
) -> Result<TransferRecord, String> {
    // ponytail: retries are rare; use one process lock until profiling justifies keyed locks.
    let _resume_guard = RESUME_TRANSFER_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let peer_id = peer_id.trim();
    let peer_addr = peer_addr.trim();
    if message_id <= 0 || peer_id.is_empty() || peer_addr.is_empty() {
        return Err("message, peer and address are required".to_string());
    }
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    if message.sender_id != db::get_user_id(pool).await?
        || message.client_message_id.is_none()
        || message.conversation_id.is_none()
    {
        return Err("only stable local file messages can be resumed".to_string());
    }
    let transfers = sqlx::query_as::<_, TransferRecord>(
        "SELECT id, message_id, conversation_id, peer_id, direction, status,
                bytes_total, bytes_transferred, error, created_at, updated_at
         FROM transfers
         WHERE message_id = ? AND peer_id = ? AND direction = 'send'
         ORDER BY updated_at DESC",
    )
    .bind(message_id)
    .bind(peer_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("查询可恢复文件传输失败: {error}"))?;
    if transfers.iter().any(|transfer| transfer.status == "completed") {
        return Err("file was already delivered to this peer".to_string());
    }
    if transfers.iter().any(|transfer| {
        matches!(
            transfer.status.as_str(),
            "queued" | "waiting_peer" | "offering" | "transferring" | "cancelling"
        )
    }) {
        return Err("file transfer is already active".to_string());
    }
    let previous = transfers
        .iter()
        .find(|transfer| transfer.status == "awaiting_acceptance")
        .or_else(|| transfers.first())
        .ok_or_else(|| "resumable file transfer not found".to_string())?;
    let mut job = prepare_resume_job(pool, previous, peer_addr).await?;
    let transfer = if previous.status == "awaiting_acceptance" {
        db::transition_transfer_status(
            pool,
            &previous.id,
            "awaiting_acceptance",
            "queued",
            previous.bytes_transferred,
            None,
        )
        .await?
        .ok_or_else(|| "file transfer is no longer resumable".to_string())?
    } else if previous.status == "failed" {
        let transfer_id = format!(
            "{}:retry:{}",
            recipient_transfer_id(&job.client_message_id, peer_id),
            uuid::Uuid::new_v4()
        );
        job.transfer_id = transfer_id.clone();
        db::create_transfer(
            pool,
            &transfer_id,
            Some(message_id),
            &previous.conversation_id,
            peer_id,
            "send",
            "queued",
            job.source.size,
        )
        .await?
    } else {
        return Err("file transfer is not resumable".to_string());
    };
    if transfer.status != "queued" {
        return Err("file transfer is no longer resumable".to_string());
    }
    spawn_upload(pool.clone(), job);
    refresh_file_status(pool, message_id).await?;
    Ok(transfer)
}

pub(crate) fn received_partial_path(download_root: &Path, transfer_id: &str) -> PathBuf {
    let mut first = DefaultHasher::new();
    (0u8, transfer_id).hash(&mut first);
    let mut second = DefaultHasher::new();
    (1u8, transfer_id).hash(&mut second);
    download_root.join(format!(
        ".xchat-{:016x}{:016x}.downloading",
        first.finish(),
        second.finish()
    ))
}

fn is_received_partial_name(name: &str) -> bool {
    name.strip_prefix(".xchat-")
        .and_then(|name| name.strip_suffix(".downloading"))
        .is_some_and(|hash| hash.len() == 32 && hash.chars().all(|ch| ch.is_ascii_hexdigit()))
}

pub(crate) async fn cleanup_stale_received_partials(
    download_root: &Path,
    max_age: Duration,
) -> Result<usize, String> {
    let mut entries = match tokio::fs::read_dir(download_root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("读取下载目录失败: {error}")),
    };
    let now = SystemTime::now();
    let mut removed = 0;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("读取下载临时文件失败: {error}"))?
    {
        if !entry
            .file_name()
            .to_str()
            .is_some_and(is_received_partial_name)
        {
            continue;
        }
        let metadata = entry
            .metadata()
            .await
            .map_err(|error| format!("读取下载临时文件信息失败: {error}"))?;
        let stale = metadata.is_file()
            && metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= max_age);
        if stale {
            tokio::fs::remove_file(entry.path())
                .await
                .map_err(|error| format!("清理过期下载临时文件失败: {error}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub async fn cancel_receive_transfer(
    pool: &Pool<Sqlite>,
    transfer_id: &str,
) -> Result<TransferRecord, String> {
    let initial = db::get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer not found".to_string())?;
    let message_id = initial
        .message_id
        .ok_or_else(|| "receive transfer has no file message".to_string())?;
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let lock_key = message
        .client_message_id
        .as_deref()
        .unwrap_or(transfer_id)
        .to_string();
    let _guard = lock_receive_file(&lock_key).await;
    let transfer = db::get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer not found".to_string())?;
    if transfer.direction != "receive" {
        return Err("only receive transfers can be cancelled here".to_string());
    }
    if transfer.status == "completed" {
        return Err("completed receive transfers cannot be cancelled".to_string());
    }
    let download_root = PathBuf::from(db::get_download_path(pool).await?);
    let partial_path = received_partial_path(&download_root, transfer_id);
    if let Err(error) = tokio::fs::remove_file(&partial_path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("清理接收临时文件失败: {error}"));
        }
    }
    let transfer = db::update_transfer(
        pool,
        transfer_id,
        "cancelled",
        transfer.bytes_transferred,
        None,
    )
    .await?;
    db::update_file_status_by_id(pool, message_id, "cancelled").await?;
    Ok(transfer)
}

pub async fn retry_message(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    message_id: i64,
) -> Result<ConversationFileSendResult, String> {
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let my_id = db::get_user_id(pool).await?;
    if message.msg_type != "file"
        || message.sender_id != my_id
        || !matches!(message.file_status.as_deref(), Some("failed" | "cancelled"))
    {
        return Err("only failed or cancelled local file messages can be retried".to_string());
    }
    let conversation_id = message
        .conversation_id
        .as_deref()
        .ok_or_else(|| "file message has no conversation".to_string())?;
    let client_message_id = message
        .client_message_id
        .as_deref()
        .ok_or_else(|| "file message has no stable id".to_string())?;
    let source_path = message
        .file_path
        .as_deref()
        .ok_or_else(|| "source file path is missing".to_string())?;
    let source = validate_source(source_path).await?;
    if message.file_size != Some(source.size) {
        return Err("source file size changed".to_string());
    }

    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    let members = db::get_conversation_members(pool, conversation_id).await?;
    let recipients = remote_recipient_ids(&conversation, &members, &my_id)?;
    let existing = sqlx::query_as::<_, TransferRecord>(
        "SELECT id, message_id, conversation_id, peer_id, direction, status,
                bytes_total, bytes_transferred, error, created_at, updated_at
         FROM transfers WHERE message_id = ? AND direction = 'send'",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("查询历史文件传输失败: {error}"))?;
    if existing.iter().any(|transfer| {
        matches!(
            transfer.status.as_str(),
            "queued"
                | "waiting_peer"
                | "offering"
                | "awaiting_acceptance"
                | "transferring"
                | "cancelling"
        )
    }) {
        return Err("file message already has an active transfer".to_string());
    }
    let completed: BTreeSet<_> = existing
        .iter()
        .filter(|transfer| transfer.status == "completed")
        .map(|transfer| transfer.peer_id.as_str())
        .collect();
    let retry_recipients: Vec<_> = recipients
        .into_iter()
        .filter(|peer_id| !completed.contains(peer_id.as_str()))
        .collect();
    if retry_recipients.is_empty() {
        return Err("file message has no failed recipients to retry".to_string());
    }

    let peers: HashMap<_, _> = peer_manager
        .get_all_peers()
        .into_iter()
        .map(|peer| (peer.id.clone(), peer))
        .collect();
    let group_sync = group_sync_message(&conversation, &members)?;
    let mut transfers = Vec::with_capacity(retry_recipients.len());
    let mut jobs = Vec::new();
    for peer_id in retry_recipients {
        let peer_addr = peers.get(&peer_id).and_then(|peer| {
            (!peer.is_offline && !peer.addr.trim().is_empty()).then(|| peer.addr.clone())
        });
        let status = if peer_addr.is_some() {
            "queued"
        } else {
            "waiting_peer"
        };
        let transfer_id = format!(
            "{}:retry:{}",
            recipient_transfer_id(client_message_id, &peer_id),
            uuid::Uuid::new_v4()
        );
        let transfer = db::create_transfer(
            pool,
            &transfer_id,
            Some(message_id),
            conversation_id,
            &peer_id,
            "send",
            status,
            source.size,
        )
        .await?;
        if let Some(peer_addr) = peer_addr {
            jobs.push(UploadJob {
                transfer_id,
                peer_addr,
                conversation_id: conversation_id.to_string(),
                client_message_id: client_message_id.to_string(),
                message_id,
                source: source.clone(),
                group_sync: group_sync.clone(),
            });
        }
        transfers.push(transfer);
    }
    refresh_file_status(pool, message_id).await?;
    for job in jobs {
        spawn_upload(pool.clone(), job);
    }
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    Ok(ConversationFileSendResult { message, transfers })
}

async fn prepare_resume_job(
    pool: &Pool<Sqlite>,
    transfer: &TransferRecord,
    peer_addr: &str,
) -> Result<UploadJob, String> {
    let message_id = transfer
        .message_id
        .ok_or_else(|| "transfer has no file message".to_string())?;
    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let conversation_id = message
        .conversation_id
        .as_deref()
        .ok_or_else(|| "file message has no conversation".to_string())?;
    let client_message_id = message
        .client_message_id
        .as_deref()
        .ok_or_else(|| "file message has no stable id".to_string())?;
    if conversation_id != transfer.conversation_id {
        return Err("transfer conversation does not match its message".to_string());
    }

    let conversation = db::get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    let members = db::get_conversation_members(pool, conversation_id).await?;
    let my_id = db::get_user_id(pool).await?;
    let recipients = remote_recipient_ids(&conversation, &members, &my_id)?;
    if !recipients.iter().any(|id| id == &transfer.peer_id) {
        return Err("transfer peer is not a conversation member".to_string());
    }

    let source_path = message
        .file_path
        .as_deref()
        .ok_or_else(|| "source file path is missing".to_string())?;
    let source = validate_source(source_path).await?;
    if source.size != transfer.bytes_total {
        return Err("source file size changed".to_string());
    }

    Ok(UploadJob {
        transfer_id: transfer.id.clone(),
        peer_addr: peer_addr.to_string(),
        conversation_id: conversation_id.to_string(),
        client_message_id: client_message_id.to_string(),
        message_id,
        source,
        group_sync: group_sync_message(&conversation, &members)?,
    })
}

fn spawn_upload(pool: Pool<Sqlite>, job: UploadJob) {
    tokio::spawn(async move {
        run_upload(&pool, job).await;
    });
}

async fn run_upload(pool: &Pool<Sqlite>, job: UploadJob) {
    let registry = cancellation_registry();
    let token = registry.register(job.transfer_id.clone());
    let current = match db::get_transfer(pool, &job.transfer_id).await {
        Ok(Some(transfer)) => transfer,
        Ok(None) => {
            registry.complete(&job.transfer_id);
            return;
        }
        Err(error) => {
            eprintln!("[ConversationFile] 读取传输失败: {error}");
            registry.complete(&job.transfer_id);
            return;
        }
    };

    if matches!(
        current.status.as_str(),
        "completed" | "cancelled" | "failed"
    ) {
        if let Err(error) = refresh_file_status(pool, job.message_id).await {
            eprintln!("[ConversationFile] 聚合文件状态失败: {error}");
        }
        registry.complete(&job.transfer_id);
        return;
    }
    if current.status == "cancelling" || token.load(Ordering::Acquire) {
        let _ = update_terminal(
            pool,
            job.message_id,
            &job.transfer_id,
            "cancelled",
            current.bytes_transferred,
            None,
        )
        .await;
        registry.complete(&job.transfer_id);
        return;
    }
    if !matches!(current.status.as_str(), "queued" | "waiting_peer") {
        registry.complete(&job.transfer_id);
        return;
    }

    let claimed = match db::update_transfer(
        pool,
        &job.transfer_id,
        "transferring",
        current.bytes_transferred,
        None,
    )
    .await
    {
        Ok(transfer) => transfer,
        Err(error) => {
            eprintln!("[ConversationFile] 启动传输失败: {error}");
            registry.complete(&job.transfer_id);
            return;
        }
    };
    if claimed.status != "transferring" || token.load(Ordering::Acquire) {
        let _ = update_terminal(
            pool,
            job.message_id,
            &job.transfer_id,
            "cancelled",
            claimed.bytes_transferred,
            None,
        )
        .await;
        registry.complete(&job.transfer_id);
        return;
    }
    if let Err(error) = refresh_file_status(pool, job.message_id).await {
        eprintln!("[ConversationFile] 聚合文件状态失败: {error}");
    }

    let outcome = upload_chunks(pool, &job, &token).await;
    let (status, bytes, error) = match outcome {
        UploadOutcome::Completed(bytes) => ("completed", bytes, None),
        UploadOutcome::AwaitingAcceptance(bytes) => ("awaiting_acceptance", bytes, None),
        UploadOutcome::Cancelled(bytes) => {
            if notify_remote_cleanup(&job, "cancelled").await.as_deref() == Some("completed") {
                ("completed", job.source.size, None)
            } else {
                ("cancelled", bytes, None)
            }
        }
        UploadOutcome::Failed(bytes, error) => {
            if notify_remote_cleanup(&job, "failed").await.as_deref() == Some("completed") {
                ("completed", job.source.size, None)
            } else {
                ("failed", bytes, Some(error))
            }
        }
    };
    if let Err(update_error) = update_terminal(
        pool,
        job.message_id,
        &job.transfer_id,
        status,
        bytes,
        error.as_deref(),
    )
    .await
    {
        eprintln!("[ConversationFile] 保存传输终态失败: {update_error}");
    }
    registry.complete(&job.transfer_id);
}

async fn post_remote_terminal(
    peer_addr: &str,
    client_message_id: &str,
    transfer_id: &str,
    status: &str,
    peer_id: Option<&str>,
) -> Option<String> {
    let client_message_id = urlencoding::encode(client_message_id);
    let transfer_id = urlencoding::encode(transfer_id);
    let mut url = format!(
        "http://{}/api/uploads/{}/cancel?status={}&transfer_id={}",
        peer_addr.trim_end_matches('/'),
        client_message_id,
        status,
        transfer_id
    );
    if let Some(peer_id) = peer_id {
        url.push_str("&peer_id=");
        url.push_str(&urlencoding::encode(peer_id));
    }
    let response = match reqwest::Client::new()
        .post(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("[ConversationFile] 接收端取消清理失败: {error}");
            return None;
        }
    };
    if !response.status().is_success() {
        eprintln!(
            "[ConversationFile] 接收端取消清理被拒绝: {}",
            response.status()
        );
        return None;
    }
    response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|body| body.get("status").and_then(|status| status.as_str()).map(str::to_owned))
}

async fn notify_remote_cleanup(job: &UploadJob, status: &str) -> Option<String> {
    post_remote_terminal(
        &job.peer_addr,
        &job.client_message_id,
        &job.transfer_id,
        status,
        None,
    )
    .await
}

pub(crate) async fn notify_peer_terminal(
    pool: &Pool<Sqlite>,
    transfer: &TransferRecord,
    status: &str,
) -> Option<String> {
    let Some(message_id) = transfer.message_id else {
        return None;
    };
    let Ok(Some(message)) = db::get_file_message_by_id(pool, message_id).await else {
        return None;
    };
    let Some(client_message_id) = message.client_message_id.as_deref() else {
        return None;
    };
    let Ok(Some(peer)) = db::get_user_metadata(pool, &transfer.peer_id).await else {
        return None;
    };
    let receiver_id = if transfer.direction == "receive" {
        db::get_user_id(pool).await.ok()
    } else {
        None
    };
    post_remote_terminal(
        &peer.addr,
        client_message_id,
        &transfer.id,
        status,
        receiver_id.as_deref(),
    )
    .await
}

async fn upload_chunks(
    pool: &Pool<Sqlite>,
    job: &UploadJob,
    token: &super::transfer::TransferCancellationToken,
) -> UploadOutcome {
    if token.load(Ordering::Acquire) {
        return UploadOutcome::Cancelled(0);
    }

    let mut file = match tokio::fs::File::open(&job.source.path).await {
        Ok(file) => file,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("打开源文件失败: {error}"));
        }
    };
    if let Some(group_sync) = &job.group_sync {
        if let Err(error) = super::protocol::send_protocol_message(&job.peer_addr, group_sync).await
        {
            return UploadOutcome::Failed(0, format!("发送群同步失败: {error}"));
        }
    }
    if token.load(Ordering::Acquire) {
        return UploadOutcome::Cancelled(0);
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("创建上传客户端失败: {error}"));
        }
    };
    let upload_url = format!("http://{}/api/upload", job.peer_addr.trim_end_matches('/'));
    let total_chunks = chunk_total(job.source.size);
    let my_id = match db::get_user_id(pool).await {
        Ok(id) => id,
        Err(error) => return UploadOutcome::Failed(0, error),
    };
    let started = Instant::now();
    let mut bytes_transferred = 0i64;

    for chunk_index in 0..total_chunks {
        if token.load(Ordering::Acquire) {
            return UploadOutcome::Cancelled(bytes_transferred);
        }

        let mut chunk = vec![0; CHUNK_SIZE];
        let mut bytes_read = 0usize;
        while bytes_read < CHUNK_SIZE {
            match file.read(&mut chunk[bytes_read..]).await {
                Ok(0) => break,
                Ok(read) => bytes_read += read,
                Err(error) => {
                    return UploadOutcome::Failed(
                        bytes_transferred,
                        format!("读取源文件失败: {error}"),
                    );
                }
            }
        }
        chunk.truncate(bytes_read);
        if bytes_read == 0 && job.source.size > 0 {
            return UploadOutcome::Failed(
                bytes_transferred,
                "源文件在传输过程中被截断".to_string(),
            );
        }

        let speed_mb_s = if started.elapsed().as_secs_f64() > 0.0 {
            bytes_transferred as f64 / (1024.0 * 1024.0) / started.elapsed().as_secs_f64()
        } else {
            0.0
        };
        let part = match reqwest::multipart::Part::bytes(chunk).mime_str("application/octet-stream")
        {
            Ok(part) => part,
            Err(error) => {
                return UploadOutcome::Failed(
                    bytes_transferred,
                    format!("创建上传分块失败: {error}"),
                );
            }
        };
        let mut form = reqwest::multipart::Form::new()
            .text("peer_id", my_id.clone())
            .text("file_name", job.source.file_name.clone())
            .text("file_size", job.source.size.to_string())
            .text("chunk_index", chunk_index.to_string())
            .text("chunk_total", total_chunks.to_string())
            .text("sender_msg_id", job.message_id.to_string())
            .text("speed_mb_s", format!("{speed_mb_s:.1}"))
            .text("conversation_id", job.conversation_id.clone())
            .text("client_message_id", job.client_message_id.clone())
            .text("transfer_id", job.transfer_id.clone())
            .part("chunk", part);
        if chunk_index == 0 {
            if let Some(group_sync) = &job.group_sync {
                let group_sync = match serde_json::to_string(group_sync) {
                    Ok(group_sync) => group_sync,
                    Err(error) => {
                        return UploadOutcome::Failed(
                            bytes_transferred,
                            format!("序列化群同步失败: {error}"),
                        );
                    }
                };
                form = form.text("group_sync", group_sync);
            }
        }

        let response = match client.post(&upload_url).multipart(form).send().await {
            Ok(response) => response,
            Err(error) => {
                if token.load(Ordering::Acquire) {
                    return UploadOutcome::Cancelled(bytes_transferred);
                }
                return UploadOutcome::Failed(bytes_transferred, format!("上传分块失败: {error}"));
            }
        };
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            if token.load(Ordering::Acquire) {
                return UploadOutcome::Cancelled(bytes_transferred);
            }
            let detail: String = body.chars().take(512).collect();
            return UploadOutcome::Failed(
                bytes_transferred,
                format!("接收端拒绝分块 ({status}): {detail}"),
            );
        }

        let response_status = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("status")
                    .and_then(|status| status.as_str())
                    .map(str::to_owned)
            });
        if response_status.as_deref() == Some("awaiting_acceptance") {
            return UploadOutcome::AwaitingAcceptance(bytes_transferred);
        }
        if response_status.as_deref() == Some("already_exists") {
            return UploadOutcome::Completed(job.source.size);
        }
        bytes_transferred += bytes_read as i64;
        if chunk_index + 1 == total_chunks {
            return UploadOutcome::Completed(bytes_transferred);
        }
        if token.load(Ordering::Acquire) {
            return UploadOutcome::Cancelled(bytes_transferred);
        }
        if let Err(error) = db::update_transfer(
            pool,
            &job.transfer_id,
            "transferring",
            bytes_transferred,
            None,
        )
        .await
        {
            return UploadOutcome::Failed(bytes_transferred, error);
        }
        if token.load(Ordering::Acquire) {
            return UploadOutcome::Cancelled(bytes_transferred);
        }
    }

    if token.load(Ordering::Acquire) {
        UploadOutcome::Cancelled(bytes_transferred)
    } else {
        UploadOutcome::Completed(bytes_transferred)
    }
}

async fn update_terminal(
    pool: &Pool<Sqlite>,
    message_id: i64,
    transfer_id: &str,
    status: &str,
    bytes_transferred: i64,
    error: Option<&str>,
) -> Result<(), String> {
    db::update_transfer(pool, transfer_id, status, bytes_transferred, error).await?;
    refresh_file_status(pool, message_id).await
}

pub(crate) async fn refresh_file_status(
    pool: &Pool<Sqlite>,
    message_id: i64,
) -> Result<(), String> {
    let statuses = sqlx::query_scalar::<_, String>(
        "SELECT transfer.status
         FROM transfers transfer
         WHERE transfer.message_id = ? AND transfer.direction = 'send'
           AND transfer.rowid = (
               SELECT MAX(latest.rowid)
               FROM transfers latest
               WHERE latest.message_id = transfer.message_id
                 AND latest.direction = transfer.direction
                 AND latest.peer_id = transfer.peer_id
           )",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("查询文件传输状态失败: {error}"))?;
    let Some(status) = aggregate_file_status(&statuses) else {
        return Ok(());
    };

    let message = db::get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let (Some(path), Some(size)) = (message.file_path.as_deref(), message.file_size) else {
        return Err("file message metadata is incomplete".to_string());
    };
    db::set_file_message_metadata(pool, message_id, path, size, status).await?;
    if status == "completed" {
        cleanup_managed_temp_source(Path::new(path));
    }
    Ok(())
}

fn cleanup_managed_temp_source(path: &Path) {
    let Ok(canonical_path) = path.canonicalize() else {
        return;
    };
    for directory in ["xchat-captures", "xchat-web-staging"] {
        let Ok(root) = std::env::temp_dir().join(directory).canonicalize() else {
            continue;
        };
        if canonical_path == root || !canonical_path.starts_with(&root) {
            continue;
        }
        let parent = canonical_path.parent().map(Path::to_path_buf);
        let _ = std::fs::remove_file(&canonical_path);
        if let Some(parent) = parent.filter(|parent| *parent != root && parent.starts_with(&root)) {
            let _ = std::fs::remove_dir(parent);
        }
        break;
    }
}

async fn validate_source(source_path: &str) -> Result<ValidatedSource, String> {
    let source_path = source_path.trim();
    if source_path.is_empty() {
        return Err("source path is required".to_string());
    }
    let canonical: PathBuf = tokio::fs::canonicalize(source_path)
        .await
        .map_err(|error| format!("源文件不存在或不可访问: {error}"))?;
    let metadata = tokio::fs::metadata(&canonical)
        .await
        .map_err(|error| format!("读取源文件信息失败: {error}"))?;
    if !metadata.is_file() {
        return Err("source path must be a regular file".to_string());
    }
    let size = i64::try_from(metadata.len()).map_err(|_| "source file is too large".to_string())?;
    let path = canonical
        .to_str()
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "source path is not valid UTF-8".to_string())?
        .to_string();
    let file_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "source file name is invalid".to_string())?
        .to_string();
    Ok(ValidatedSource {
        path,
        file_name,
        size,
    })
}

fn remote_recipient_ids(
    conversation: &ConversationRecord,
    members: &[ConversationMemberRecord],
    my_id: &str,
) -> Result<Vec<String>, String> {
    if !members.iter().any(|member| member.peer_id == my_id) {
        return Err("local user is not a conversation member".to_string());
    }

    let member_ids: BTreeSet<_> = members
        .iter()
        .map(|member| member.peer_id.as_str())
        .collect();
    let recipients = match conversation.kind.as_str() {
        "direct" => {
            let peer_id = conversation
                .peer_id
                .as_deref()
                .filter(|peer_id| !peer_id.is_empty() && *peer_id != my_id)
                .ok_or_else(|| "direct conversation has no remote peer".to_string())?;
            if !member_ids.contains(peer_id) {
                return Err("direct conversation peer is not a member".to_string());
            }
            vec![peer_id.to_string()]
        }
        "group" => member_ids
            .into_iter()
            .filter(|peer_id| *peer_id != my_id)
            .map(str::to_string)
            .collect(),
        _ => return Err("unsupported conversation kind".to_string()),
    };
    if recipients.is_empty() {
        return Err("conversation has no remote recipients".to_string());
    }
    Ok(recipients)
}

fn group_sync_message(
    conversation: &ConversationRecord,
    members: &[ConversationMemberRecord],
) -> Result<Option<ProtocolMessage>, String> {
    if conversation.kind != "group" {
        return Ok(None);
    }
    let title = conversation
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| "group title is missing".to_string())?;
    let created_by = conversation
        .created_by
        .as_deref()
        .filter(|created_by| !created_by.trim().is_empty())
        .ok_or_else(|| "group creator is missing".to_string())?;
    let version =
        u64::try_from(conversation.version).map_err(|_| "group version is invalid".to_string())?;
    if version == 0 {
        return Err("group version is invalid".to_string());
    }
    Ok(Some(ProtocolMessage::GroupSync {
        group_id: conversation.id.clone(),
        title: title.to_string(),
        created_by: created_by.to_string(),
        members: members
            .iter()
            .map(|member| GroupMember {
                peer_id: member.peer_id.clone(),
                display_name: member.display_name.clone(),
                role: member.role.clone(),
            })
            .collect(),
        version,
        timestamp: unix_timestamp() as u64,
    }))
}

pub(crate) fn recipient_transfer_id(client_message_id: &str, peer_id: &str) -> String {
    format!("{client_message_id}:{peer_id}")
}

fn chunk_total(file_size: i64) -> u64 {
    ((file_size as u64 + CHUNK_SIZE as u64 - 1) / CHUNK_SIZE as u64).max(1)
}

fn aggregate_file_status(statuses: &[String]) -> Option<&'static str> {
    if statuses.is_empty() {
        None
    } else if statuses.iter().all(|status| status == "completed") {
        Some("completed")
    } else if statuses.iter().any(|status| {
        matches!(
            status.as_str(),
            "queued" | "offering" | "awaiting_acceptance" | "transferring" | "cancelling"
        )
    }) {
        Some("transferring")
    } else if statuses.iter().any(|status| status == "waiting_peer") {
        Some("waiting_peer")
    } else if statuses.iter().all(|status| status == "cancelled") {
        Some("cancelled")
    } else {
        Some("failed")
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_partial_cleanup_only_targets_managed_names() {
        assert!(is_received_partial_name(
            ".xchat-0123456789abcdef0123456789abcdef.downloading"
        ));
        assert!(!is_received_partial_name("../../report.downloading"));
        assert!(!is_received_partial_name(".xchat-not-managed.downloading"));
    }

    #[test]
    fn transfer_identity_is_stable_per_recipient_and_empty_files_have_one_chunk() {
        assert_eq!(
            recipient_transfer_id("message-1", "peer-1"),
            "message-1:peer-1"
        );
        assert_ne!(
            recipient_transfer_id("message-1", "peer-1"),
            recipient_transfer_id("message-1", "peer-2")
        );
        assert_eq!(chunk_total(0), 1);
        assert_eq!(chunk_total(CHUNK_SIZE as i64), 1);
        assert_eq!(chunk_total(CHUNK_SIZE as i64 + 1), 2);
        assert_eq!(
            aggregate_file_status(&["completed".into(), "completed".into()]),
            Some("completed")
        );
        assert_eq!(
            aggregate_file_status(&["completed".into(), "queued".into()]),
            Some("transferring")
        );
        assert_eq!(
            aggregate_file_status(&["failed".into(), "waiting_peer".into()]),
            Some("waiting_peer")
        );
        assert_eq!(
            aggregate_file_status(&["cancelled".into(), "cancelled".into()]),
            Some("cancelled")
        );
        assert_eq!(
            aggregate_file_status(&["completed".into(), "failed".into()]),
            Some("failed")
        );
    }

    #[test]
    fn managed_transfer_sources_are_cleaned_without_touching_other_temp_files() {
        let managed_root = std::env::temp_dir().join("xchat-web-staging");
        let managed_dir = managed_root.join(format!("test-{}", uuid::Uuid::new_v4()));
        let managed_file = managed_dir.join("capture.png");
        std::fs::create_dir_all(&managed_dir).unwrap();
        std::fs::write(&managed_file, b"xchat").unwrap();

        cleanup_managed_temp_source(&managed_file);
        assert!(!managed_file.exists());
        assert!(!managed_dir.exists());

        let unrelated =
            std::env::temp_dir().join(format!("xchat-unmanaged-{}", uuid::Uuid::new_v4()));
        std::fs::write(&unrelated, b"keep").unwrap();
        cleanup_managed_temp_source(&unrelated);
        assert!(unrelated.exists());
        std::fs::remove_file(unrelated).unwrap();
    }

    #[tokio::test]
    async fn retry_preserves_logical_message_and_receive_cancel_cleans_managed_partial() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-retry-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_dir = app_dir.join("downloads");
        db::update_download_path(&pool, download_dir.to_string_lossy().into_owned())
            .await
            .unwrap();
        db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let managed_source_dir = std::env::temp_dir()
            .join("xchat-web-staging")
            .join(uuid::Uuid::new_v4().to_string());
        tokio::fs::create_dir_all(&managed_source_dir)
            .await
            .unwrap();
        let source = managed_source_dir.join("retry.bin");
        tokio::fs::write(&source, b"retry").await.unwrap();
        let peer_manager = PeerManager::new();
        let first = send_path(
            &pool,
            &peer_manager,
            &conversation.id,
            source.to_str().unwrap(),
        )
        .await
        .unwrap();
        let original_client_id = first.message.client_message_id.clone();
        db::update_transfer(
            &pool,
            &first.transfers[0].id,
            "failed",
            0,
            Some("test failure"),
        )
        .await
        .unwrap();
        refresh_file_status(&pool, first.message.id).await.unwrap();
        assert!(source.exists(), "failed managed sources must remain retryable");

        let retried = retry_message(&pool, &peer_manager, first.message.id)
            .await
            .unwrap();
        assert_eq!(retried.message.id, first.message.id);
        assert_eq!(retried.message.client_message_id, original_client_id);
        assert_ne!(retried.transfers[0].id, first.transfers[0].id);
        assert_eq!(retried.transfers[0].status, "waiting_peer");
        db::update_transfer(
            &pool,
            &retried.transfers[0].id,
            "completed",
            5,
            None,
        )
        .await
        .unwrap();
        refresh_file_status(&pool, first.message.id).await.unwrap();
        assert_eq!(
            db::get_file_message_by_id(&pool, first.message.id)
                .await
                .unwrap()
                .unwrap()
                .file_status
                .as_deref(),
            Some("completed"),
            "a completed retry must supersede the old failed attempt"
        );

        let resume_source = app_dir.join("resume.bin");
        tokio::fs::write(&resume_source, b"resume").await.unwrap();
        let awaiting = send_path(
            &pool,
            &peer_manager,
            &conversation.id,
            resume_source.to_str().unwrap(),
        )
        .await
        .unwrap();
        db::update_transfer(
            &pool,
            &awaiting.transfers[0].id,
            "failed",
            0,
            Some("test failure"),
        )
        .await
        .unwrap();
        refresh_file_status(&pool, awaiting.message.id)
            .await
            .unwrap();
        let resumed = resume_transfer(
            &pool,
            awaiting.message.id,
            "peer-a",
            "127.0.0.1:1",
        )
        .await
        .unwrap();
        assert_ne!(resumed.id, awaiting.transfers[0].id);
        assert_eq!(resumed.message_id, Some(awaiting.message.id));
        assert_eq!(resumed.status, "queued");
        tokio::time::sleep(Duration::from_millis(25)).await;

        let self_id = db::get_user_id(&pool).await.unwrap();
        let incoming = db::save_conversation_message(
            &pool,
            &conversation.id,
            "peer-a",
            Some(&self_id),
            "incoming.bin",
            "file",
            unix_timestamp(),
            "received",
            "incoming-client",
        )
        .await
        .unwrap();
        let incoming_path = download_dir.join("incoming.bin");
        db::set_file_message_metadata(
            &pool,
            incoming.id,
            incoming_path.to_str().unwrap(),
            4,
            "downloading",
        )
        .await
        .unwrap();
        let incoming_transfer_id = recipient_transfer_id("incoming-client", &self_id);
        db::create_transfer(
            &pool,
            &incoming_transfer_id,
            Some(incoming.id),
            &conversation.id,
            "peer-a",
            "receive",
            "transferring",
            4,
        )
        .await
        .unwrap();
        let partial_path = received_partial_path(&download_dir, &incoming_transfer_id);
        tokio::fs::create_dir_all(&download_dir).await.unwrap();
        tokio::fs::write(&partial_path, b"part").await.unwrap();
        let cancelled = cancel_receive_transfer(&pool, &incoming_transfer_id)
            .await
            .unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert!(!partial_path.exists());
        assert_eq!(
            db::get_file_message_by_id(&pool, incoming.id)
                .await
                .unwrap()
                .unwrap()
                .file_status
                .as_deref(),
            Some("cancelled")
        );

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
        if managed_source_dir.exists() {
            std::fs::remove_dir_all(managed_source_dir).unwrap();
        }
    }
}
