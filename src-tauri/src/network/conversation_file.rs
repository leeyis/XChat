use crate::{
    db::{self, ConversationMemberRecord, ConversationRecord, MessageRecord, TransferRecord},
    peers::PeerManager,
};
use futures_util::{
    future::{BoxFuture, Shared},
    stream, FutureExt, StreamExt,
};
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
const FILE_MERGE_BUFFER: usize = 1024 * 1024;
const PARALLEL_PARTS_PER_CHANNEL: usize = 4;
const MAX_PARALLEL_PARTS: usize = 4096;
pub const PARALLEL_FILE_CAPABILITY: &str = "parallel_file_v2";
pub const PARALLEL_FILE_V3_CAPABILITY: &str = "parallel_file_v3:16";
const PARALLEL_FILE_V3_CAPABILITY_PREFIX: &str = "parallel_file_v3:";
pub const PARALLEL_FILE_V4_CAPABILITY: &str = "parallel_file_v4:16";
const PARALLEL_FILE_V4_CAPABILITY_PREFIX: &str = "parallel_file_v4:";
type ReceiveTransferLock = tokio::sync::Mutex<()>;
type FileSha256 = Shared<BoxFuture<'static, Result<String, String>>>;
static RECEIVE_TRANSFER_LOCKS: OnceLock<Mutex<HashMap<String, Weak<ReceiveTransferLock>>>> =
    OnceLock::new();
static RESUME_TRANSFER_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static UPLOAD_START_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RECEIVE_PROCESSING_PHASES: OnceLock<Mutex<HashMap<String, ReceiveProcessingPhase>>> =
    OnceLock::new();
static RECEIVE_PROCESSING_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn receive_processing_revision() -> u64 {
    RECEIVE_PROCESSING_REVISION.load(Ordering::Acquire)
}
static RECEIVE_PROGRESS_WINDOWS: OnceLock<Mutex<HashMap<String, Weak<Mutex<Instant>>>>> =
    OnceLock::new();

fn receive_progress_window(transfer_id: &str) -> Arc<Mutex<Instant>> {
    let mut windows = RECEIVE_PROGRESS_WINDOWS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    windows.retain(|_, window| window.strong_count() > 0);
    if let Some(window) = windows.get(transfer_id).and_then(Weak::upgrade) {
        return window;
    }
    let window = Arc::new(Mutex::new(Instant::now()));
    windows.insert(transfer_id.to_owned(), Arc::downgrade(&window));
    window
}

fn take_progress_window(window: &Mutex<Instant>, now: Instant) -> bool {
    let mut last = window.lock().unwrap_or_else(|error| error.into_inner());
    if now.duration_since(*last) < Duration::from_millis(500) {
        return false;
    }
    *last = now;
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiveProcessingPhase {
    Merging,
    Verifying,
    Saving,
}

impl ReceiveProcessingPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Merging => "merging",
            Self::Verifying => "verifying",
            Self::Saving => "saving",
        }
    }
}

pub(crate) fn receive_processing_phase(transfer_id: &str) -> Option<ReceiveProcessingPhase> {
    RECEIVE_PROCESSING_PHASES.get().and_then(|phases| {
        phases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(transfer_id)
            .copied()
    })
}

// The caller holds the receive-file lock. A dropped/failed finalizer cannot leave a stale phase.
pub(crate) struct ReceiveProcessingGuard {
    transfer_id: String,
}

impl ReceiveProcessingGuard {
    pub(crate) fn new(transfer_id: &str) -> Self {
        Self {
            transfer_id: transfer_id.to_string(),
        }
    }

    pub(crate) fn set_phase(&self, phase: ReceiveProcessingPhase) {
        RECEIVE_PROCESSING_PHASES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(self.transfer_id.clone(), phase);
        RECEIVE_PROCESSING_REVISION.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for ReceiveProcessingGuard {
    fn drop(&mut self) {
        if let Some(phases) = RECEIVE_PROCESSING_PHASES.get() {
            phases
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.transfer_id);
            RECEIVE_PROCESSING_REVISION.fetch_add(1, Ordering::AcqRel);
        }
    }
}

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
    peer_id: String,
    peer_addr: String,
    conversation_id: String,
    client_message_id: String,
    message_id: i64,
    source: ValidatedSource,
    group_sync: Option<ProtocolMessage>,
    upload_plan: UploadPlan,
    concurrency: super::transfer::TransferConcurrencyGeneration,
    file_sha256: Option<FileSha256>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UploadProtocol {
    SequentialV1,
    FixedV2,
    FlexibleV3,
    StreamingV4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UploadPlan {
    protocol: UploadProtocol,
    channels: u8,
}

impl UploadPlan {
    const fn new(protocol: UploadProtocol, channels: u8) -> Self {
        Self { protocol, channels }
    }

    fn is_parallel(self) -> bool {
        self.protocol != UploadProtocol::SequentialV1
    }

    fn manifest_version(self) -> Option<u8> {
        match self.protocol {
            UploadProtocol::SequentialV1 => None,
            UploadProtocol::FixedV2 => Some(2),
            UploadProtocol::FlexibleV3 => Some(3),
            UploadProtocol::StreamingV4 => Some(4),
        }
    }
}

enum UploadOutcome {
    Completed(i64),
    AwaitingAcceptance(i64),
    Cancelled(i64),
    Failed(i64, String),
}

async fn await_with_transfer_cancellation<F>(
    future: F,
    token: &super::transfer::TransferCancellationToken,
) -> Option<F::Output>
where
    F: std::future::Future,
{
    tokio::pin!(future);
    loop {
        if token.load(Ordering::Acquire) {
            return None;
        }
        tokio::select! {
            output = &mut future => return Some(output),
            _ = tokio::time::sleep(Duration::from_millis(25)) => {}
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ParallelChunkRange {
    pub index: usize,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ParallelPrepareRequest {
    #[serde(default)]
    pub voice: Option<crate::voice::VoiceMetadata>,
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
    #[serde(default)]
    max_parallel_channels: Option<u8>,
}

fn advertised_parallel_limit(capabilities: &[String], prefix: &str) -> Option<u8> {
    capabilities
        .iter()
        .filter_map(|capability| capability.strip_prefix(prefix))
        .filter_map(|value| value.parse::<u8>().ok())
        .filter_map(|value| super::transfer::validate_max_parallel_channels(value).ok())
        .max()
}

fn negotiate_upload_plan(capabilities: &[String], local_limit: u8) -> UploadPlan {
    let local_limit = super::transfer::validate_max_parallel_channels(local_limit)
        .unwrap_or(super::transfer::DEFAULT_MAX_PARALLEL_CHANNELS);
    if let Some(remote_limit) =
        advertised_parallel_limit(capabilities, PARALLEL_FILE_V4_CAPABILITY_PREFIX)
    {
        return UploadPlan::new(UploadProtocol::StreamingV4, local_limit.min(remote_limit));
    }
    let remote_v3_limit =
        advertised_parallel_limit(capabilities, PARALLEL_FILE_V3_CAPABILITY_PREFIX);
    if let Some(remote_limit) = remote_v3_limit {
        return UploadPlan::new(UploadProtocol::FlexibleV3, local_limit.min(remote_limit));
    }
    if capabilities
        .iter()
        .any(|capability| capability == PARALLEL_FILE_CAPABILITY)
    {
        return UploadPlan::new(UploadProtocol::FixedV2, 4);
    }
    UploadPlan::new(UploadProtocol::SequentialV1, 1)
}

fn encoded_parallel_plan(transfer_id: &str) -> Option<UploadPlan> {
    for (protocol, prefix) in [
        (UploadProtocol::StreamingV4, ":retry:v4c"),
        (UploadProtocol::FlexibleV3, ":retry:v3c"),
    ] {
        if let Some((_, suffix)) = transfer_id.rsplit_once(prefix) {
            let (channels, nonce) = suffix.split_once('-')?;
            if nonce.is_empty() {
                return None;
            }
            let channels =
                super::transfer::validate_max_parallel_channels(channels.parse().ok()?).ok()?;
            return Some(UploadPlan::new(protocol, channels));
        }
    }
    None
}

fn transfer_id_matches_upload_plan(transfer_id: &str, upload_plan: UploadPlan) -> bool {
    match upload_plan.protocol {
        UploadProtocol::FlexibleV3 | UploadProtocol::StreamingV4 => {
            encoded_parallel_plan(transfer_id) == Some(upload_plan)
        }
        UploadProtocol::SequentialV1 | UploadProtocol::FixedV2 => {
            encoded_parallel_plan(transfer_id).is_none()
        }
    }
}

fn new_transfer_id_for_plan(
    client_message_id: &str,
    peer_id: &str,
    upload_plan: UploadPlan,
    retry: bool,
) -> String {
    let base = recipient_transfer_id(client_message_id, peer_id);
    match upload_plan.protocol {
        UploadProtocol::FlexibleV3 | UploadProtocol::StreamingV4 => format!(
            "{base}:retry:v{}c{}-{}",
            upload_plan.manifest_version().unwrap(),
            upload_plan.channels,
            uuid::Uuid::new_v4()
        ),
        UploadProtocol::SequentialV1 | UploadProtocol::FixedV2 if retry => {
            format!("{base}:retry:{}", uuid::Uuid::new_v4())
        }
        UploadProtocol::SequentialV1 | UploadProtocol::FixedV2 => base,
    }
}

fn upload_plan_for_resume(
    transfer_id: &str,
    capabilities: &[String],
    local_limit: u8,
) -> UploadPlan {
    if let Some(plan) = encoded_parallel_plan(transfer_id) {
        let prefix = if plan.protocol == UploadProtocol::StreamingV4 {
            PARALLEL_FILE_V4_CAPABILITY_PREFIX
        } else {
            PARALLEL_FILE_V3_CAPABILITY_PREFIX
        };
        if advertised_parallel_limit(capabilities, prefix)
            .is_some_and(|remote| remote >= plan.channels)
        {
            return plan;
        }
        return negotiate_upload_plan(capabilities, local_limit);
    }
    if capabilities
        .iter()
        .any(|capability| capability == PARALLEL_FILE_CAPABILITY)
    {
        UploadPlan::new(UploadProtocol::FixedV2, 4)
    } else {
        UploadPlan::new(UploadProtocol::SequentialV1, 1)
    }
}

pub async fn send_path(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    source_path: &str,
) -> Result<ConversationFileSendResult, String> {
    send_path_identified(pool, peer_manager, conversation_id, source_path,
        &uuid::Uuid::new_v4().to_string(), None).await
}

pub(crate) async fn send_path_identified(
    pool: &Pool<Sqlite>,
    peer_manager: &PeerManager,
    conversation_id: &str,
    source_path: &str,
    client_message_id: &str,
    voice: Option<&crate::voice::VoiceMetadata>,
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
    let concurrency = super::transfer::concurrency_controller()
        .configured_generation(pool)
        .await?;
    let local_limit = concurrency.limit();
    let upload_plans: HashMap<_, _> = recipient_ids
        .iter()
        .map(|peer_id| {
            let capabilities = peers
                .get(peer_id)
                .map(|peer| peer.capabilities.as_slice())
                .unwrap_or_default();
            (
                peer_id.clone(),
                if voice.is_some() { UploadPlan::new(UploadProtocol::StreamingV4, local_limit) }
                else { negotiate_upload_plan(capabilities, local_limit) },
            )
        })
        .collect();
    let client_message_id = client_message_id.to_string();
    let receiver_id = (conversation.kind == "direct")
        .then_some(conversation.peer_id.as_deref())
        .flatten();
    let message_status = if online_addresses.is_empty() {
        "pending"
    } else {
        "sent"
    };
    let file_status = if online_addresses.is_empty() {
        "waiting_peer"
    } else {
        "queued"
    };
    let group_sync = group_sync_message(&conversation, &members)?;
    let planned_transfers = recipient_ids
        .iter()
        .map(|peer_id| {
            let upload_plan = upload_plans
                .get(peer_id)
                .copied()
                .unwrap_or_else(|| UploadPlan::new(UploadProtocol::SequentialV1, 1));
            db::OutgoingTransfer {
                id: new_transfer_id_for_plan(&client_message_id, peer_id, upload_plan, false),
                peer_id: peer_id.clone(),
                status: if online_addresses.contains_key(peer_id) {
                    "queued"
                } else {
                    "waiting_peer"
                },
            }
        })
        .collect::<Vec<_>>();
    let enqueued = db::enqueue_outgoing_message(
        pool,
        db::OutgoingMessage {
            conversation_id,
            sender_id: &my_id,
            receiver_id,
            content: &source.file_name,
            msg_type: if voice.is_some() { "voice" } else { "file" },
            timestamp: unix_timestamp(),
            status: message_status,
            client_message_id: &client_message_id,
            recipients: &recipient_ids,
            mentions: &[],
        },
        Some(db::OutgoingFile {
            voice,
            path: &source.path,
            size: source.size,
            status: file_status,
            transfers: &planned_transfers,
        }),
    )
    .await?;
    if !enqueued.is_new {
        return Ok(ConversationFileSendResult { message: enqueued.message, transfers: enqueued.transfers });
    }
    let message = enqueued.message;
    crate::tasks::remember_source(pool, message.id, &source.path).await?;
    // Recipients and recovery metadata share one bounded digest reader. Uploads can start immediately.
    let parallel_hash = recorded_file_sha256(pool, message.id, source.path.clone());
    let saved_hash = parallel_hash.clone();
    tokio::spawn(async move { let _ = saved_hash.await; });
    let transfers = enqueued.transfers;
    let mut jobs = Vec::with_capacity(online_addresses.len());
    for transfer in &transfers {
        let peer_id = &transfer.peer_id;
        let upload_plan = upload_plans[peer_id];
        if let Some(peer_addr) = online_addresses.get(peer_id) {
            jobs.push(UploadJob {
                transfer_id: transfer.id.clone(),
                peer_id: peer_id.clone(),
                peer_addr: peer_addr.clone(),
                conversation_id: conversation_id.to_string(),
                client_message_id: client_message_id.clone(),
                message_id: message.id,
                source: source.clone(),
                group_sync: group_sync.clone(),
                upload_plan,
                concurrency: concurrency.clone(),
                file_sha256: upload_plan.is_parallel().then(|| parallel_hash.clone()),
            });
        }
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
    let _resume_guard = RESUME_TRANSFER_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock().await;
    let peer_id = peer_id.trim();
    let peer_addr = peer_addr.trim();
    if peer_id.is_empty() || peer_addr.is_empty() {
        return Err("peer id and address are required".to_string());
    }
    let active_peers = peer_manager.get_active_peers();
    let Some(peer) = active_peers.iter().find(|peer| peer.id == peer_id) else {
        return Err("peer is not online".to_string());
    };
    let concurrency = super::transfer::concurrency_controller()
        .configured_generation(pool)
        .await?;
    let local_limit = concurrency.limit();

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
        // Recovery must reuse the receiver's persisted manifest while the peer
        // still supports its layout, just like an explicit manual resume.
        let upload_plan = upload_plan_for_resume(&transfer.id, &peer.capabilities, local_limit);
        match prepare_waiting_resume_job(
            pool,
            &transfer,
            peer_addr,
            upload_plan,
            concurrency.clone(),
        )
        .await
        {
            Ok(job) => {
                let claimed = db::update_transfer(
                    pool,
                    &job.transfer_id,
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

async fn prepare_waiting_resume_job(
    pool: &Pool<Sqlite>,
    transfer: &TransferRecord,
    peer_addr: &str,
    upload_plan: UploadPlan,
    concurrency: super::transfer::TransferConcurrencyGeneration,
) -> Result<UploadJob, String> {
    let mut job = prepare_resume_job(pool, transfer, peer_addr, upload_plan, concurrency).await?;
    if transfer_id_matches_upload_plan(&job.transfer_id, upload_plan) {
        return Ok(job);
    }

    let replacement_id =
        new_transfer_id_for_plan(&job.client_message_id, &job.peer_id, upload_plan, true);
    let result = sqlx::query(
        "UPDATE transfers SET id = ?, updated_at = ?
         WHERE id = ? AND direction = 'send' AND status = 'waiting_peer'",
    )
    .bind(&replacement_id)
    .bind(unix_timestamp())
    .bind(&job.transfer_id)
    .execute(pool)
    .await
    .map_err(|error| format!("更新待发送传输协议失败: {error}"))?;
    if result.rows_affected() != 1 {
        return Err("待发送传输状态已变化".to_string());
    }
    job.transfer_id = replacement_id;
    Ok(job)
}

pub async fn resume_transfer(
    pool: &Pool<Sqlite>,
    message_id: i64,
    peer_id: &str,
    peer_addr: &str,
    peer_capabilities: &[String],
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
    let concurrency = super::transfer::concurrency_controller()
        .configured_generation(pool)
        .await?;
    let local_limit = concurrency.limit();
    let upload_plan = upload_plan_for_resume(&previous.id, peer_capabilities, local_limit);
    let mut job = prepare_resume_job(pool, previous, peer_addr, upload_plan, concurrency).await?;
    let layout_matches = transfer_id_matches_upload_plan(&previous.id, upload_plan);
    let transfer = if previous.status == "awaiting_acceptance" && layout_matches {
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
    } else if previous.status == "failed" && upload_plan.is_parallel() && layout_matches {
        reset_send_transfer_for_retry(pool, &previous.id, "queued").await?
    } else if matches!(previous.status.as_str(), "failed" | "awaiting_acceptance") {
        if previous.status == "awaiting_acceptance" {
            db::update_transfer(
                pool,
                &previous.id,
                "cancelled",
                previous.bytes_transferred,
                None,
            )
            .await?;
        }
        let transfer_id =
            new_transfer_id_for_plan(&job.client_message_id, peer_id, upload_plan, true);
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
    if crate::backup::is_history(pool, message.client_message_id.as_deref().unwrap_or_default()).await? {
        return Err("恢复的历史文件不会重新入队；请转发为新消息".into());
    }
    if !matches!(message.msg_type.as_str(), "file" | "voice")
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
    crate::tasks::source_unchanged(pool, message_id, source_path).await?;
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
    sqlx::query("DELETE FROM task_hidden WHERE message_id=?")
        .bind(message_id).execute(pool).await.map_err(|e| e.to_string())?;

    let peers: HashMap<_, _> = peer_manager
        .get_all_peers()
        .into_iter()
        .map(|peer| (peer.id.clone(), peer))
        .collect();
    let concurrency = super::transfer::concurrency_controller()
        .configured_generation(pool)
        .await?;
    let local_limit = concurrency.limit();
    let upload_plans: HashMap<_, _> = retry_recipients
        .iter()
        .map(|peer_id| {
            let capabilities = peers
                .get(peer_id)
                .map(|peer| peer.capabilities.as_slice())
                .unwrap_or_default();
            (
                peer_id.clone(),
                if message.msg_type == "voice" { UploadPlan::new(UploadProtocol::StreamingV4, local_limit) }
                else { negotiate_upload_plan(capabilities, local_limit) },
            )
        })
        .collect();
    let parallel_hash = recorded_file_sha256(pool, message_id, source.path.clone());
    let group_sync = group_sync_message(&conversation, &members)?;
    let mut transfers = Vec::with_capacity(retry_recipients.len());
    let mut jobs = Vec::new();
    for peer_id in retry_recipients {
        let upload_plan = upload_plans
            .get(&peer_id)
            .copied()
            .unwrap_or_else(|| UploadPlan::new(UploadProtocol::SequentialV1, 1));
        let peer_addr = peers.get(&peer_id).and_then(|peer| {
            (!peer.is_offline && !peer.addr.trim().is_empty()).then(|| peer.addr.clone())
        });
        let status = if peer_addr.is_some() {
            "queued"
        } else {
            "waiting_peer"
        };
        let reusable = upload_plan
            .is_parallel()
            .then(|| {
                existing
                    .iter()
                    .filter(|transfer| {
                        transfer.peer_id == peer_id
                            && transfer.status == "failed"
                            && transfer_id_matches_upload_plan(&transfer.id, upload_plan)
                    })
                    .max_by_key(|transfer| transfer.updated_at)
            })
            .flatten();
        let (transfer_id, transfer) = if let Some(previous) = reusable {
            (
                previous.id.clone(),
                reset_send_transfer_for_retry(pool, &previous.id, status).await?,
            )
        } else {
            let transfer_id =
                new_transfer_id_for_plan(client_message_id, &peer_id, upload_plan, true);
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
                peer_id: peer_id.clone(),
                peer_addr,
                conversation_id: conversation_id.to_string(),
                client_message_id: client_message_id.to_string(),
                message_id,
                source: source.clone(),
                group_sync: group_sync.clone(),
                upload_plan,
                concurrency: concurrency.clone(),
                file_sha256: upload_plan
                    .is_parallel()
                    .then(|| parallel_hash.clone()),
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
    upload_plan: UploadPlan,
    concurrency: super::transfer::TransferConcurrencyGeneration,
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
    crate::tasks::source_unchanged(pool, message_id, source_path).await?;
    if source.size != transfer.bytes_total {
        return Err("source file size changed".to_string());
    }
    let file_sha256 = upload_plan
        .is_parallel()
        .then(|| recorded_file_sha256(pool, message_id, source.path.clone()));

    Ok(UploadJob {
        transfer_id: transfer.id.clone(),
        peer_id: transfer.peer_id.clone(),
        peer_addr: peer_addr.to_string(),
        conversation_id: conversation_id.to_string(),
        client_message_id: client_message_id.to_string(),
        message_id,
        source,
        group_sync: group_sync_message(&conversation, &members)?,
        upload_plan,
        concurrency,
        file_sha256,
    })
}

fn spawn_upload(pool: Pool<Sqlite>, job: UploadJob) {
    tokio::spawn(async move {
        run_upload(&pool, job).await;
    });
}

pub async fn recover_abandoned_uploads(pool: &Pool<Sqlite>) -> Result<(), String> {
    let messages = db::recover_abandoned_transfers(pool).await?;
    for id in messages {
        refresh_file_status(pool, id).await?;
        eprintln!("[ConversationFile] recovered message_id={id}");
    }
    Ok(())
}

struct UploadExecutionGuard {
    id: String,
    token: super::transfer::TransferCancellationToken,
}

impl Drop for UploadExecutionGuard {
    fn drop(&mut self) {
        self.token.store(true, Ordering::Release);
        cancellation_registry().complete_execution(&self.id, &self.token);
    }
}

async fn renew_upload_lease(
    pool: &Pool<Sqlite>,
    lease: &db::TransferLease,
    token: &super::transfer::TransferCancellationToken,
) -> Result<(), String> {
    match db::renew_transfer_lease(pool, lease).await? {
        Some(db::TransferLeaseState::Cancelling) => token.store(true, Ordering::Release),
        Some(db::TransferLeaseState::Active) => {}
        None => return Err("transfer execution lease expired or replaced".into()),
    }
    Ok(())
}

async fn run_upload(pool: &Pool<Sqlite>, mut job: UploadJob) {
    // Keep claim order and token replacement order identical within this process.
    // Otherwise a worker paused after its claim could install a stale token after
    // a replacement has acquired the expired lease, cancelling the new execution.
    let startup = UPLOAD_START_LOCK.lock().await;
    let lease = match db::claim_send_transfer(pool, &job.transfer_id).await {
        Ok(Some(lease)) => lease,
        Ok(None) => return,
        Err(error) => {
            eprintln!(
                "[ConversationFile] claim transfer_id={} error={error}",
                job.transfer_id
            );
            return;
        }
    };
    let token = cancellation_registry().register_execution(&job.transfer_id);
    let _guard = UploadExecutionGuard {
        id: job.transfer_id.clone(),
        token: token.clone(),
    };
    drop(startup);
    let transfer_id = job.transfer_id.clone();
    eprintln!(
        "[ConversationFile] claimed transfer_id={transfer_id} message_id={} lease={}",
        job.message_id, lease.token
    );
    let work = async {
        // Cancel may have committed between the claim and token registration, or
        // in another process. Carry the durable state into this execution's token.
        renew_upload_lease(pool, &lease, &token).await?;
        refresh_file_status(pool, job.message_id).await?;
        let outcome = upload_chunks(pool, &job, &token, &lease).await;
        // A stale worker must not cancel the receiver's newly resumed task either.
        renew_upload_lease(pool, &lease, &token).await?;
        // Drop our digest subscription promptly when preparation fails or is cancelled.
        job.file_sha256 = None;
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
        db::update_owned_transfer(pool, &lease, status, bytes, error.as_deref()).await?;
        refresh_file_status(pool, job.message_id).await
    };
    let heartbeat = async {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            renew_upload_lease(pool, &lease, &token).await?;
        }
    };
    let result = tokio::select! {
        result = work => result,
        result = heartbeat => result,
    };
    if let Err(error) = result {
        eprintln!("[ConversationFile] execution stopped transfer_id={transfer_id} error={error}");
    }
}

async fn post_remote_terminal(
    peer_addr: &str,
    expected_peer_id: &str,
    client_message_id: &str,
    transfer_id: &str,
    status: &str,
    peer_id: Option<&str>,
) -> Option<String> {
    if let Err(error) =
        super::peer_identity::require_peer_identity(peer_addr, expected_peer_id).await
    {
        eprintln!("[ConversationFile] 取消清理前核对设备身份失败: {error}");
        return None;
    }
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
        &job.peer_id,
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
        &transfer.peer_id,
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
    lease: &db::TransferLease,
) -> UploadOutcome {
    if token.load(Ordering::Acquire) {
        return UploadOutcome::Cancelled(0);
    }

    if let Err(error) =
        super::peer_identity::require_peer_identity(&job.peer_addr, &job.peer_id).await
    {
        return UploadOutcome::Failed(0, format!("发送文件前核对设备身份失败: {error}"));
    }

    if let Some(group_sync) = &job.group_sync {
        if let Err(error) =
            super::protocol::send_protocol_message(&job.peer_addr, &job.peer_id, group_sync).await
        {
            return UploadOutcome::Failed(0, format!("发送群同步失败: {error}"));
        }
    }
    if token.load(Ordering::Acquire) {
        return UploadOutcome::Cancelled(0);
    }
    if job.upload_plan.is_parallel() {
        return upload_parallel_chunks(pool, job, token, lease).await;
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

        // Reserve the stream before allocating the legacy 4 MiB buffer.
        let permit = match job.concurrency.acquire_for_peer(&job.peer_id, token).await {
            Ok(permit) => permit,
            Err(super::transfer::TransferPermitError::Cancelled) => {
                return UploadOutcome::Cancelled(bytes_transferred);
            }
            Err(super::transfer::TransferPermitError::Closed) => {
                return UploadOutcome::Failed(bytes_transferred, "文件传输并发控制器已关闭".to_string());
            }
        };
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

        let response = match await_with_transfer_cancellation(
            client.post(&upload_url).multipart(form).send(),
            token,
        )
        .await
        {
            Some(Ok(response)) => response,
            Some(Err(error)) => {
                if token.load(Ordering::Acquire) {
                    return UploadOutcome::Cancelled(bytes_transferred);
                }
                return UploadOutcome::Failed(bytes_transferred, format!("上传分块失败: {error}"));
            }
            None => return UploadOutcome::Cancelled(bytes_transferred),
        };
        let status = response.status();
        let body = match await_with_transfer_cancellation(response.text(), token).await {
            Some(body) => body.unwrap_or_default(),
            None => return UploadOutcome::Cancelled(bytes_transferred),
        };
        drop(permit);
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
        if let Err(error) = db::update_owned_transfer(
            pool,
            lease,
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
    lease: &db::TransferLease,
) -> UploadOutcome {
    let Some(hash) = job.file_sha256.clone() else {
        return UploadOutcome::Failed(0, "并行传输缺少文件摘要".to_string());
    };
    let Some(protocol_version) = job.upload_plan.manifest_version() else {
        return UploadOutcome::Failed(0, "并行传输协议无效".to_string());
    };
    let streaming = protocol_version == 4;
    let digest = await_with_transfer_cancellation(hash, token);
    tokio::pin!(digest);
    let file_sha256 = if streaming {
        String::new()
    } else {
        match digest.as_mut().await {
            Some(Ok(hash)) => hash,
            Some(Err(error)) => return UploadOutcome::Failed(0, error),
            None => return UploadOutcome::Cancelled(0),
        }
    };
    let file_size = job.source.size.max(0) as u64;
    let chunks = match job.upload_plan.protocol {
        UploadProtocol::FixedV2 => parallel_chunk_ranges(file_size),
        UploadProtocol::FlexibleV3 | UploadProtocol::StreamingV4 => {
            flexible_parallel_chunk_ranges(file_size, job.upload_plan.channels)
        }
        UploadProtocol::SequentialV1 => {
            return UploadOutcome::Failed(0, "并行传输协议无效".to_string());
        }
    };
    let sender_id = match db::get_user_id(pool).await {
        Ok(id) => id,
        Err(error) => return UploadOutcome::Failed(0, error),
    };
    let mut request = ParallelPrepareRequest {
        voice: match crate::voice::metadata(pool, &job.client_message_id).await {
            Ok(voice) => voice,
            Err(error) => return UploadOutcome::Failed(0, error),
        },
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
        .connect_timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return UploadOutcome::Failed(0, format!("创建并行上传客户端失败: {error}"));
        }
    };
    let base_url = format!("http://{}", job.peer_addr.trim_end_matches('/'));
    let prepared = match await_with_transfer_cancellation(
        post_parallel_request(
            &client,
            &format!("{base_url}/api/uploads/v{protocol_version}/prepare"),
            &request,
        ),
        token,
    )
    .await
    {
        Some(Ok(response)) => response,
        Some(Err(error)) => return UploadOutcome::Failed(0, error),
        None => return UploadOutcome::Cancelled(0),
    };
    if let Err(error) = validate_prepared_response(&request, &prepared) {
        return UploadOutcome::Failed(0, error);
    }
    if let Some(limit) = prepared.max_parallel_channels {
        job.concurrency.set_peer_limit(&job.peer_id, limit);
    }
    match prepared.status.as_str() {
        "awaiting_acceptance" => {
            return UploadOutcome::AwaitingAcceptance(prepared.received as i64)
        }
        "already_exists" | "completed" if !streaming => {
            return UploadOutcome::Completed(job.source.size);
        }
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
    if missing.is_empty() && !streaming {
        return UploadOutcome::Completed(job.source.size);
    }

    let progress = Arc::new(AtomicI64::new(prepared.received as i64));
    let prepare_request = request.clone();
    let mut uploads = stream::iter(missing)
        .map(|chunk| {
            upload_parallel_range(
                client.clone(),
                base_url.clone(),
                job.peer_id.clone(),
                protocol_version,
                job.transfer_id.clone(),
                job.source.path.clone(),
                chunk,
                prepare_request.clone(),
                progress.clone(),
                job.concurrency.clone(),
                token.clone(),
            )
        })
        .buffer_unordered(usize::from(job.upload_plan.channels));
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut streaming_digest = None;
    let mut persisted_bytes = -1;

    loop {
        tokio::select! {
            result = &mut digest, if streaming && streaming_digest.is_none() => {
                let bytes = progress.load(Ordering::Acquire).min(job.source.size);
                match result {
                    Some(Ok(hash)) => streaming_digest = Some(hash),
                    Some(Err(error)) => return UploadOutcome::Failed(bytes, error),
                    None => return UploadOutcome::Cancelled(bytes),
                }
            }
            _ = interval.tick() => {
                let bytes = progress.load(Ordering::Acquire).min(job.source.size);
                if token.load(Ordering::Acquire) {
                    return UploadOutcome::Cancelled(bytes);
                }
                if bytes == persisted_bytes { continue; }
                if let Err(error) = db::update_owned_transfer(
                    pool,
                    lease,
                    "transferring",
                    bytes,
                    None,
                ).await {
                    return UploadOutcome::Failed(bytes, error);
                }
                persisted_bytes = bytes;
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
        return UploadOutcome::Cancelled(bytes);
    }
    if bytes < job.source.size {
        return UploadOutcome::Failed(bytes, "并行上传未覆盖完整文件".to_string());
    }
    if streaming {
        request.file_sha256 = match streaming_digest {
            Some(hash) => hash,
            None => match digest.await {
                Some(Ok(hash)) => hash,
                Some(Err(error)) => return UploadOutcome::Failed(bytes, error),
                None => return UploadOutcome::Cancelled(bytes),
            },
        };
        // The receiver publishes the file only after checking this final digest.
        let completed = await_with_transfer_cancellation(
            post_parallel_request(
                &client,
                &format!("{base_url}/api/uploads/v4/complete"),
                &request,
            ),
            token,
        )
        .await;
        match completed {
            Some(Ok(response))
                if matches!(response.status.as_str(), "completed" | "already_exists") => {}
            Some(Ok(response)) => {
                return UploadOutcome::Failed(
                    bytes,
                    format!("接收端尚未确认完整文件: {}", response.status),
                );
            }
            Some(Err(error)) => return UploadOutcome::Failed(bytes, error),
            None => return UploadOutcome::Cancelled(bytes),
        }
    }
    UploadOutcome::Completed(job.source.size)
}

async fn post_parallel_request(
    client: &reqwest::Client,
    url: &str,
    request: &ParallelPrepareRequest,
) -> Result<ParallelPrepareResponse, String> {
    super::file_retry::post(client, url, request).await
}

fn validate_prepared_response(
    request: &ParallelPrepareRequest,
    response: &ParallelPrepareResponse,
) -> Result<(), String> {
    let missing: BTreeSet<_> = response.missing_chunks.iter().copied().collect();
    if missing.len() != response.missing_chunks.len()
        || response.max_parallel_channels.is_some_and(|limit| limit == 0 || limit > 16)
        || missing.iter().any(|index| *index >= request.chunks.len())
        || response.received > request.file_size
        || (response.status == "ready"
            && response.received
                != request
                    .chunks
                    .iter()
                    .filter(|chunk| !missing.contains(&chunk.index))
                    .map(|chunk| chunk.length)
                    .sum::<u64>())
    {
        return Err("接收端返回了无效的缺失分块或确认字节数".to_string());
    }
    Ok(())
}

async fn upload_parallel_range(
    client: reqwest::Client,
    base_url: String,
    peer_id: String,
    protocol_version: u8,
    transfer_id: String,
    source_path: String,
    chunk: ParallelChunkRange,
    prepare: ParallelPrepareRequest,
    progress: Arc<AtomicI64>,
    concurrency: super::transfer::TransferConcurrencyGeneration,
    token: super::transfer::TransferCancellationToken,
) -> Result<(), String> {
    use super::file_retry::{self, RequestError};
    use std::sync::atomic::{AtomicBool, AtomicU64};

    let operation = async {
        let mut last_error = String::new();
        for attempt in 0..file_retry::ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(file_retry::backoff(attempt - 1)).await;
                // A lost response does not mean the receiver lost the block.
                let state: ParallelPrepareResponse = match file_retry::post_once(
                    &client,
                    &format!("{base_url}/api/uploads/v{protocol_version}/prepare"),
                    &prepare,
                )
                .await
                {
                    Ok(state) => state,
                    Err(error) if error.retryable => {
                        last_error = error.detail;
                        continue;
                    }
                    Err(error) => return Err(error.detail),
                };
                validate_prepared_response(&prepare, &state)?;
                if let Some(limit) = state.max_parallel_channels {
                    concurrency.set_peer_limit(&peer_id, limit);
                }
                if matches!(state.status.as_str(), "completed" | "already_exists")
                    || (state.status == "ready" && !state.missing_chunks.contains(&chunk.index))
                {
                    progress.fetch_add(chunk.length as i64, Ordering::AcqRel);
                    return Ok(());
                }
                if state.status != "ready" {
                    return Err(format!("接收尝试不可继续: {}", state.status));
                }
            }
            let permit = concurrency
                .acquire_for_peer(&peer_id, &token)
                .await
                .map_err(|_| "transfer cancelled".to_string())?;
            let mut file = tokio::fs::File::open(&source_path)
                .await
                .map_err(|error| format!("打开并行上传源文件失败: {error}"))?;
            if file
                .metadata()
                .await
                .map_err(|error| error.to_string())?
                .len()
                != prepare.file_size
            {
                return Err("源文件大小在传输过程中发生变化".to_string());
            }
            file.seek(SeekFrom::Start(chunk.offset))
                .await
                .map_err(|error| format!("定位并行上传分块失败: {error}"))?;
            let read_bytes = Arc::new(AtomicU64::new(0));
            let source_error = Arc::new(AtomicBool::new(false));
            let stream_bytes = read_bytes.clone();
            let stream_error = source_error.clone();
            let stream =
                ReaderStream::with_capacity(file.take(chunk.length), PARALLEL_STREAM_BUFFER).map(
                    move |result| {
                        match &result {
                            Ok(bytes) => {
                                stream_bytes.fetch_add(bytes.len() as u64, Ordering::AcqRel);
                            }
                            Err(_) => {
                                stream_error.store(true, Ordering::Release);
                            }
                        }
                        result
                    },
                );
            let url = format!(
                "{base_url}/api/uploads/v{protocol_version}/{}/{}",
                urlencoding::encode(&transfer_id),
                chunk.index
            );
            let send = async {
                let response = client
                    .post(url)
                    .header(reqwest::header::CONTENT_LENGTH, chunk.length)
                    .body(reqwest::Body::wrap_stream(stream))
                    .send()
                    .await
                    .map_err(RequestError::transport)?;
                let response: serde_json::Value = file_retry::decode(response).await?;
                if !matches!(
                    response.get("status").and_then(|v| v.as_str()),
                    Some("receiving" | "completed" | "already_exists")
                ) {
                    return Err(RequestError::permanent("并行分块返回未知状态"));
                }
                Ok(())
            };
            tokio::pin!(send);
            let mut observed = 0;
            let mut last_progress = Instant::now();
            let result = loop {
                tokio::select! {
                    result = &mut send => break result,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        let current = read_bytes.load(Ordering::Acquire);
                        if current != observed { observed = current; last_progress = Instant::now(); }
                        let deadline = if current == chunk.length { file_retry::RESPONSE_TIMEOUT } else { file_retry::IDLE_TIMEOUT };
                        if last_progress.elapsed() >= deadline { break Err(RequestError::transient("分块传输无进展超时")); }
                    }
                }
            };
            drop(permit);
            match result {
                Ok(()) => {
                    // Persist only confirmed bytes, never bytes merely read by reqwest.
                    progress.fetch_add(chunk.length as i64, Ordering::AcqRel);
                    return Ok(());
                }
                Err(error) if error.retryable && !source_error.load(Ordering::Acquire) => {
                    last_error = error.detail
                }
                Err(error) => return Err(error.detail),
            }
        }
        Err(format!("分块重试达到上限: {last_error}"))
    };
    await_with_transfer_cancellation(
        tokio::time::timeout(Duration::from_secs(3600), operation),
        &token,
    )
    .await
    .ok_or_else(|| "transfer cancelled".to_string())?
    .map_err(|_| "分块传输达到总时限".to_string())?
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

pub(crate) fn flexible_parallel_chunk_ranges(
    file_size: u64,
    channels: u8,
) -> Vec<ParallelChunkRange> {
    if file_size <= CHUNK_SIZE as u64 {
        return vec![ParallelChunkRange {
            index: 0,
            offset: 0,
            length: file_size,
        }];
    }

    let parts_for_size = file_size / CHUNK_SIZE as u64
        + u64::from(file_size % CHUNK_SIZE as u64 != 0);
    let parts_for_fairness = usize::from(channels.max(1)) * PARALLEL_PARTS_PER_CHANNEL;
    let part_count = parts_for_size
        .max(parts_for_fairness as u64)
        .min(MAX_PARALLEL_PARTS as u64)
        .min(file_size) as usize;
    let base = file_size / part_count as u64;
    let remainder = file_size % part_count as u64;
    let mut offset = 0;
    (0..part_count)
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

pub(crate) fn valid_flexible_parallel_chunks(
    file_size: u64,
    chunks: &[ParallelChunkRange],
) -> bool {
    if file_size == 0 {
        return chunks
            == [ParallelChunkRange {
                index: 0,
                offset: 0,
                length: 0,
            }];
    }
    if chunks.is_empty() || chunks.len() > MAX_PARALLEL_PARTS {
        return false;
    }

    let mut expected_offset = 0u64;
    for (expected_index, chunk) in chunks.iter().enumerate() {
        if chunk.index != expected_index || chunk.offset != expected_offset || chunk.length == 0 {
            return false;
        }
        let Some(next_offset) = expected_offset.checked_add(chunk.length) else {
            return false;
        };
        if next_offset > file_size {
            return false;
        }
        expected_offset = next_offset;
    }
    expected_offset == file_size
}

pub(crate) fn valid_parallel_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn valid_parallel_prepare(request: &ParallelPrepareRequest, version: u8) -> bool {
    let chunks_valid = match version {
        2 => request.chunks == parallel_chunk_ranges(request.file_size),
        3 | 4 => valid_flexible_parallel_chunks(request.file_size, &request.chunks),
        _ => false,
    };
    !request.sender_id.trim().is_empty()
        && !request.conversation_id.trim().is_empty()
        && !request.client_message_id.trim().is_empty()
        && request.client_message_id.len() <= 128
        && !request.transfer_id.trim().is_empty()
        && request.transfer_id.len() <= 256
        && !request.sender_msg_id.trim().is_empty()
        && request.sender_msg_id.len() <= 64
        && (valid_parallel_sha256(&request.file_sha256)
            || (version == 4 && request.file_sha256.is_empty()))
        && chunks_valid
        && request.voice.as_ref().is_none_or(|voice| version == 4 && voice.validate(request.file_size).is_ok())
}

fn valid_parallel_manifest(manifest: &ParallelTransferManifest, transfer_id: &str) -> bool {
    manifest.transfer_id == transfer_id
        && (valid_parallel_sha256(&manifest.file_sha256)
            || (manifest.version == 4 && manifest.file_sha256.is_empty()))
        && match manifest.version {
            2 => manifest.chunks == parallel_chunk_ranges(manifest.file_size),
            3 | 4 => valid_flexible_parallel_chunks(manifest.file_size, &manifest.chunks),
            _ => false,
        }
}

pub(crate) fn parallel_manifests_match(
    existing: &ParallelTransferManifest,
    requested: &ParallelTransferManifest,
) -> bool {
    if existing == requested {
        return true;
    }
    // V4 supplies its digest at completion; all identity and range fields stay fixed.
    if existing.version == 4 && existing.file_sha256.is_empty() {
        let mut pending = requested.clone();
        pending.file_sha256.clear();
        return existing == &pending;
    }
    false
}

#[cfg(test)]
fn deferred_file_sha256(path: String) -> FileSha256 {
    async move { sha256_file(Path::new(&path)).await }
        .boxed()
        .shared()
}

fn recorded_file_sha256(pool: &Pool<Sqlite>, message_id: i64, path: String) -> FileSha256 {
    let pool = pool.clone();
    async move {
        let digest = sha256_file(Path::new(&path)).await?;
        crate::tasks::pin_digest(&pool, message_id, &path, &digest).await?;
        Ok(digest)
    }.boxed().shared()
}

pub(crate) async fn sha256_file(path: &Path) -> Result<String, String> {
    let permit = super::transfer::digest_permit().await?;
    let path = path.to_path_buf();
    // Dropping the last recipient's future stops the blocking reader between buffers.
    let (_cancel_on_drop, mut cancellation) = tokio::sync::oneshot::channel::<()>();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        use std::io::Read;

        let mut file =
            std::fs::File::open(path).map_err(|error| format!("打开文件摘要源失败: {error}"))?;
        let mut digest = Sha256::new();
        let mut buffer = vec![0; PARALLEL_STREAM_BUFFER];
        loop {
            if cancellation.try_recv() == Err(tokio::sync::oneshot::error::TryRecvError::Closed) {
                return Err("文件摘要计算已取消".to_string());
            }
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("读取文件摘要源失败: {error}"))?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        Ok(sha256_hex(&digest.finalize()))
    })
    .await
    .map_err(|error| format!("文件摘要任务失败: {error}"))?
}

fn parallel_transfer_key(transfer_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(transfer_id.as_bytes());
    sha256_hex(&digest.finalize())
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
    if !valid_parallel_manifest(&manifest, transfer_id) {
        return Err("并行传输清单无效".to_string());
    }
    Ok(Some(manifest))
}

pub(crate) async fn create_or_resume_parallel_manifest(
    download_root: &Path,
    manifest: ParallelTransferManifest,
) -> Result<(ParallelTransferManifest, Vec<usize>, u64), String> {
    if !valid_parallel_manifest(&manifest, &manifest.transfer_id) {
        return Err("并行传输清单无效".to_string());
    }
    if let Some(existing) = load_parallel_manifest(download_root, &manifest.transfer_id).await? {
        if !parallel_manifests_match(&existing, &manifest) {
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
        return Ok((manifest, missing, bytes));
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
    let _receive_permit = super::transfer::receive_permit(&manifest.sender_id).await?;
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
    let _disk_reservation = super::transfer::reserve_disk(download_root, chunk.length).await?;
    let temporary = directory.join(format!(".{chunk_index:06}-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|error| format!("创建并行分块临时文件失败: {error}"))?;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    let mut reported = 0i64;
    let mut pending = 0i64;
    let progress_window = receive_progress_window(transfer_id);
    loop {
        let data = match tokio::time::timeout(super::file_retry::IDLE_TIMEOUT, stream.next()).await
        {
            Ok(Some(data)) => data,
            Ok(None) => break,
            Err(_) => {
                drop(file);
                return Err(fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    "读取并行分块无进展超时".to_string(),
                )
                .await);
            }
        };
        let data = match data {
            Ok(data) => data,
            Err(error) => {
                drop(file);
                return Err(fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    format!("读取并行分块请求失败: {error}"),
                )
                .await);
            }
        };
        written = written.saturating_add(data.len() as u64);
        if written > chunk.length {
            drop(file);
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                "并行分块超过声明长度".to_string(),
            )
            .await);
        }
        if let Err(error) = file.write_all(&data).await {
            drop(file);
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                format!("写入并行分块失败: {error}"),
            )
            .await);
        }
        pending += data.len() as i64;
        if take_progress_window(&progress_window, Instant::now()) {
            if let Err(error) = adjust_transfer_progress(pool, transfer_id, pending).await {
                drop(file);
                return Err(fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    error,
                )
                .await);
            }
            reported += pending;
            pending = 0;
        }
    }
    if pending > 0 {
        if let Err(error) = adjust_transfer_progress(pool, transfer_id, pending).await {
            drop(file);
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                error,
            )
            .await);
        }
        reported += pending;
    }
    if written != chunk.length {
        drop(file);
        return Err(fail_parallel_chunk_attempt(
            pool,
            transfer_id,
            &temporary,
            None,
            reported,
            "并行分块长度与清单不一致".to_string(),
        )
        .await);
    }
    if let Err(error) = file.flush().await {
        drop(file);
        return Err(fail_parallel_chunk_attempt(
            pool,
            transfer_id,
            &temporary,
            None,
            reported,
            format!("保存并行分块失败: {error}"),
        )
        .await);
    }
    drop(file);

    let _guard = lock_receive_file(&manifest.client_message_id).await;
    let transfer = match db::get_transfer(pool, transfer_id).await {
        Ok(Some(transfer)) => transfer,
        Ok(None) => {
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                "并行接收传输不存在".to_string(),
            )
            .await)
        }
        Err(error) => {
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                error,
            )
            .await)
        }
    };
    if transfer.status != "transferring" {
        return Err(fail_parallel_chunk_attempt(
            pool,
            transfer_id,
            &temporary,
            None,
            reported,
            "并行接收传输已结束".to_string(),
        )
        .await);
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
                return Err(fail_parallel_chunk_attempt(
                    pool,
                    transfer_id,
                    &temporary,
                    None,
                    reported,
                    format!("替换无效并行分块失败: {error}"),
                )
                .await);
            }
        }
        if let Err(error) = tokio::fs::rename(&temporary, &existing_path).await {
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                None,
                reported,
                format!("发布并行分块失败: {error}"),
            )
            .await);
        }
        published = true;
    }
    let received = match received_parallel_chunks(download_root, &manifest).await {
        Ok(received) => received,
        Err(error) => {
            return Err(fail_parallel_chunk_attempt(
                pool,
                transfer_id,
                &temporary,
                published.then_some(existing_path.as_path()),
                reported,
                error,
            )
            .await)
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
        return Err(fail_parallel_chunk_attempt(
            pool,
            transfer_id,
            &temporary,
            published.then_some(existing_path.as_path()),
            reported,
            format!("校正并行传输进度失败: {error}"),
        )
        .await);
    }
    Ok(ParallelChunkReceiveResult {
        complete: received.len() == manifest.chunks.len(),
        manifest,
        received: bytes,
    })
}

// Own only this attempt's staging file. A cancelled blocking task must never
// truncate or remove another attempt's output, even after its caller is dropped.
struct PendingMergedFile {
    path: PathBuf,
    file: Option<std::fs::File>,
}

impl Drop for PendingMergedFile {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

fn copy_and_hash_parallel_parts(
    download_root: &Path,
    manifest: &ParallelTransferManifest,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<(PendingMergedFile, String), String> {
    use std::io::{Read, Write};

    let path = parallel_transfer_dir(download_root, &manifest.transfer_id)
        .join(format!("merging-{}.tmp", uuid::Uuid::new_v4()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("创建并行合并文件失败: {error}"))?;
    let mut pending = PendingMergedFile {
        path,
        file: Some(file),
    };
    let mut digest = Sha256::new();
    let mut buffer = vec![0; FILE_MERGE_BUFFER];
    let mut total = 0u64;
    for chunk in &manifest.chunks {
        let path = parallel_part_path(download_root, &manifest.transfer_id, chunk.index);
        let mut part =
            std::fs::File::open(path).map_err(|error| format!("打开并行分块失败: {error}"))?;
        let mut copied = 0u64;
        loop {
            if is_cancelled() {
                return Err("文件合并已取消".to_string());
            }
            let read = part
                .read(&mut buffer)
                .map_err(|error| format!("读取并行分块失败: {error}"))?;
            if read == 0 {
                break;
            }
            copied += read as u64;
            if copied > chunk.length {
                return Err("并行分块长度在合并前发生变化".to_string());
            }
            let bytes = &buffer[..read];
            pending
                .file
                .as_mut()
                .unwrap()
                .write_all(bytes)
                .map_err(|error| format!("合并并行分块失败: {error}"))?;
            // Hash the bytes being assembled instead of reading the whole file again.
            digest.update(bytes);
        }
        if copied != chunk.length {
            return Err("并行分块长度在合并前发生变化".to_string());
        }
        total += copied;
    }
    if total != manifest.file_size {
        return Err("并行合并文件大小不一致".to_string());
    }
    drop(pending.file.take());
    Ok((pending, sha256_hex(&digest.finalize())))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("writing to a String cannot fail");
    }
    result
}

pub(crate) async fn merge_parallel_parts(
    download_root: &Path,
    manifest: &ParallelTransferManifest,
    mut on_phase: impl FnMut(ReceiveProcessingPhase),
) -> Result<PathBuf, String> {
    if !valid_parallel_sha256(&manifest.file_sha256) {
        return Err("并行传输缺少完整文件摘要".to_string());
    }
    let permit = super::transfer::merge_permit().await?;
    let disk_reservation = super::transfer::reserve_disk(download_root, manifest.file_size).await?;
    on_phase(ReceiveProcessingPhase::Merging);
    let partial_path = received_partial_path(download_root, &manifest.transfer_id);
    let root = download_root.to_path_buf();
    let job = manifest.clone();
    let (_cancel_on_drop, mut cancellation) = tokio::sync::oneshot::channel::<()>();
    let (pending, digest) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _disk_reservation = disk_reservation;
        copy_and_hash_parallel_parts(&root, &job, || {
            cancellation.try_recv() == Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        })
    })
    .await
    .map_err(|error| format!("文件合并任务失败: {error}"))??;
    on_phase(ReceiveProcessingPhase::Verifying);
    if digest != manifest.file_sha256 {
        drop(pending);
        cleanup_parallel_transfer(download_root, &manifest.transfer_id).await?;
        return Err("并行合并文件 SHA-256 校验失败".to_string());
    }
    tokio::fs::rename(&pending.path, &partial_path)
        .await
        .map_err(|error| format!("保存并行合并文件失败: {error}"))?;
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
    fn phase2_progress_is_coalesced_across_chunks_of_one_transfer() {
        let first = receive_progress_window("phase2-progress-window");
        let second = receive_progress_window("phase2-progress-window");
        assert!(Arc::ptr_eq(&first, &second));
        let start = Instant::now();
        *first.lock().unwrap() = start;
        let mut writes = 0;
        for tick in 0..2000 {
            for window in [&first, &second] {
                writes += usize::from(take_progress_window(
                    window,
                    start + Duration::from_millis(tick),
                ));
            }
        }
        assert_eq!(writes, 3, "4000 reports must share the same 500 ms window");
    }

    #[tokio::test]
    async fn phase2_chunk_retry_confirms_commits_and_never_double_counts() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        // busy then success; committed but response lost; permanently busy; invalid;
        // cancellation during the backoff. Every case uses a real loopback server.
        for mode in 0..5 {
            let calls = Arc::new(AtomicUsize::new(0));
            let queries = Arc::new(AtomicUsize::new(0));
            let token = Arc::new(AtomicBool::new(false));
            let count = calls.clone();
            let cancelled = token.clone();
            let query_count = queries.clone();
            let router = axum::Router::new().route("/api/uploads/v4/test/0", axum::routing::post(move |request: axum::extract::Request| {
                let count = count.clone();
                let cancelled = cancelled.clone();
                async move {
                    let bytes = axum::body::to_bytes(request.into_body(), 1024).await.unwrap();
                    assert_eq!(bytes.as_ref(), b"payload");
                    let attempt = count.fetch_add(1, Ordering::SeqCst);
                    if mode == 4 { cancelled.store(true, Ordering::Release); }
                    let status = if mode == 3 { 409 } else if mode == 0 && attempt > 0 { 200 } else { 503 };
                    (axum::http::StatusCode::from_u16(status).unwrap(), axum::Json(serde_json::json!({"status": "receiving"})))
                }
            })).route("/api/uploads/v4/prepare", axum::routing::post(move || {
                query_count.fetch_add(1, Ordering::SeqCst);
                async move { axum::Json(serde_json::json!({"status":"ready", "received":if mode == 1 {7} else {0}, "missing_chunks":if mode == 1 {vec![]} else {vec![0]}})) }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let path =
                std::env::temp_dir().join(format!("xchat-phase2-retry-{}", uuid::Uuid::new_v4()));
            tokio::fs::write(&path, b"payload").await.unwrap();
            let chunk = ParallelChunkRange {
                index: 0,
                offset: 0,
                length: 7,
            };
            let prepare = ParallelPrepareRequest {
                voice: None,
                sender_id: "sender".into(),
                conversation_id: "direct".into(),
                client_message_id: "message".into(),
                transfer_id: "test".into(),
                sender_msg_id: "1".into(),
                file_name: "file.bin".into(),
                file_size: 7,
                file_sha256: String::new(),
                chunks: vec![chunk.clone()],
            };
            let progress = Arc::new(AtomicI64::new(0));
            let result = upload_parallel_range(
                reqwest::Client::new(),
                base,
                "peer".into(),
                4,
                "test".into(),
                path.to_string_lossy().into_owned(),
                chunk,
                prepare,
                progress.clone(),
                super::super::transfer::TransferConcurrencyController::default()
                    .generation(4)
                    .unwrap(),
                token,
            )
            .await;
            assert_eq!(result.is_ok(), mode < 2, "mode {mode}: {result:?}");
            assert_eq!(
                progress.load(Ordering::Acquire),
                if mode < 2 { 7 } else { 0 }
            );
            assert_eq!(calls.load(Ordering::SeqCst), [2, 1, 3, 1, 1][mode]);
            assert_eq!(queries.load(Ordering::SeqCst), [1, 1, 2, 0, 0][mode]);
            server.abort();
            tokio::fs::remove_file(path).await.unwrap();
        }
    }

    async fn claim_test_upload(pool: &Pool<Sqlite>, id: &str) -> db::TransferLease {
        sqlx::query("UPDATE transfers SET status = 'queued' WHERE id = ?")
            .bind(id).execute(pool).await.unwrap();
        db::claim_send_transfer(pool, id).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn qa_cancel_between_claim_and_registration_prevents_upload() {
        use std::sync::atomic::AtomicUsize;
        let uploaded = Arc::new(AtomicUsize::new(0));
        let requests = uploaded.clone();
        let router = axum::Router::new()
            .route(
                "/api/peer_identity",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({ "device_id": "peer-a", "name": "Alice" }))
                }),
            )
            .route(
                "/api/upload",
                axum::routing::post(move |request: axum::extract::Request| {
                    let requests = requests.clone();
                    async move {
                        let _ = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                            .await
                            .unwrap();
                        requests.fetch_add(1, Ordering::SeqCst);
                        axum::Json(serde_json::json!({ "status": "completed" }))
                    }
                }),
            )
            .route(
                "/api/uploads/{id}/cancel",
                axum::routing::post(|| async {
                    axum::Json(serde_json::json!({ "status": "cancelled" }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let app_dir =
            std::env::temp_dir().join(format!("xchat-qa-cancel-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let source = app_dir.join("payload.bin");
        tokio::fs::write(&source, b"payload").await.unwrap();
        let sent = send_path(
            &pool,
            &PeerManager::new(),
            &conversation.id,
            source.to_str().unwrap(),
        )
        .await
        .unwrap();
        let job = prepare_resume_job(
            &pool,
            &sent.transfers[0],
            &address.to_string(),
            UploadPlan::new(UploadProtocol::SequentialV1, 1),
            super::super::transfer::TransferConcurrencyController::default()
                .generation(4)
                .unwrap(),
        )
        .await
        .unwrap();
        // Persist the same state as a cancel arriving after the atomic claim, before
        // the worker has installed its in-memory cancellation handle.
        sqlx::query(
            "CREATE TRIGGER cancel_on_claim AFTER UPDATE OF status ON transfers
            WHEN NEW.status = 'transferring' AND OLD.status = 'waiting_peer'
            BEGIN UPDATE transfers SET status = 'cancelling' WHERE id = NEW.id; END",
        )
        .execute(&pool)
        .await
        .unwrap();
        run_upload(&pool, job).await;
        let result = db::get_transfer(&pool, &sent.transfers[0].id)
            .await
            .unwrap()
            .unwrap();
        server.abort();
        let _ = server.await;
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
        assert_eq!(
            uploaded.load(Ordering::SeqCst),
            0,
            "a persisted cancellation must prevent payload IO"
        );
        assert_eq!(result.status, "cancelled");
    }

    #[tokio::test]
    async fn qa_automatic_recovery_keeps_compatible_parallel_layout() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-qa-layout-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let source = app_dir.join("source.bin");
        tokio::fs::write(&source, b"payload").await.unwrap();
        let peers = PeerManager::new();
        let sent = send_path(&pool, &peers, &conversation.id, source.to_str().unwrap())
            .await
            .unwrap();
        let old_plan = UploadPlan::new(UploadProtocol::FlexibleV3, 8);
        let old_id = new_transfer_id_for_plan(
            sent.message.client_message_id.as_deref().unwrap(),
            "peer-a",
            old_plan,
            false,
        );
        sqlx::query("UPDATE transfers SET id = ?, status = 'transferring', bytes_transferred = 3 WHERE id = ?")
            .bind(&old_id).bind(&sent.transfers[0].id).execute(&pool).await.unwrap();
        recover_abandoned_uploads(&pool).await.unwrap();
        // The receiver still supports eight V3 parts, while current preferences
        // would negotiate sixteen V4 parts for a brand new upload.
        super::super::transfer::save_max_parallel_channels(&pool, 16)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            axum::serve(listener, axum::Router::new()).await.unwrap();
        });
        peers.add_or_update_with_details(
            "peer-a".into(),
            "Alice".into(),
            address.clone(),
            0,
            None,
            None,
            None,
            vec![
                PARALLEL_FILE_V3_CAPABILITY.into(),
                PARALLEL_FILE_V4_CAPABILITY.into(),
            ],
            None,
            true,
        );
        resume_waiting_for_peer(&pool, &peers, "peer-a", &address)
            .await
            .unwrap();
        let retained = db::get_transfer(&pool, &old_id).await.unwrap();
        // Let the intentionally identity-less mock reject the worker before cleanup.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let active: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM transfers WHERE status IN ('queued', 'transferring')",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                let status: Option<String> =
                    sqlx::query_scalar("SELECT file_status FROM messages WHERE id = ?")
                        .bind(sent.message.id)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                if active == 0 && status.as_deref() == Some("failed") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        server.abort();
        let _ = server.await;
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
        let retained =
            retained.expect("automatic recovery must preserve a supported manifest identity");
        assert_eq!(retained.bytes_transferred, 3);
    }

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

    #[test]
    fn upload_protocol_negotiation_preserves_v1_v2_and_bounds_v3() {
        assert_eq!(
            negotiate_upload_plan(&[], 16),
            UploadPlan::new(UploadProtocol::SequentialV1, 1)
        );
        assert_eq!(
            negotiate_upload_plan(&[PARALLEL_FILE_CAPABILITY.into()], 16),
            UploadPlan::new(UploadProtocol::FixedV2, 4)
        );
        assert_eq!(
            negotiate_upload_plan(&[PARALLEL_FILE_V3_CAPABILITY.into()], 8),
            UploadPlan::new(UploadProtocol::FlexibleV3, 8)
        );
        assert_eq!(
            negotiate_upload_plan(&["parallel_file_v3:4".into()], 16),
            UploadPlan::new(UploadProtocol::FlexibleV3, 4)
        );
        assert_eq!(
            negotiate_upload_plan(
                &[
                    PARALLEL_FILE_V3_CAPABILITY.into(),
                    PARALLEL_FILE_V4_CAPABILITY.into()
                ],
                8,
            ),
            UploadPlan::new(UploadProtocol::StreamingV4, 8)
        );
        assert_eq!(
            negotiate_upload_plan(&["parallel_file_v4:4".into()], 16),
            UploadPlan::new(UploadProtocol::StreamingV4, 4)
        );
        assert_eq!(
            negotiate_upload_plan(
                &[
                    PARALLEL_FILE_CAPABILITY.into(),
                    "parallel_file_v3:not-a-number".into(),
                    "parallel_file_v3:99".into(),
                    "parallel_file_v4:99".into(),
                ],
                16,
            ),
            UploadPlan::new(UploadProtocol::FixedV2, 4)
        );
    }

    #[test]
    fn v3_transfer_ids_only_resume_the_same_manifest_layout() {
        let v2 = UploadPlan::new(UploadProtocol::FixedV2, 4);
        let v3_eight = UploadPlan::new(UploadProtocol::FlexibleV3, 8);
        let v3_sixteen = UploadPlan::new(UploadProtocol::FlexibleV3, 16);
        let base = recipient_transfer_id("message-1", "peer-1");

        assert_eq!(
            new_transfer_id_for_plan("message-1", "peer-1", v2, false),
            base
        );
        let v3_id = new_transfer_id_for_plan("message-1", "peer-1", v3_eight, false);
        assert!(v3_id.starts_with(&format!("{base}:retry:v3c8-")));
        assert!(transfer_id_matches_upload_plan(&v3_id, v3_eight));
        assert!(!transfer_id_matches_upload_plan(&v3_id, v3_sixteen));
        assert!(!transfer_id_matches_upload_plan(&v3_id, v2));
        assert!(transfer_id_matches_upload_plan(&base, v2));
        let v4 = UploadPlan::new(UploadProtocol::StreamingV4, 8);
        let v4_id = new_transfer_id_for_plan("message-1", "peer-1", v4, false);
        assert!(v4_id.starts_with(&format!("{base}:retry:v4c8-")));
        assert!(transfer_id_matches_upload_plan(&v4_id, v4));
        assert!(!transfer_id_matches_upload_plan(&v4_id, v3_eight));
        assert!(!transfer_id_matches_upload_plan(&v3_id, v4));
        assert!(!transfer_id_matches_upload_plan(&v4_id, v2));
    }

    #[test]
    fn resume_keeps_existing_v2_or_v3_layout_after_capability_or_setting_changes() {
        let capabilities = vec![
            PARALLEL_FILE_CAPABILITY.to_string(),
            PARALLEL_FILE_V3_CAPABILITY.to_string(),
            PARALLEL_FILE_V4_CAPABILITY.to_string(),
        ];
        let legacy_v2 = recipient_transfer_id("message-1", "peer-1");
        assert_eq!(
            upload_plan_for_resume(&legacy_v2, &capabilities, 16),
            UploadPlan::new(UploadProtocol::FixedV2, 4)
        );

        let v3_eight = new_transfer_id_for_plan(
            "message-2",
            "peer-1",
            UploadPlan::new(UploadProtocol::FlexibleV3, 8),
            false,
        );
        assert_eq!(
            upload_plan_for_resume(&v3_eight, &capabilities, 16),
            UploadPlan::new(UploadProtocol::FlexibleV3, 8)
        );
        let v4 = UploadPlan::new(UploadProtocol::StreamingV4, 8);
        let v4_id = new_transfer_id_for_plan("message-3", "peer-1", v4, false);
        assert_eq!(upload_plan_for_resume(&v4_id, &capabilities, 4), v4);
        assert_eq!(
            upload_plan_for_resume(&v4_id, &[PARALLEL_FILE_V3_CAPABILITY.into()], 4),
            UploadPlan::new(UploadProtocol::FlexibleV3, 4)
        );
    }

    #[test]
    fn v3_ranges_are_bounded_contiguous_and_cover_the_file() {
        for channels in [4, 8, 16] {
            for size in [
                0,
                1,
                CHUNK_SIZE as u64,
                CHUNK_SIZE as u64 + 1,
                CHUNK_SIZE as u64 * 17 + 31,
            ] {
                let ranges = flexible_parallel_chunk_ranges(size, channels);
                assert!(valid_flexible_parallel_chunks(size, &ranges));
                assert!(ranges.len() <= MAX_PARALLEL_PARTS);
                assert_eq!(ranges.first().unwrap().offset, 0);
                assert_eq!(
                    ranges.iter().map(|range| range.length).sum::<u64>(),
                    size
                );
                if size > CHUNK_SIZE as u64 {
                    assert!(ranges.len() >= usize::from(channels) * 4);
                }
            }
        }
    }

    #[test]
    fn v3_chunk_validation_rejects_unsafe_layouts_and_v2_remains_fixed() {
        let size = CHUNK_SIZE as u64 + 17;
        let valid = flexible_parallel_chunk_ranges(size, 8);
        assert!(valid_flexible_parallel_chunks(size, &valid));

        let mut gap = valid.clone();
        gap[1].offset += 1;
        assert!(!valid_flexible_parallel_chunks(size, &gap));

        let mut overlap = valid.clone();
        overlap[1].offset -= 1;
        assert!(!valid_flexible_parallel_chunks(size, &overlap));

        let mut duplicate_index = valid.clone();
        duplicate_index[1].index = 0;
        assert!(!valid_flexible_parallel_chunks(size, &duplicate_index));

        let mut zero_length = valid.clone();
        zero_length[0].length = 0;
        assert!(!valid_flexible_parallel_chunks(size, &zero_length));

        assert!(!valid_flexible_parallel_chunks(
            u64::MAX,
            &[ParallelChunkRange {
                index: 0,
                offset: u64::MAX,
                length: 2,
            }],
        ));
        assert!(!valid_flexible_parallel_chunks(
            MAX_PARALLEL_PARTS as u64 + 1,
            &(0..=MAX_PARALLEL_PARTS)
                .map(|index| ParallelChunkRange {
                    index,
                    offset: index as u64,
                    length: 1,
                })
                .collect::<Vec<_>>(),
        ));

        let request = ParallelPrepareRequest {
            voice: None,
            sender_id: "sender".into(),
            conversation_id: "conversation".into(),
            client_message_id: "message".into(),
            transfer_id: "message:receiver".into(),
            sender_msg_id: "42".into(),
            file_name: "source.bin".into(),
            file_size: size,
            file_sha256: "0".repeat(64),
            chunks: valid,
        };
        assert!(!valid_parallel_prepare(&request, 2));
        assert!(valid_parallel_prepare(&request, 3));

        let mut v2 = request;
        v2.chunks = parallel_chunk_ranges(size);
        assert!(valid_parallel_prepare(&v2, 2));
        assert!(valid_parallel_prepare(&v2, 3));
        v2.file_sha256.clear();
        assert!(!valid_parallel_prepare(&v2, 2));
        assert!(!valid_parallel_prepare(&v2, 3));
        assert!(valid_parallel_prepare(&v2, 4));
        v2.file_sha256 = "invalid".into();
        assert!(!valid_parallel_prepare(&v2, 4));
    }

    #[tokio::test]
    async fn v3_manifest_loads_but_invalid_flexible_layout_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "xchat-parallel-v3-manifest-test-{}",
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let transfer_id = "message:receiver";
        let directory = parallel_transfer_dir(&root, transfer_id);
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let mut manifest = ParallelTransferManifest {
            version: 3,
            sender_id: "sender".into(),
            conversation_id: "conversation".into(),
            client_message_id: "message".into(),
            transfer_id: transfer_id.into(),
            sender_msg_id: "42".into(),
            file_name: "source.bin".into(),
            final_file_name: "source.bin".into(),
            file_size: CHUNK_SIZE as u64 + 17,
            file_sha256: "0".repeat(64),
            chunks: flexible_parallel_chunk_ranges(CHUNK_SIZE as u64 + 17, 8),
            message_id: 7,
        };
        tokio::fs::write(
            parallel_manifest_path(&root, transfer_id),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(
            load_parallel_manifest(&root, transfer_id)
                .await
                .unwrap()
                .unwrap(),
            manifest
        );

        manifest.chunks[1].offset += 1;
        tokio::fs::write(
            parallel_manifest_path(&root, transfer_id),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .await
        .unwrap();
        assert!(load_parallel_manifest(&root, transfer_id).await.is_err());

        tokio::fs::remove_dir_all(root).await.unwrap();
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

        let (_, missing, received) = create_or_resume_parallel_manifest(&root, manifest.clone())
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
        let (_, missing, received) = create_or_resume_parallel_manifest(&root, manifest.clone())
            .await
            .unwrap();
        assert!(missing.is_empty());
        assert_eq!(received, size as u64);

        let mut phases = Vec::new();
        let merged = merge_parallel_parts(&root, &manifest, |phase| phases.push(phase))
            .await
            .unwrap();
        assert_eq!(
            phases,
            [
                ReceiveProcessingPhase::Merging,
                ReceiveProcessingPhase::Verifying
            ]
        );
        assert_eq!(tokio::fs::read(&merged).await.unwrap(), data);
        cleanup_parallel_transfer(&root, &manifest.transfer_id)
            .await
            .unwrap();
        tokio::fs::remove_file(merged).await.unwrap();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn single_pass_merge_cleans_failed_attempts_and_preserves_retry_output() {
        let root = std::env::temp_dir().join(format!("xchat-single-pass-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let data: Vec<u8> = (0..(2 * FILE_MERGE_BUFFER + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let manifest = ParallelTransferManifest {
            version: 4,
            sender_id: "sender".into(),
            conversation_id: "conversation".into(),
            client_message_id: "single-pass-message".into(),
            transfer_id: "single-pass-transfer".into(),
            sender_msg_id: "1".into(),
            file_name: "data.bin".into(),
            final_file_name: "data.bin".into(),
            file_size: data.len() as u64,
            // Independently generated with Python hashlib, also checking SHA-256 compatibility.
            file_sha256: "b057f26faa652e9916aa08eed7e50906151f83de0e878173e9d06e359c0dd75e".into(),
            chunks: parallel_chunk_ranges(data.len() as u64),
            message_id: 1,
        };
        create_or_resume_parallel_manifest(&root, manifest.clone())
            .await
            .unwrap();
        let part = parallel_part_path(&root, &manifest.transfer_id, 0);
        tokio::fs::write(&part, &data).await.unwrap();
        let partial = received_partial_path(&root, &manifest.transfer_id);
        tokio::fs::write(&partial, b"previous attempt")
            .await
            .unwrap();

        let job_root = root.clone();
        let job_manifest = manifest.clone();
        let error = tokio::task::spawn_blocking(move || {
            let mut reads = 0;
            copy_and_hash_parallel_parts(&job_root, &job_manifest, || {
                reads += 1;
                reads > 1
            })
            .err()
            .unwrap()
        })
        .await
        .unwrap();
        assert!(error.contains("取消"));
        assert_eq!(
            tokio::fs::read(&partial).await.unwrap(),
            b"previous attempt"
        );

        for bytes in [&data[..data.len() - 1], &data[..]] {
            tokio::fs::write(&part, bytes).await.unwrap();
            let mut changed = manifest.clone();
            if bytes.len() == data.len() {
                changed.chunks[0].length -= 1;
            }
            let error = merge_parallel_parts(&root, &changed, |_| {})
                .await
                .unwrap_err();
            assert!(error.contains("长度"));
            assert_eq!(
                tokio::fs::read(&partial).await.unwrap(),
                b"previous attempt"
            );
            let mut entries =
                tokio::fs::read_dir(parallel_transfer_dir(&root, &manifest.transfer_id))
                    .await
                    .unwrap();
            while let Some(entry) = entries.next_entry().await.unwrap() {
                assert!(!entry.file_name().to_string_lossy().starts_with("merging-"));
            }
        }

        let mut corrupt = data.clone();
        corrupt[FILE_MERGE_BUFFER + 1] ^= 1;
        tokio::fs::write(&part, corrupt).await.unwrap();
        assert!(merge_parallel_parts(&root, &manifest, |_| {})
            .await
            .unwrap_err()
            .contains("SHA-256"));
        assert!(!parallel_transfer_dir(&root, &manifest.transfer_id).exists());
        assert_eq!(
            tokio::fs::read(&partial).await.unwrap(),
            b"previous attempt"
        );

        create_or_resume_parallel_manifest(&root, manifest.clone())
            .await
            .unwrap();
        tokio::fs::write(&part, &data).await.unwrap();
        let merged = merge_parallel_parts(&root, &manifest, |_| {})
            .await
            .unwrap();
        assert_eq!(merged, partial);
        assert_eq!(tokio::fs::read(&merged).await.unwrap(), data);
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
        crate::db::remove_test_database(&pool, &app_dir).await;
    }

    #[tokio::test]
    async fn streaming_upload_sends_chunks_before_digest_is_ready() {
        let data = b"a file whose checksum is deliberately delayed";
        let expected_hash = sha256_hex(&Sha256::digest(data));
        let (chunks_tx, mut chunks_rx) = tokio::sync::mpsc::unbounded_channel();
        let (complete_tx, mut complete_rx) = tokio::sync::mpsc::unbounded_channel();
        let router = axum::Router::new()
            .route(
                "/api/uploads/v4/prepare",
                axum::routing::post(
                    |axum::Json(payload): axum::Json<ParallelPrepareRequest>| async move {
                        assert!(payload.file_sha256.is_empty());
                        axum::Json(serde_json::json!({
                            "status": "ready",
                            "received": 0,
                            "missing_chunks": payload.chunks.iter()
                                .map(|chunk| chunk.index).collect::<Vec<_>>(),
                        }))
                    },
                ),
            )
            .route(
                "/api/uploads/v4/:transfer_id/:chunk_index",
                axum::routing::post(
                    move |axum::extract::Path((_, index)): axum::extract::Path<(String, usize)>,
                          request: axum::extract::Request| {
                        let chunks_tx = chunks_tx.clone();
                        async move {
                            let bytes = axum::body::to_bytes(request.into_body(), 1024)
                                .await
                                .unwrap();
                            chunks_tx.send((index, bytes)).unwrap();
                            axum::Json(serde_json::json!({ "status": "receiving" }))
                        }
                    },
                ),
            )
            .route(
                "/api/uploads/v4/complete",
                axum::routing::post(
                    move |axum::Json(payload): axum::Json<ParallelPrepareRequest>| {
                        let complete_tx = complete_tx.clone();
                        async move {
                            complete_tx.send(payload.file_sha256).unwrap();
                            axum::Json(serde_json::json!({ "status": "completed" }))
                        }
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let app_dir =
            std::env::temp_dir().join(format!("xchat-streaming-send-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone())).await.unwrap();
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
        let source = app_dir.join("source.bin");
        tokio::fs::write(&source, data).await.unwrap();
        let plan = UploadPlan::new(UploadProtocol::StreamingV4, 4);
        let transfer_id = new_transfer_id_for_plan("streaming-send", "peer-a", plan, false);
        db::create_transfer(
            &pool,
            &transfer_id,
            None,
            &conversation.id,
            "peer-a",
            "send",
            "transferring",
            data.len() as i64,
        )
        .await
        .unwrap();
        let release_hash = Arc::new(tokio::sync::Notify::new());
        let hash_gate = release_hash.clone();
        let hash = expected_hash.clone();
        let job = UploadJob {
            transfer_id,
            peer_id: "peer-a".into(),
            peer_addr: address.to_string(),
            conversation_id: conversation.id,
            client_message_id: "streaming-send".into(),
            message_id: 1,
            source: ValidatedSource {
                path: source.to_string_lossy().into_owned(),
                file_name: "source.bin".into(),
                size: data.len() as i64,
            },
            group_sync: None,
            upload_plan: plan,
            concurrency: super::super::transfer::TransferConcurrencyController::default()
                .generation(4)
                .unwrap(),
            file_sha256: Some(
                async move {
                    hash_gate.notified().await;
                    Ok(hash)
                }
                .boxed()
                .shared(),
            ),
        };
        let mut cancelled_job = job.clone();
        let task_pool = pool.clone();
        let upload = tokio::spawn(async move {
            upload_parallel_chunks(
                &task_pool,
                &job,
                &Arc::new(std::sync::atomic::AtomicBool::new(false)),
                &claim_test_upload(&task_pool, &job.transfer_id).await,
            )
            .await
        });
        let mut received = Vec::new();
        let ranges = flexible_parallel_chunk_ranges(data.len() as u64, 4);
        for _ in &ranges {
            received.push(
                tokio::time::timeout(Duration::from_secs(2), chunks_rx.recv())
                    .await
                    .expect("upload waited for the full-file checksum")
                    .unwrap(),
            );
        }
        received.sort_by_key(|(index, _)| *index);
        let bytes: Vec<_> = received
            .into_iter()
            .flat_map(|(_, bytes)| bytes.to_vec())
            .collect();
        assert_eq!(bytes, data);
        assert!(!upload.is_finished());
        assert!(complete_rx.try_recv().is_err());
        release_hash.notify_one();
        let outcome = tokio::time::timeout(Duration::from_secs(2), upload)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(outcome, UploadOutcome::Completed(size) if size == data.len() as i64));
        assert_eq!(complete_rx.try_recv().unwrap(), expected_hash);

        cancelled_job.transfer_id = new_transfer_id_for_plan("streaming-cancel", "peer-a", plan, false);
        cancelled_job.client_message_id = "streaming-cancel".into();
        cancelled_job.file_sha256 = Some(futures_util::future::pending().boxed().shared());
        db::create_transfer(
            &pool,
            &cancelled_job.transfer_id,
            None,
            &cancelled_job.conversation_id,
            "peer-a",
            "send",
            "transferring",
            data.len() as i64,
        )
        .await
        .unwrap();
        let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_token = token.clone();
        let task_pool = pool.clone();
        let upload =
            tokio::spawn(
                async move {
                    let lease = claim_test_upload(&task_pool, &cancelled_job.transfer_id).await;
                    upload_parallel_chunks(&task_pool, &cancelled_job, &token, &lease).await
                },
            );
        for _ in &ranges {
            tokio::time::timeout(Duration::from_secs(2), chunks_rx.recv())
                .await
                .unwrap()
                .unwrap();
        }
        cancel_token.store(true, Ordering::Release);
        let outcome = tokio::time::timeout(Duration::from_secs(2), upload)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(outcome, UploadOutcome::Cancelled(size) if size == data.len() as i64));
        assert!(complete_rx.try_recv().is_err());
        server.abort();
        let _ = server.await;
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
    }

    #[tokio::test]
    async fn concurrent_v3_uploads_share_global_limit_and_both_make_progress() {
        #[derive(Clone)]
        struct Probe {
            active: Arc<std::sync::atomic::AtomicUsize>,
            peak: Arc<std::sync::atomic::AtomicUsize>,
            order: Arc<Mutex<Vec<String>>>,
        }

        let probe = Probe {
            active: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            peak: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            order: Arc::new(Mutex::new(Vec::new())),
        };
        let prepare = axum::routing::post(
            |axum::Json(payload): axum::Json<ParallelPrepareRequest>| async move {
                axum::Json(serde_json::json!({
                    "status": "ready",
                    "received": 0,
                    "missing_chunks": payload
                        .chunks
                        .iter()
                        .map(|chunk| chunk.index)
                        .collect::<Vec<_>>(),
                }))
            },
        );
        let chunk_probe = probe.clone();
        let chunks = axum::routing::post(
            move |axum::extract::Path((transfer_id, _chunk_index)): axum::extract::Path<(
                String,
                usize,
            )>,
                  request: axum::extract::Request| {
                let probe = chunk_probe.clone();
                async move {
                    let active = probe
                        .active
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                        + 1;
                    probe
                        .peak
                        .fetch_max(active, std::sync::atomic::Ordering::AcqRel);
                    probe
                        .order
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(transfer_id);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let _ = axum::body::to_bytes(request.into_body(), usize::MAX).await;
                    probe
                        .active
                        .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                    axum::Json(serde_json::json!({ "status": "receiving" }))
                }
            },
        );
        let router = axum::Router::new()
            .route(
                "/api/peer_identity",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "device_id": "peer-a",
                        "name": "Alice",
                    }))
                }),
            )
            .route("/api/uploads/v3/prepare", prepare)
            .route("/api/uploads/v3/:transfer_id/:chunk_index", chunks);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let app_dir = std::env::temp_dir().join(format!(
            "xchat-global-parallel-limit-{}",
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
        let file_size = CHUNK_SIZE + 17;
        let source_a = app_dir.join("source-a.bin");
        let source_b = app_dir.join("source-b.bin");
        let data = vec![7u8; file_size];
        tokio::fs::write(&source_a, &data).await.unwrap();
        tokio::fs::write(&source_b, &data).await.unwrap();
        let controller = crate::network::transfer::TransferConcurrencyController::default();
        for limit in crate::network::transfer::MAX_PARALLEL_CHANNEL_OPTIONS {
            probe
                .peak
                .store(0, std::sync::atomic::Ordering::Release);
            probe
                .order
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
            let generation = controller.generation(limit).unwrap();
            let transfer_a = format!("transfer-a-{limit}");
            let transfer_b = format!("transfer-b-{limit}");
            for transfer_id in [&transfer_a, &transfer_b] {
                db::create_transfer(
                    &pool,
                    transfer_id,
                    None,
                    &conversation.id,
                    "peer-a",
                    "send",
                    "transferring",
                    file_size as i64,
                )
                .await
                .unwrap();
            }
            let make_job = |transfer_id: &str, source: &Path| UploadJob {
                transfer_id: transfer_id.into(),
                peer_id: "peer-a".into(),
                peer_addr: address.to_string(),
                conversation_id: conversation.id.clone(),
                client_message_id: transfer_id.into(),
                message_id: 1,
                source: ValidatedSource {
                    path: source.to_string_lossy().into_owned(),
                    file_name: source.file_name().unwrap().to_string_lossy().into_owned(),
                    size: file_size as i64,
                },
                group_sync: None,
                upload_plan: UploadPlan::new(UploadProtocol::FlexibleV3, limit),
                concurrency: generation.clone(),
                file_sha256: Some(
                    futures_util::future::ready(Ok("0".repeat(64))).boxed().shared(),
                ),
            };
            let token_a = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let token_b = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let job_a = make_job(&transfer_a, &source_a);
            let job_b = make_job(&transfer_b, &source_b);
            let lease_a = claim_test_upload(&pool, &transfer_a).await;
            let lease_b = claim_test_upload(&pool, &transfer_b).await;

            let (outcome_a, outcome_b) = tokio::join!(
                upload_parallel_chunks(&pool, &job_a, &token_a, &lease_a),
                upload_parallel_chunks(&pool, &job_b, &token_b, &lease_b),
            );

            for outcome in [outcome_a, outcome_b] {
                match outcome {
                    UploadOutcome::Completed(bytes) => assert_eq!(bytes, file_size as i64),
                    UploadOutcome::Failed(_, error) => panic!("parallel upload failed: {error}"),
                    UploadOutcome::AwaitingAcceptance(_) => {
                        panic!("parallel upload unexpectedly awaited acceptance")
                    }
                    UploadOutcome::Cancelled(_) => {
                        panic!("parallel upload was unexpectedly cancelled")
                    }
                }
            }
            assert!(
                probe.peak.load(std::sync::atomic::Ordering::Acquire)
                    <= usize::from(limit),
                "limit {limit} was exceeded"
            );
            let order = probe
                .order
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let first_b = order
                .iter()
                .position(|transfer_id| transfer_id == &transfer_b)
                .unwrap();
            assert!(
                first_b
                    < flexible_parallel_chunk_ranges(file_size as u64, limit).len(),
                "the second transfer must start before the first consumes its full queue at limit {limit}"
            );
        }

        server.abort();
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
    }

    #[tokio::test]
    async fn sequential_v1_and_fixed_v2_use_the_same_global_permit_pool() {
        let data_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let v1_requests = data_requests.clone();
        let v2_requests = data_requests.clone();
        let router = axum::Router::new()
            .route(
                "/api/peer_identity",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "device_id": "peer-a",
                        "name": "Alice",
                    }))
                }),
            )
            .route(
                "/api/upload",
                axum::routing::post(move |request: axum::extract::Request| {
                    let requests = v1_requests.clone();
                    async move {
                        requests.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        let _ = axum::body::to_bytes(request.into_body(), usize::MAX).await;
                        axum::Json(serde_json::json!({ "status": "receiving" }))
                    }
                }),
            )
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
                axum::routing::post(move |request: axum::extract::Request| {
                    let requests = v2_requests.clone();
                    async move {
                        requests.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        let _ = axum::body::to_bytes(request.into_body(), usize::MAX).await;
                        axum::Json(serde_json::json!({ "status": "receiving" }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let app_dir = std::env::temp_dir().join(format!(
            "xchat-shared-legacy-limit-{}",
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
        let source = app_dir.join("source.bin");
        tokio::fs::write(&source, b"payload").await.unwrap();
        let generation = crate::network::transfer::TransferConcurrencyController::default()
            .generation(4)
            .unwrap();
        let blocker = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(generation.acquire(&blocker).await.unwrap());
        }
        for transfer_id in ["v1-transfer", "v2-transfer"] {
            db::create_transfer(
                &pool,
                transfer_id,
                None,
                &conversation.id,
                "peer-a",
                "send",
                "transferring",
                7,
            )
            .await
            .unwrap();
        }
        let make_job = |transfer_id: &str, upload_plan: UploadPlan| UploadJob {
            transfer_id: transfer_id.into(),
            peer_id: "peer-a".into(),
            peer_addr: address.to_string(),
            conversation_id: conversation.id.clone(),
            client_message_id: transfer_id.into(),
            message_id: 1,
            source: ValidatedSource {
                path: source.to_string_lossy().into_owned(),
                file_name: "source.bin".into(),
                size: 7,
            },
            group_sync: None,
            upload_plan,
            concurrency: generation.clone(),
            file_sha256: upload_plan
                .is_parallel()
                .then(|| futures_util::future::ready(Ok("0".repeat(64))).boxed().shared()),
        };
        let v1_job = make_job(
            "v1-transfer",
            UploadPlan::new(UploadProtocol::SequentialV1, 1),
        );
        let v2_job = make_job(
            "v2-transfer",
            UploadPlan::new(UploadProtocol::FixedV2, 4),
        );
        let pool_v1 = pool.clone();
        let v1 = tokio::spawn(async move {
            upload_chunks(
                &pool_v1,
                &v1_job,
                &Arc::new(std::sync::atomic::AtomicBool::new(false)),
                &claim_test_upload(&pool_v1, &v1_job.transfer_id).await,
            )
            .await
        });
        let pool_v2 = pool.clone();
        let v2 = tokio::spawn(async move {
            upload_chunks(
                &pool_v2,
                &v2_job,
                &Arc::new(std::sync::atomic::AtomicBool::new(false)),
                &claim_test_upload(&pool_v2, &v2_job.transfer_id).await,
            )
            .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            data_requests.load(std::sync::atomic::Ordering::Acquire),
            0,
            "neither legacy protocol may bypass an exhausted global pool"
        );
        drop(held);
        for outcome in [v1.await.unwrap(), v2.await.unwrap()] {
            assert!(matches!(outcome, UploadOutcome::Completed(7)));
        }
        assert_eq!(
            data_requests.load(std::sync::atomic::Ordering::Acquire),
            2
        );

        server.abort();
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
    }

    #[tokio::test]
    async fn cancelling_an_in_flight_request_releases_its_global_permit() {
        let started = Arc::new(tokio::sync::Notify::new());
        let request_started = started.clone();
        let router = axum::Router::new()
            .route(
                "/api/peer_identity",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "device_id": "peer-a",
                        "name": "Alice",
                    }))
                }),
            )
            .route(
                "/api/upload",
                axum::routing::post(move |request: axum::extract::Request| {
                    let started = request_started.clone();
                    async move {
                        let _ = axum::body::to_bytes(request.into_body(), usize::MAX).await;
                        started.notify_one();
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        axum::Json(serde_json::json!({ "status": "receiving" }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let app_dir = std::env::temp_dir().join(format!(
            "xchat-cancel-in-flight-{}",
            uuid::Uuid::new_v4()
        ));
        let pool = db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let source = app_dir.join("source.bin");
        tokio::fs::write(&source, b"payload").await.unwrap();
        let generation = crate::network::transfer::TransferConcurrencyController::default()
            .generation(4)
            .unwrap();
        let blocker = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut held = Vec::new();
        for _ in 0..3 {
            held.push(generation.acquire(&blocker).await.unwrap());
        }
        let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let job = UploadJob {
            transfer_id: "cancel-in-flight".into(),
            peer_id: "peer-a".into(),
            peer_addr: address.to_string(),
            conversation_id: "conversation".into(),
            client_message_id: "cancel-in-flight".into(),
            message_id: 1,
            source: ValidatedSource {
                path: source.to_string_lossy().into_owned(),
                file_name: "source.bin".into(),
                size: 7,
            },
            group_sync: None,
            upload_plan: UploadPlan::new(UploadProtocol::SequentialV1, 1),
            concurrency: generation.clone(),
            file_sha256: None,
        };
        let upload_pool = pool.clone();
        let upload_token = token.clone();
        let mut upload = tokio::spawn(async move {
            // This test cancels before any progress write; no database task is created.
            let lease = db::TransferLease { transfer_id: job.transfer_id.clone(), token: "unused".into() };
            upload_chunks(&upload_pool, &job, &upload_token, &lease).await
        });
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("the request should reach the receiver");

        token.store(true, Ordering::Release);
        let outcome = tokio::time::timeout(Duration::from_secs(1), &mut upload)
            .await
            .expect("an in-flight request should stop promptly")
            .unwrap();
        assert!(matches!(outcome, UploadOutcome::Cancelled(0)));
        let replacement = tokio::time::timeout(
            Duration::from_secs(1),
            generation.acquire(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
        )
        .await
        .expect("cancellation must release the request permit")
        .unwrap();
        drop(replacement);
        drop(held);

        server.abort();
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
    }

    #[tokio::test]
    async fn parallel_sender_failure_marks_receiver_failed() {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let terminal_tx = status_tx.clone();
        let router = axum::Router::new()
            .route(
                "/api/peer_identity",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "device_id": "peer-a",
                        "name": "Alice",
                    }))
                }),
            )
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
                peer_id: "peer-a".into(),
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
                upload_plan: UploadPlan::new(UploadProtocol::FixedV2, 4),
                concurrency: crate::network::transfer::TransferConcurrencyController::default()
                    .generation(4)
                    .unwrap(),
                file_sha256: Some(deferred_file_sha256(
                    source_path.to_string_lossy().into_owned(),
                )),
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
        crate::db::remove_test_database(&pool, &app_dir).await;
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
    async fn large_parallel_send_returns_before_reading_source() {
        let identity_requested = Arc::new(tokio::sync::Notify::new());
        let identity_release = Arc::new(tokio::sync::Notify::new());
        let requested = identity_requested.clone();
        let release = identity_release.clone();
        let router = axum::Router::new().route(
            "/api/peer_identity",
            axum::routing::get(move || {
                let requested = requested.clone();
                let release = release.clone();
                async move {
                    requested.notify_one();
                    release.notified().await;
                    axum::Json(serde_json::json!({
                        "device_id": "peer-a",
                        "name": "Alice",
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let app_dir = std::env::temp_dir()
            .join(format!("xchat-large-send-test-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let conversation = db::ensure_direct_conversation(&pool, "peer-a")
            .await
            .unwrap();
        let source = app_dir.join("large.bin");
        let file_size = 6 * 1024 * 1024 * 1024 + 1;
        tokio::fs::File::create(&source)
            .await
            .unwrap()
            .set_len(file_size)
            .await
            .unwrap();
        let peers = PeerManager::new();
        peers.add_or_update_with_details(
            "peer-a".into(),
            "Alice".into(),
            address.to_string(),
            0,
            None,
            None,
            None,
            vec![PARALLEL_FILE_V3_CAPABILITY.into()],
            None,
            true,
        );

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            send_path(&pool, &peers, &conversation.id, source.to_str().unwrap()),
        )
        .await;
        let mut cancelled = false;
        if let Ok(Ok(result)) = &result {
            assert_eq!(result.message.file_size, Some(file_size as i64));
            assert_eq!(result.message.file_status.as_deref(), Some("queued"));
            assert_eq!(result.transfers.len(), 1);
            let transfer_id = &result.transfers[0].id;
            tokio::time::timeout(Duration::from_secs(2), identity_requested.notified())
                .await
                .unwrap();
            cancellation_registry().request_cancel(transfer_id);
            identity_release.notify_one();
            cancelled = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if db::get_transfer(&pool, transfer_id)
                        .await
                        .unwrap()
                        .unwrap()
                        .status
                        == "cancelled"
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .is_ok();
        }

        server.abort();
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
        assert!(
            result.is_ok(),
            "send must return without scanning the 6 GiB source"
        );
        result.unwrap().unwrap();
        assert!(
            cancelled,
            "preparation must be cancellable before reading the source"
        );
    }

    #[tokio::test]
    async fn offline_file_messages_remain_pending_until_the_peer_returns() {
        let app_dir = std::env::temp_dir().join(format!(
            "xchat-offline-file-test-{}",
            uuid::Uuid::new_v4()
        ));
        let pool = db::init_db_standalone(Some(app_dir.clone()))
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
        let source = app_dir.join("waiting.png");
        tokio::fs::write(&source, b"image").await.unwrap();

        let result = send_path(
            &pool,
            &PeerManager::new(),
            &conversation.id,
            source.to_str().unwrap(),
        )
        .await
        .unwrap();
        let message_status = result.message.status.clone();
        let file_status = result.message.file_status.clone();
        let transfer_statuses = result
            .transfers
            .iter()
            .map(|transfer| transfer.status.clone())
            .collect::<Vec<_>>();

        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;

        assert_eq!(message_status.as_deref(), Some("pending"));
        assert_eq!(file_status.as_deref(), Some("waiting_peer"));
        assert!(
            transfer_statuses
                .iter()
                .all(|status| status == "waiting_peer")
        );
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
            &[],
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
        crate::db::remove_test_database(&pool, &app_dir).await;
        if managed_source_dir.exists() {
            std::fs::remove_dir_all(managed_source_dir).unwrap();
        }
    }
}
