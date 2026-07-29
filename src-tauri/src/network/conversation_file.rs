use crate::{
    db::{self, ConversationMemberRecord, ConversationRecord, MessageRecord, TransferRecord},
    peers::PeerManager,
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Pool, Sqlite};
use std::{
    collections::hash_map::DefaultHasher,
    collections::{BTreeSet, HashMap},
    hash::{Hash, Hasher},
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use super::{
    protocol::{GroupMember, ProtocolMessage},
    transfer::cancellation_registry,
};

const CHUNK_SIZE: usize = 4 * 1024 * 1024;
const PARALLEL_STREAM_BUFFER: usize = 256 * 1024;
pub const PARALLEL_FILE_CAPABILITY: &str = "parallel_file_v2";
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
    parallel_v2: bool,
    file_sha256: Option<String>,
}

enum UploadOutcome {
    Completed(i64),
    AwaitingAcceptance(i64),
    Cancelled(i64),
    Failed(i64, String),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ParallelChunkRange {
    pub index: usize,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ParallelPrepareRequest {
    pub sender_id: String,
    pub conversation_id: String,
    pub client_message_id: String,
    pub transfer_id: String,
    pub sender_msg_id: String,
    pub file_name: String,
    pub file_size: u64,
    pub file_sha256: String,
    pub chunks: Vec<ParallelChunkRange>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ParallelTransferManifest {
    pub version: u8,
    pub sender_id: String,
    pub conversation_id: String,
    pub client_message_id: String,
    pub transfer_id: String,
    pub sender_msg_id: String,
    pub file_name: String,
    pub final_file_name: String,
    pub file_size: u64,
    pub file_sha256: String,
    pub chunks: Vec<ParallelChunkRange>,
    pub message_id: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct ParallelChunkReceiveResult {
    pub manifest: ParallelTransferManifest,
    pub received: u64,
    pub complete: bool,
}

#[derive(Debug, Deserialize)]
struct ParallelPrepareResponse {
    status: String,
    #[serde(default)]
    missing_chunks: Vec<usize>,
    #[serde(default)]
    received: u64,
}

fn supports_parallel_file(capabilities: &[String]) -> bool {
    capabilities
        .iter()
        .any(|capability| capability == PARALLEL_FILE_CAPABILITY)
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
    let parallel_hash = if recipient_ids.iter().any(|peer_id| {
        online_addresses.contains_key(peer_id)
            && peers
                .get(peer_id)
                .is_some_and(|peer| supports_parallel_file(&peer.capabilities))
    }) {
        Some(sha256_file(Path::new(&source.path)).await?)
    } else {
        None
    };

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
            let parallel_v2 = peers
                .get(&peer_id)
                .is_some_and(|peer| supports_parallel_file(&peer.capabilities));
            jobs.push(UploadJob {
                transfer_id,
                peer_addr: peer_addr.clone(),
                conversation_id: conversation_id.to_string(),
                client_message_id: client_message_id.clone(),
                message_id: message.id,
                source: source.clone(),
                group_sync: group_sync.clone(),
                parallel_v2,
                file_sha256: parallel_v2.then(|| parallel_hash.clone()).flatten(),
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
    let parallel_v2 = peer_manager
        .get_active_peers()
        .iter()
        .find(|peer| peer.id == peer_id)
        .is_some_and(|peer| supports_parallel_file(&peer.capabilities));

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
        match prepare_resume_job(pool, &transfer, peer_addr, parallel_v2).await {
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
    parallel_v2: bool,
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
    let mut job = prepare_resume_job(pool, previous, peer_addr, parallel_v2).await?;
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
    } else if previous.status == "failed" && parallel_v2 {
        reset_send_transfer_for_retry(pool, &previous.id, "queued").await?
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
    cleanup_parallel_transfer(&download_root, transfer_id).await?;
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
    let parallel_hash = if retry_recipients.iter().any(|peer_id| {
        peers.get(peer_id).is_some_and(|peer| {
            !peer.is_offline
                && !peer.addr.trim().is_empty()
                && supports_parallel_file(&peer.capabilities)
        })
    }) {
        Some(sha256_file(Path::new(&source.path)).await?)
    } else {
        None
    };
    let group_sync = group_sync_message(&conversation, &members)?;
    let mut transfers = Vec::with_capacity(retry_recipients.len());
    let mut jobs = Vec::new();
    for peer_id in retry_recipients {
        let parallel_v2 = peers
            .get(&peer_id)
            .is_some_and(|peer| supports_parallel_file(&peer.capabilities));
        let peer_addr = peers.get(&peer_id).and_then(|peer| {
            (!peer.is_offline && !peer.addr.trim().is_empty()).then(|| peer.addr.clone())
        });
        let status = if peer_addr.is_some() {
            "queued"
        } else {
            "waiting_peer"
        };
        let reusable = parallel_v2.then(|| {
            existing
                .iter()
                .filter(|transfer| transfer.peer_id == peer_id && transfer.status == "failed")
                .max_by_key(|transfer| transfer.updated_at)
        }).flatten();
        let (transfer_id, transfer) = if let Some(previous) = reusable {
            (
                previous.id.clone(),
                reset_send_transfer_for_retry(pool, &previous.id, status).await?,
            )
        } else {
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
            (transfer_id, transfer)
        };
        if let Some(peer_addr) = peer_addr {
            jobs.push(UploadJob {
                transfer_id,
                peer_addr,
                conversation_id: conversation_id.to_string(),
                client_message_id: client_message_id.to_string(),
                message_id,
                source: source.clone(),
                group_sync: group_sync.clone(),
                parallel_v2,
                file_sha256: parallel_v2.then(|| parallel_hash.clone()).flatten(),
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

async fn reset_send_transfer_for_retry(
    pool: &Pool<Sqlite>,
    transfer_id: &str,
    status: &str,
) -> Result<TransferRecord, String> {
    sqlx::query(
        "UPDATE transfers
         SET status = ?, bytes_transferred = 0, error = NULL, updated_at = ?
         WHERE id = ? AND direction = 'send' AND status = 'failed'",
    )
    .bind(status)
    .bind(unix_timestamp())
    .bind(transfer_id)
    .execute(pool)
    .await
    .map_err(|error| format!("重置并行重试传输失败: {error}"))?;
    db::get_transfer(pool, transfer_id)
        .await?
        .filter(|transfer| transfer.status == status)
        .ok_or_else(|| "并行传输已无法重试".to_string())
}

async fn prepare_resume_job(
    pool: &Pool<Sqlite>,
    transfer: &TransferRecord,
    peer_addr: &str,
    parallel_v2: bool,
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
    let file_sha256 = if parallel_v2 {
        Some(sha256_file(Path::new(&source.path)).await?)
    } else {
        None
    };

    Ok(UploadJob {
        transfer_id: transfer.id.clone(),
        peer_addr: peer_addr.to_string(),
        conversation_id: conversation_id.to_string(),
        client_message_id: client_message_id.to_string(),
        message_id,
        source,
        group_sync: group_sync_message(&conversation, &members)?,
        parallel_v2,
        file_sha256,
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

    if let Some(group_sync) = &job.group_sync {
        if let Err(error) = super::protocol::send_protocol_message(&job.peer_addr, group_sync).await
        {
            return UploadOutcome::Failed(0, format!("发送群同步失败: {error}"));
        }
    }
    if token.load(Ordering::Acquire) {
        return UploadOutcome::Cancelled(0);
    }
    if job.parallel_v2 {
        return upload_parallel_chunks(pool, job, token).await;
    }

    let mut file = match tokio::fs::File::open(&job.source.path).await {
        Ok(file) => file,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("打开源文件失败: {error}"));
        }
    };
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

async fn upload_parallel_chunks(
    pool: &Pool<Sqlite>,
    job: &UploadJob,
    token: &super::transfer::TransferCancellationToken,
) -> UploadOutcome {
    let Some(file_sha256) = job.file_sha256.clone() else {
        return UploadOutcome::Failed(0, "并行传输缺少文件摘要".to_string());
    };
    let file_size = job.source.size.max(0) as u64;
    let chunks = parallel_chunk_ranges(file_size);
    let sender_id = match db::get_user_id(pool).await {
        Ok(id) => id,
        Err(error) => return UploadOutcome::Failed(0, error),
    };
    let request = ParallelPrepareRequest {
        sender_id,
        conversation_id: job.conversation_id.clone(),
        client_message_id: job.client_message_id.clone(),
        transfer_id: job.transfer_id.clone(),
        sender_msg_id: job.message_id.to_string(),
        file_name: job.source.file_name.clone(),
        file_size,
        file_sha256,
        chunks: chunks.clone(),
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 60))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("创建并行上传客户端失败: {error}"));
        }
    };
    let base_url = format!(
        "http://{}",
        job.peer_addr.trim_end_matches('/')
    );
    let response = match client
        .post(format!("{base_url}/api/uploads/v2/prepare"))
        .json(&request)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("准备并行传输失败: {error}"));
        }
    };
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail: String = body.chars().take(512).collect();
        return UploadOutcome::Failed(0, format!("接收端拒绝并行传输 ({status}): {detail}"));
    }
    let prepared: ParallelPrepareResponse = match serde_json::from_str(&body) {
        Ok(response) => response,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("解析并行传输响应失败: {error}"));
        }
    };
    match prepared.status.as_str() {
        "awaiting_acceptance" => return UploadOutcome::AwaitingAcceptance(prepared.received as i64),
        "already_exists" | "completed" => return UploadOutcome::Completed(job.source.size),
        "ready" => {}
        status => {
            return UploadOutcome::Failed(
                prepared.received as i64,
                format!("接收端返回未知并行传输状态: {status}"),
            );
        }
    }

    let missing: Vec<_> = chunks
        .into_iter()
        .filter(|chunk| prepared.missing_chunks.contains(&chunk.index))
        .collect();
    if missing.len() != prepared.missing_chunks.len() {
        return UploadOutcome::Failed(
            prepared.received as i64,
            "接收端返回了无效的缺失分块".to_string(),
        );
    }
    if missing.is_empty() {
        return UploadOutcome::Completed(job.source.size);
    }

    let progress = Arc::new(AtomicI64::new(prepared.received as i64));
    let mut uploads = FuturesUnordered::new();
    for chunk in missing {
        uploads.push(upload_parallel_range(
            client.clone(),
            base_url.clone(),
            job.transfer_id.clone(),
            job.source.path.clone(),
            chunk,
            progress.clone(),
        ));
    }
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    while !uploads.is_empty() {
        tokio::select! {
            _ = interval.tick() => {
                let bytes = progress.load(Ordering::Acquire).min(job.source.size);
                if token.load(Ordering::Acquire) {
                    return UploadOutcome::Cancelled(bytes);
                }
                if let Err(error) = db::update_transfer(
                    pool,
                    &job.transfer_id,
                    "transferring",
                    bytes,
                    None,
                ).await {
                    return UploadOutcome::Failed(bytes, error);
                }
            }
            result = uploads.next() => {
                let Some(result) = result else { break };
                if let Err(error) = result {
                    let bytes = progress.load(Ordering::Acquire).min(job.source.size);
                    if token.load(Ordering::Acquire) {
                        return UploadOutcome::Cancelled(bytes);
                    }
                    return UploadOutcome::Failed(bytes, error);
                }
            }
        }
    }

    let bytes = progress.load(Ordering::Acquire).min(job.source.size);
    if token.load(Ordering::Acquire) {
        UploadOutcome::Cancelled(bytes)
    } else if bytes < job.source.size {
        UploadOutcome::Failed(bytes, "并行上传未覆盖完整文件".to_string())
    } else {
        UploadOutcome::Completed(job.source.size)
    }
}

async fn upload_parallel_range(
    client: reqwest::Client,
    base_url: String,
    transfer_id: String,
    source_path: String,
    chunk: ParallelChunkRange,
    progress: Arc<AtomicI64>,
) -> Result<(), String> {
    let mut file = tokio::fs::File::open(&source_path)
        .await
        .map_err(|error| format!("打开并行上传源文件失败: {error}"))?;
    file.seek(SeekFrom::Start(chunk.offset))
        .await
        .map_err(|error| format!("定位并行上传分块失败: {error}"))?;
    let stream_progress = progress.clone();
    let stream = ReaderStream::with_capacity(file.take(chunk.length), PARALLEL_STREAM_BUFFER)
        .map(move |result| {
            if let Ok(bytes) = &result {
                stream_progress.fetch_add(bytes.len() as i64, Ordering::AcqRel);
            }
            result
        });
    let url = format!(
        "{base_url}/api/uploads/v2/{}/{}",
        urlencoding::encode(&transfer_id),
        chunk.index
    );
    let response = client
        .post(url)
        .header(reqwest::header::CONTENT_LENGTH, chunk.length)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .map_err(|error| format!("上传并行分块 {} 失败: {error}", chunk.index))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail: String = body.chars().take(512).collect();
        return Err(format!(
            "接收端拒绝并行分块 {} ({status}): {detail}",
            chunk.index
        ));
    }
    let response_status = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("status")?.as_str().map(str::to_owned));
    if !matches!(
        response_status.as_deref(),
        Some("receiving" | "completed" | "already_exists")
    ) {
        return Err(format!("并行分块 {} 返回未知状态", chunk.index));
    }
    Ok(())
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

pub(crate) fn parallel_chunk_ranges(file_size: u64) -> Vec<ParallelChunkRange> {
    if file_size <= CHUNK_SIZE as u64 {
        return vec![ParallelChunkRange {
            index: 0,
            offset: 0,
            length: file_size,
        }];
    }

    let base = file_size / 4;
    let remainder = file_size % 4;
    let mut offset = 0;
    (0..4)
        .map(|index| {
            let length = base + u64::from((index as u64) < remainder);
            let range = ParallelChunkRange {
                index,
                offset,
                length,
            };
            offset += length;
            range
        })
        .collect()
}

pub(crate) fn valid_parallel_prepare(request: &ParallelPrepareRequest) -> bool {
    !request.sender_id.trim().is_empty()
        && !request.conversation_id.trim().is_empty()
        && !request.client_message_id.trim().is_empty()
        && request.client_message_id.len() <= 128
        && !request.transfer_id.trim().is_empty()
        && request.transfer_id.len() <= 256
        && !request.sender_msg_id.trim().is_empty()
        && request.sender_msg_id.len() <= 64
        && request.file_sha256.len() == 64
        && request
            .file_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && request.chunks == parallel_chunk_ranges(request.file_size)
}

pub(crate) async fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("打开文件摘要源失败: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; PARALLEL_STREAM_BUFFER];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("读取文件摘要源失败: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn parallel_transfer_key(transfer_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(transfer_id.as_bytes());
    format!("{:x}", digest.finalize())
}

pub(crate) fn parallel_transfer_dir(download_root: &Path, transfer_id: &str) -> PathBuf {
    download_root
        .join(".xchat-receive")
        .join(parallel_transfer_key(transfer_id))
}

fn parallel_manifest_path(download_root: &Path, transfer_id: &str) -> PathBuf {
    parallel_transfer_dir(download_root, transfer_id).join("manifest.json")
}

fn parallel_part_path(download_root: &Path, transfer_id: &str, index: usize) -> PathBuf {
    parallel_transfer_dir(download_root, transfer_id).join(format!("{index:06}.part"))
}

pub(crate) async fn load_parallel_manifest(
    download_root: &Path,
    transfer_id: &str,
) -> Result<Option<ParallelTransferManifest>, String> {
    let path = parallel_manifest_path(download_root, transfer_id);
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取并行传输清单失败: {error}")),
    };
    let manifest: ParallelTransferManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析并行传输清单失败: {error}"))?;
    if manifest.version != 2
        || manifest.transfer_id != transfer_id
        || manifest.file_sha256.len() != 64
        || manifest.chunks != parallel_chunk_ranges(manifest.file_size)
    {
        return Err("并行传输清单无效".to_string());
    }
    Ok(Some(manifest))
}

pub(crate) async fn create_or_resume_parallel_manifest(
    download_root: &Path,
    manifest: ParallelTransferManifest,
) -> Result<(ParallelTransferManifest, Vec<usize>, u64), String> {
    if let Some(existing) = load_parallel_manifest(download_root, &manifest.transfer_id).await? {
        if existing != manifest {
            return Err("并行传输清单与已有内容冲突".to_string());
        }
        let received = received_parallel_chunks(download_root, &existing).await?;
        let bytes = received
            .iter()
            .map(|index| existing.chunks[*index].length)
            .sum();
        let missing = existing
            .chunks
            .iter()
            .filter(|chunk| !received.contains(&chunk.index))
            .map(|chunk| chunk.index)
            .collect();
        return Ok((existing, missing, bytes));
    }

    let directory = parallel_transfer_dir(download_root, &manifest.transfer_id);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| format!("创建并行传输目录失败: {error}"))?;
    let path = directory.join("manifest.json");
    let temporary = directory.join(format!(".manifest-{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec(&manifest)
        .map_err(|error| format!("序列化并行传输清单失败: {error}"))?;
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|error| format!("写入并行传输清单失败: {error}"))?;
    tokio::fs::rename(&temporary, &path)
        .await
        .map_err(|error| format!("发布并行传输清单失败: {error}"))?;
    let missing = manifest.chunks.iter().map(|chunk| chunk.index).collect();
    Ok((manifest, missing, 0))
}

async fn received_parallel_chunks(
    download_root: &Path,
    manifest: &ParallelTransferManifest,
) -> Result<BTreeSet<usize>, String> {
    let mut received = BTreeSet::new();
    for chunk in &manifest.chunks {
        let path = parallel_part_path(download_root, &manifest.transfer_id, chunk.index);
        match tokio::fs::metadata(&path).await {
            Ok(metadata) if metadata.is_file() && metadata.len() == chunk.length => {
                received.insert(chunk.index);
            }
            Ok(_) => {
                tokio::fs::remove_file(&path)
                    .await
                    .map_err(|error| format!("清理无效并行分块失败: {error}"))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("读取并行分块信息失败: {error}")),
        }
    }
    Ok(received)
}

async fn adjust_transfer_progress(
    pool: &Pool<Sqlite>,
    transfer_id: &str,
    delta: i64,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE transfers
         SET bytes_transferred = MIN(bytes_total, MAX(0, bytes_transferred + ?)),
             updated_at = ?
         WHERE id = ? AND status = 'transferring'",
    )
    .bind(delta)
    .bind(unix_timestamp())
    .bind(transfer_id)
    .execute(pool)
    .await
    .map_err(|error| format!("更新并行传输进度失败: {error}"))?;
    Ok(())
}

async fn rollback_parallel_chunk_attempt(
    pool: &Pool<Sqlite>,
    transfer_id: &str,
    temporary: &Path,
    published: Option<&Path>,
    reported: i64,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for path in published.into_iter().chain(std::iter::once(temporary)) {
        if let Err(error) = tokio::fs::remove_file(path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                errors.push(format!("清理 {} 失败: {error}", path.display()));
            }
        }
    }
    if reported > 0 {
        if let Err(error) = adjust_transfer_progress(pool, transfer_id, -reported).await {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn fail_parallel_chunk_attempt(
    pool: &Pool<Sqlite>,
    transfer_id: &str,
    temporary: &Path,
    published: Option<&Path>,
    reported: i64,
    error: String,
) -> String {
    match rollback_parallel_chunk_attempt(
        pool,
        transfer_id,
        temporary,
        published,
        reported,
    )
    .await
    {
        Ok(()) => error,
        Err(rollback_error) => format!("{error}; 回滚并行分块失败: {rollback_error}"),
    }
}

pub(crate) async fn receive_parallel_chunk(
    pool: &Pool<Sqlite>,
    download_root: &Path,
    transfer_id: &str,
    chunk_index: usize,
    body: axum::body::Body,
) -> Result<ParallelChunkReceiveResult, String> {
    let manifest = load_parallel_manifest(download_root, transfer_id)
        .await?
        .ok_or_else(|| "并行传输尚未准备".to_string())?;
    let chunk = manifest
        .chunks
        .get(chunk_index)
        .filter(|chunk| chunk.index == chunk_index)
        .cloned()
        .ok_or_else(|| "并行分块序号无效".to_string())?;
    let transfer = db::get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "并行接收传输不存在".to_string())?;
    if transfer.direction != "receive" || transfer.status != "transferring" {
        return Err("并行接收传输当前不可写".to_string());
    }

    let existing_path = parallel_part_path(download_root, transfer_id, chunk_index);
    if tokio::fs::metadata(&existing_path)
        .await
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == chunk.length)
    {
        let received = received_parallel_chunks(download_root, &manifest).await?;
        let bytes = received
            .iter()
            .map(|index| manifest.chunks[*index].length)
            .sum();
        return Ok(ParallelChunkReceiveResult {
            complete: received.len() == manifest.chunks.len(),
            manifest,
            received: bytes,
        });
    }

    let directory = parallel_transfer_dir(download_root, transfer_id);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| format!("创建并行分块目录失败: {error}"))?;
    let temporary = directory.join(format!(
        ".{chunk_index:06}-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|error| format!("创建并行分块临时文件失败: {error}"))?;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    let mut reported = 0i64;
    let mut pending = 0i64;
    let mut last_report = Instant::now();
    while let Some(data) = stream.next().await {
        let data = match data {
            Ok(data) => data,
            Err(error) => {
                drop(file);
                return Err(
                    fail_parallel_chunk_attempt(
                        pool,
                        transfer_id,
                        &temporary,
                        None,
                        reported,
                        format!("读取并行分块请求失败: {error}"),
                    )
                    .await,
                );
            }
        };
        written = written.saturating_add(data.len() as u64);
        if written > chunk.length {
            drop(file);
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    "并行分块超过声明长度".to_string(),
                )
                .await,
            );
        }
        if let Err(error) = file.write_all(&data).await {
            drop(file);
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    format!("写入并行分块失败: {error}"),
                )
                .await,
            );
        }
        pending += data.len() as i64;
        if pending >= 1024 * 1024 || last_report.elapsed() >= Duration::from_millis(250) {
            if let Err(error) = adjust_transfer_progress(pool, transfer_id, pending).await {
                drop(file);
                return Err(
                    fail_parallel_chunk_attempt(
                        pool,
                        transfer_id,
                        &temporary,
                        None,
                        reported,
                        error,
                    )
                    .await,
                );
            }
            reported += pending;
            pending = 0;
            last_report = Instant::now();
        }
    }
    if pending > 0 {
        if let Err(error) = adjust_transfer_progress(pool, transfer_id, pending).await {
            drop(file);
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    error,
                )
                .await,
            );
        }
        reported += pending;
    }
    if written != chunk.length {
        drop(file);
        return Err(
            fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                "并行分块长度与清单不一致".to_string(),
            )
            .await,
        );
    }
    if let Err(error) = file.flush().await {
        drop(file);
        return Err(
            fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                format!("保存并行分块失败: {error}"),
            )
            .await,
        );
    }
    drop(file);

    let _guard = lock_receive_file(&manifest.client_message_id).await;
    let transfer = match db::get_transfer(pool, transfer_id).await {
        Ok(Some(transfer)) => transfer,
        Ok(None) => {
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    "并行接收传输不存在".to_string(),
                )
                .await,
            )
        }
        Err(error) => {
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    error,
                )
                .await,
            )
        }
    };
    if transfer.status != "transferring" {
        return Err(
            fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                "并行接收传输已结束".to_string(),
            )
            .await,
        );
    }
    let mut published = false;
    if tokio::fs::metadata(&existing_path)
        .await
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == chunk.length)
    {
        rollback_parallel_chunk_attempt(pool, transfer_id, &temporary, None, reported).await?;
        reported = 0;
    } else {
        if let Err(error) = tokio::fs::remove_file(&existing_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(
                    fail_parallel_chunk_attempt(
                        pool,
                        transfer_id,
                        &temporary,
                        None,
                        reported,
                        format!("替换无效并行分块失败: {error}"),
                    )
                    .await,
                );
            }
        }
        if let Err(error) = tokio::fs::rename(&temporary, &existing_path).await {
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    format!("发布并行分块失败: {error}"),
                )
                .await,
            );
        }
        published = true;
    }
    let received = match received_parallel_chunks(download_root, &manifest).await {
        Ok(received) => received,
        Err(error) => {
            return Err(
                fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    published.then_some(existing_path.as_path()),
                    reported,
                    error,
                )
                .await,
            )
        }
    };
    let bytes = received
        .iter()
        .map(|index| manifest.chunks[*index].length)
        .sum();
    if let Err(error) = sqlx::query(
        "UPDATE transfers
         SET bytes_transferred = MAX(bytes_transferred, ?), updated_at = ?
         WHERE id = ? AND status = 'transferring'",
    )
    .bind(bytes as i64)
    .bind(unix_timestamp())
    .bind(transfer_id)
    .execute(pool)
    .await
    {
        return Err(
            fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                published.then_some(existing_path.as_path()),
                reported,
                format!("校正并行传输进度失败: {error}"),
            )
            .await,
        );
    }
    Ok(ParallelChunkReceiveResult {
        complete: received.len() == manifest.chunks.len(),
        manifest,
        received: bytes,
    })
}

pub(crate) async fn merge_parallel_parts(
    download_root: &Path,
    manifest: &ParallelTransferManifest,
) -> Result<PathBuf, String> {
    let partial_path = received_partial_path(download_root, &manifest.transfer_id);
    let mut output = tokio::fs::File::create(&partial_path)
        .await
        .map_err(|error| format!("创建并行合并文件失败: {error}"))?;
    let mut total = 0u64;
    for chunk in &manifest.chunks {
        let path = parallel_part_path(download_root, &manifest.transfer_id, chunk.index);
        let mut part = tokio::fs::File::open(&path)
            .await
            .map_err(|error| format!("打开并行分块失败: {error}"))?;
        let copied = tokio::io::copy(&mut part, &mut output)
            .await
            .map_err(|error| format!("合并并行分块失败: {error}"))?;
        if copied != chunk.length {
            let _ = tokio::fs::remove_file(&partial_path).await;
            return Err("并行分块长度在合并前发生变化".to_string());
        }
        total += copied;
    }
    output
        .flush()
        .await
        .map_err(|error| format!("保存并行合并文件失败: {error}"))?;
    drop(output);
    if total != manifest.file_size {
        let _ = tokio::fs::remove_file(&partial_path).await;
        return Err("并行合并文件大小不一致".to_string());
    }
    let digest = sha256_file(&partial_path).await?;
    if digest != manifest.file_sha256 {
        let _ = tokio::fs::remove_file(&partial_path).await;
        cleanup_parallel_transfer(download_root, &manifest.transfer_id).await?;
        return Err("并行合并文件 SHA-256 校验失败".to_string());
    }
    Ok(partial_path)
}

pub(crate) async fn cleanup_parallel_transfer(
    download_root: &Path,
    transfer_id: &str,
) -> Result<(), String> {
    let directory = parallel_transfer_dir(download_root, transfer_id);
    match tokio::fs::remove_dir_all(&directory).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("清理并行传输分块失败: {error}")),
    }
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
    fn parallel_ranges_use_one_small_part_or_four_balanced_parts() {
        assert_eq!(
            parallel_chunk_ranges(CHUNK_SIZE as u64),
            vec![ParallelChunkRange {
                index: 0,
                offset: 0,
                length: CHUNK_SIZE as u64,
            }]
        );

        let size = CHUNK_SIZE as u64 + 3;
        let ranges = parallel_chunk_ranges(size);
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges.first().unwrap().offset, 0);
        assert_eq!(
            ranges.iter().map(|range| range.length).sum::<u64>(),
            size
        );
        assert!(ranges.windows(2).all(|pair| {
            pair[0].offset + pair[0].length == pair[1].offset
                && pair[0].length.abs_diff(pair[1].length) <= 1
        }));
    }

    #[tokio::test]
    async fn parallel_manifest_rejects_conflicts_and_merges_verified_parts() {
        let root =
            std::env::temp_dir().join(format!("xchat-parallel-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let size = CHUNK_SIZE + 3;
        let data: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
        let source = root.join("source.bin");
        tokio::fs::write(&source, &data).await.unwrap();
        let manifest = ParallelTransferManifest {
            version: 2,
            sender_id: "sender".into(),
            conversation_id: "conversation".into(),
            client_message_id: "message".into(),
            transfer_id: "message:receiver".into(),
            sender_msg_id: "42".into(),
            file_name: "source.bin".into(),
            final_file_name: "source.bin".into(),
            file_size: size as u64,
            file_sha256: sha256_file(&source).await.unwrap(),
            chunks: parallel_chunk_ranges(size as u64),
            message_id: 7,
        };

        let (_, missing, received) =
            create_or_resume_parallel_manifest(&root, manifest.clone())
                .await
                .unwrap();
        assert_eq!(missing, vec![0, 1, 2, 3]);
        assert_eq!(received, 0);

        let mut conflict = manifest.clone();
        conflict.file_sha256 = "0".repeat(64);
        assert!(create_or_resume_parallel_manifest(&root, conflict)
            .await
            .unwrap_err()
            .contains("冲突"));

        for chunk in &manifest.chunks {
            let start = chunk.offset as usize;
            let end = start + chunk.length as usize;
            tokio::fs::write(
                parallel_part_path(&root, &manifest.transfer_id, chunk.index),
                &data[start..end],
            )
            .await
            .unwrap();
        }
        let (_, missing, received) =
            create_or_resume_parallel_manifest(&root, manifest.clone())
                .await
                .unwrap();
        assert!(missing.is_empty());
        assert_eq!(received, size as u64);

        let merged = merge_parallel_parts(&root, &manifest).await.unwrap();
        assert_eq!(tokio::fs::read(&merged).await.unwrap(), data);
        cleanup_parallel_transfer(&root, &manifest.transfer_id)
            .await
            .unwrap();
        tokio::fs::remove_file(merged).await.unwrap();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn parallel_chunk_progress_failure_cleans_attempt_and_rolls_back() {
        let app_dir = std::env::temp_dir().join(format!(
            "xchat-parallel-progress-test-{}",
            uuid::Uuid::new_v4()
        ));
        let pool = db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let download_root = app_dir.join("downloads");
        tokio::fs::create_dir_all(&download_root).await.unwrap();
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
        let self_id = db::get_user_id(&pool).await.unwrap();
        let message = db::save_conversation_message(
            &pool,
            &conversation.id,
            "peer-a",
            Some(&self_id),
            "large.bin",
            "file",
            unix_timestamp(),
            "received",
            "parallel-progress-message",
        )
        .await
        .unwrap();
        let file_size = 2 * 1024 * 1024;
        let transfer_id = "parallel-progress-transfer";
        db::create_transfer(
            &pool,
            transfer_id,
            Some(message.id),
            &conversation.id,
            "peer-a",
            "receive",
            "transferring",
            file_size,
        )
        .await
        .unwrap();
        let manifest = ParallelTransferManifest {
            version: 2,
            sender_id: "peer-a".into(),
            conversation_id: conversation.id,
            client_message_id: "parallel-progress-message".into(),
            transfer_id: transfer_id.into(),
            sender_msg_id: "parallel-progress-sender".into(),
            file_name: "large.bin".into(),
            final_file_name: "large.bin".into(),
            file_size: file_size as u64,
            file_sha256: "0".repeat(64),
            chunks: parallel_chunk_ranges(file_size as u64),
            message_id: message.id,
        };
        create_or_resume_parallel_manifest(&download_root, manifest)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TRIGGER fail_second_parallel_progress
             BEFORE UPDATE OF bytes_transferred ON transfers
             WHEN NEW.id = 'parallel-progress-transfer'
               AND NEW.bytes_transferred > 1048576
             BEGIN
               SELECT RAISE(FAIL, 'forced progress failure');
             END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let body = axum::body::Body::from_stream(futures_util::stream::iter([
            Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![1; 1024 * 1024])),
            Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![2; 1024 * 1024])),
        ]));
        let error =
            receive_parallel_chunk(&pool, &download_root, transfer_id, 0, body)
                .await
                .unwrap_err();
        assert!(error.contains("更新并行传输进度失败"));
        let transfer = db::get_transfer(&pool, transfer_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.bytes_transferred, 0);
        let mut entries = tokio::fs::read_dir(parallel_transfer_dir(
            &download_root,
            transfer_id,
        ))
        .await
        .unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            assert!(
                !entry.file_name().to_string_lossy().ends_with(".tmp"),
                "failed chunk left a temporary file"
            );
        }

        pool.close().await;
        tokio::fs::remove_dir_all(app_dir).await.unwrap();
    }

    #[tokio::test]
    async fn parallel_sender_failure_marks_receiver_failed() {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let terminal_tx = status_tx.clone();
        let router = axum::Router::new()
            .route(
                "/api/uploads/v2/prepare",
                axum::routing::post(|| async {
                    axum::Json(serde_json::json!({
                        "status": "ready",
                        "received": 0,
                        "missing_chunks": [0],
                    }))
                }),
            )
            .route(
                "/api/uploads/v2/:transfer_id/:chunk_index",
                axum::routing::post(|| async {
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "forced chunk failure",
                    )
                }),
            )
            .route(
                "/api/uploads/:client_message_id/cancel",
                axum::routing::post(
                    move |axum::extract::Query(query): axum::extract::Query<
                        HashMap<String, String>,
                    >| {
                        let terminal_tx = terminal_tx.clone();
                        async move {
                            let status = query.get("status").cloned().unwrap_or_default();
                            terminal_tx.send(status.clone()).unwrap();
                            axum::Json(serde_json::json!({ "status": status }))
                        }
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let app_dir = std::env::temp_dir().join(format!(
            "xchat-parallel-sender-failure-{}",
            uuid::Uuid::new_v4()
        ));
        let pool = db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        db::save_or_update_user(
            &pool,
            "peer-a".into(),
            "Alice".into(),
            address.to_string(),
            true,
            0,
        )
        .await
        .unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let self_id = db::get_user_id(&pool).await.unwrap();
        let source_path = app_dir.join("failure.bin");
        tokio::fs::write(&source_path, b"failure").await.unwrap();
        let message = db::save_conversation_message(
            &pool,
            &conversation.id,
            &self_id,
            Some("peer-a"),
            "failure.bin",
            "file",
            unix_timestamp(),
            "sending",
            "parallel-sender-failure",
        )
        .await
        .unwrap();
        db::set_file_message_metadata(
            &pool,
            message.id,
            source_path.to_str().unwrap(),
            7,
            "transferring",
        )
        .await
        .unwrap();
        let transfer_id = "parallel-sender-failure:peer-a";
        db::create_transfer(
            &pool,
            transfer_id,
            Some(message.id),
            &conversation.id,
            "peer-a",
            "send",
            "queued",
            7,
        )
        .await
        .unwrap();
        run_upload(
            &pool,
            UploadJob {
                transfer_id: transfer_id.into(),
                peer_addr: address.to_string(),
                conversation_id: conversation.id,
                client_message_id: "parallel-sender-failure".into(),
                message_id: message.id,
                source: ValidatedSource {
                    path: source_path.to_string_lossy().into_owned(),
                    file_name: "failure.bin".into(),
                    size: 7,
                },
                group_sync: None,
                parallel_v2: true,
                file_sha256: Some(sha256_file(&source_path).await.unwrap()),
            },
        )
        .await;

        assert_eq!(status_rx.try_recv().unwrap(), "failed");
        let transfer = db::get_transfer(&pool, transfer_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transfer.status, "failed");

        server.abort();
        pool.close().await;
        tokio::fs::remove_dir_all(app_dir).await.unwrap();
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
            false,
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
