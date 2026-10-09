use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
    time::Duration,
};

use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

pub type TransferCancellationToken = Arc<AtomicBool>;

const MAX_PARALLEL_CHANNELS_SETTING_KEY: &str = "file_transfer.max_parallel_channels.v1";
pub const DEFAULT_MAX_PARALLEL_CHANNELS: u8 = 4;
pub const MAX_PARALLEL_CHANNEL_OPTIONS: [u8; 3] = [4, 8, 16];
const PERMIT_CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub fn validate_max_parallel_channels(value: u8) -> Result<u8, String> {
    if MAX_PARALLEL_CHANNEL_OPTIONS.contains(&value) {
        Ok(value)
    } else {
        Err("max parallel channels must be 4, 8, or 16".to_string())
    }
}

pub async fn load_max_parallel_channels(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<u8, String> {
    let stored = crate::db::get_setting(pool, MAX_PARALLEL_CHANNELS_SETTING_KEY).await?;
    Ok(stored
        .as_deref()
        .and_then(|value| value.trim().parse::<u8>().ok())
        .and_then(|value| validate_max_parallel_channels(value).ok())
        .unwrap_or(DEFAULT_MAX_PARALLEL_CHANNELS))
}

pub async fn save_max_parallel_channels(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    value: u8,
) -> Result<(), String> {
    let value = validate_max_parallel_channels(value)?;
    crate::db::set_setting(pool, MAX_PARALLEL_CHANNELS_SETTING_KEY, &value.to_string()).await?;
    concurrency_controller().generation(value)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPermitError {
    Cancelled,
    Closed,
}

#[derive(Debug, Clone)]
pub struct TransferConcurrencyGeneration {
    limit: u8,
    budget: Arc<StreamBudget>,
}

impl TransferConcurrencyGeneration {
    pub fn limit(&self) -> u8 {
        self.limit
    }

    pub async fn acquire(
        &self,
        cancellation: &TransferCancellationToken,
    ) -> Result<StreamPermit, TransferPermitError> {
        self.acquire_for_peer("", cancellation).await
    }

    pub async fn acquire_for_peer(
        &self,
        peer: &str,
        cancellation: &TransferCancellationToken,
    ) -> Result<StreamPermit, TransferPermitError> {
        let ticket = {
            let mut state = self.budget.lock();
            state.next_ticket += 1;
            let ticket = state.next_ticket;
            state.waiters.push_back((ticket, peer.to_owned()));
            ticket
        };
        // Future cancellation removes its ticket as well as explicit cancellation.
        let _waiting = StreamWaiter {
            budget: self.budget.clone(),
            ticket,
        };
        loop {
            let notified = self.budget.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if cancellation.load(Ordering::Acquire) {
                return Err(TransferPermitError::Cancelled);
            }
            {
                let mut state = self.budget.lock();
                let first_eligible = state
                    .waiters
                    .iter()
                    .find(|(_, peer)| state.peer_available(peer));
                if state.active < state.limit && first_eligible.is_some_and(|(id, _)| *id == ticket)
                {
                    state.waiters.retain(|(id, _)| *id != ticket);
                    return Ok(self.budget.admit(&mut state, peer));
                }
            }
            tokio::select! {
                _ = &mut notified => {},
                _ = tokio::time::sleep(PERMIT_CANCEL_POLL_INTERVAL) => {},
            }
        }
    }
}

#[derive(Debug)]
pub struct TransferConcurrencyController {
    budget: Arc<StreamBudget>,
}

impl Default for TransferConcurrencyController {
    fn default() -> Self {
        Self {
            budget: StreamBudget::new(usize::from(DEFAULT_MAX_PARALLEL_CHANNELS)),
        }
    }
}

impl TransferConcurrencyController {
    pub fn generation(&self, limit: u8) -> Result<TransferConcurrencyGeneration, String> {
        let limit = validate_max_parallel_channels(limit)?;
        self.budget.lock().limit = usize::from(limit);
        self.budget.changed.notify_waiters();
        Ok(TransferConcurrencyGeneration {
            limit,
            budget: self.budget.clone(),
        })
    }
}

#[derive(Debug, Default)]
struct StreamBudgetState {
    limit: usize,
    active: usize,
    peers: HashMap<String, usize>,
    next_ticket: u64,
    waiters: VecDeque<(u64, String)>,
}

impl StreamBudgetState {
    fn peer_available(&self, peer: &str) -> bool {
        peer.is_empty() || self.peers.get(peer).copied().unwrap_or(0) < (self.limit / 2).max(4)
    }
}

#[derive(Debug)]
struct StreamBudget {
    state: Mutex<StreamBudgetState>,
    changed: Notify,
}

impl StreamBudget {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(StreamBudgetState {
                limit,
                ..Default::default()
            }),
            changed: Notify::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, StreamBudgetState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn admit(self: &Arc<Self>, state: &mut StreamBudgetState, peer: &str) -> StreamPermit {
        state.active += 1;
        *state.peers.entry(peer.to_owned()).or_default() += 1;
        self.changed.notify_waiters();
        StreamPermit {
            budget: self.clone(),
            peer: peer.to_owned(),
        }
    }

    fn try_acquire(self: &Arc<Self>, peer: &str) -> Result<StreamPermit, String> {
        let mut state = self.lock();
        if state.active >= state.limit || !state.peer_available(peer) {
            return Err("文件接收繁忙，请稍后重试".to_string());
        }
        Ok(self.admit(&mut state, peer))
    }
}

struct StreamWaiter {
    budget: Arc<StreamBudget>,
    ticket: u64,
}

impl Drop for StreamWaiter {
    fn drop(&mut self) {
        self.budget
            .lock()
            .waiters
            .retain(|(id, _)| *id != self.ticket);
        self.budget.changed.notify_waiters();
    }
}

#[derive(Debug)]
pub struct StreamPermit {
    budget: Arc<StreamBudget>,
    peer: String,
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        let mut state = self.budget.lock();
        state.active -= 1;
        if let Some(active) = state.peers.get_mut(&self.peer) {
            *active -= 1;
            if *active == 0 {
                state.peers.remove(&self.peer);
            }
        }
        self.budget.changed.notify_waiters();
    }
}

pub fn receive_permit(peer: &str) -> Result<StreamPermit, String> {
    static BUDGET: OnceLock<Arc<StreamBudget>> = OnceLock::new();
    BUDGET
        .get_or_init(|| StreamBudget::new(8))
        .try_acquire(peer)
}

pub async fn digest_permit() -> Result<OwnedSemaphorePermit, String> {
    static BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();
    BUDGET
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "摘要预算已关闭".to_string())
}

pub async fn merge_permit() -> Result<OwnedSemaphorePermit, String> {
    static BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();
    BUDGET
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "合并预算已关闭".to_string())
}

const DISK_HEADROOM: u64 = 64 * 1024 * 1024;
static DISK_RESERVATIONS: OnceLock<Mutex<HashMap<std::path::PathBuf, u64>>> = OnceLock::new();

pub struct DiskReservation {
    volume: std::path::PathBuf,
    bytes: u64,
}

impl Drop for DiskReservation {
    fn drop(&mut self) {
        let mut reservations = DISK_RESERVATIONS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(bytes) = reservations.get_mut(&self.volume) {
            *bytes = bytes.saturating_sub(self.bytes);
            if *bytes == 0 {
                reservations.remove(&self.volume);
            }
        }
    }
}

fn disk_space_sufficient(available: u64, reserved: u64, requested: u64) -> bool {
    reserved
        .checked_add(DISK_HEADROOM)
        .and_then(|needed| needed.checked_add(requested))
        .is_some_and(|needed| available >= needed)
}

pub async fn reserve_disk(path: &std::path::Path, bytes: u64) -> Result<DiskReservation, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        fn normalized(path: &std::path::Path) -> String {
            let value = path.to_string_lossy();
            if cfg!(windows) {
                value
                    .trim_start_matches(r"\\?\")
                    .replace('\\', "/")
                    .to_lowercase()
            } else {
                value.into_owned()
            }
        }
        // Serialize checking and reservation. Other processes can still consume
        // space; write errors remain fatal and the staging file is removed.
        let mut reservations = DISK_RESERVATIONS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let target = normalized(&path);
        let disk = disks
            .iter()
            .filter(|disk| {
                let mount = normalized(disk.mount_point());
                target == mount || target.starts_with(&format!("{}/", mount.trim_end_matches('/')))
            })
            .max_by_key(|disk| disk.mount_point().as_os_str().len())
            .ok_or_else(|| "无法确定下载目录的可用磁盘空间".to_string())?;
        let volume = disk.mount_point().to_path_buf();
        let reserved = reservations.get(&volume).copied().unwrap_or(0);
        if !disk_space_sufficient(disk.available_space(), reserved, bytes) {
            return Err("磁盘空间不足，无法预留接收或合并空间".to_string());
        }
        reservations.insert(volume.clone(), reserved.saturating_add(bytes));
        Ok(DiskReservation { volume, bytes })
    })
    .await
    .map_err(|error| format!("检查磁盘空间失败: {error}"))?
}

pub fn concurrency_controller() -> &'static TransferConcurrencyController {
    static CONTROLLER: OnceLock<TransferConcurrencyController> = OnceLock::new();
    CONTROLLER.get_or_init(TransferConcurrencyController::default)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationRequest {
    Requested,
    AlreadyRequested,
    NotFound,
}

#[derive(Debug, Default)]
pub struct TransferCancellationRegistry {
    transfers: Mutex<HashMap<String, TransferCancellationToken>>,
}

pub fn cancellation_registry() -> &'static TransferCancellationRegistry {
    static REGISTRY: OnceLock<TransferCancellationRegistry> = OnceLock::new();
    REGISTRY.get_or_init(TransferCancellationRegistry::default)
}

impl TransferCancellationRegistry {
    pub fn register_execution(&self, transfer_id: &str) -> TransferCancellationToken {
        let token = Arc::new(AtomicBool::new(false));
        if let Some(previous) = self
            .transfers()
            .insert(transfer_id.to_string(), token.clone())
        {
            previous.store(true, Ordering::Release);
        }
        token
    }

    pub fn complete_execution(&self, transfer_id: &str, token: &TransferCancellationToken) {
        let mut transfers = self.transfers();
        if transfers
            .get(transfer_id)
            .is_some_and(|current| Arc::ptr_eq(current, token))
        {
            transfers.remove(transfer_id);
        }
    }

    pub fn register(&self, transfer_id: impl Into<String>) -> TransferCancellationToken {
        self.transfers()
            .entry(transfer_id.into())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    pub fn request_cancel(&self, transfer_id: &str) -> CancellationRequest {
        let transfers = self.transfers();
        let Some(token) = transfers.get(transfer_id) else {
            return CancellationRequest::NotFound;
        };

        if token.swap(true, Ordering::AcqRel) {
            CancellationRequest::AlreadyRequested
        } else {
            CancellationRequest::Requested
        }
    }

    pub fn is_cancelled(&self, transfer_id: &str) -> bool {
        self.transfers()
            .get(transfer_id)
            .is_some_and(|token| token.load(Ordering::Acquire))
    }

    pub fn complete(&self, transfer_id: &str) -> bool {
        self.transfers().remove(transfer_id).is_some()
    }

    fn transfers(&self) -> MutexGuard<'_, HashMap<String, TransferCancellationToken>> {
        self.transfers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_reservation_keeps_headroom_and_cannot_overflow() {
        assert!(disk_space_sufficient(DISK_HEADROOM + 200, 100, 100));
        assert!(!disk_space_sufficient(DISK_HEADROOM + 199, 100, 100));
        assert!(!disk_space_sufficient(0, 0, 1));
        assert!(!disk_space_sufficient(u64::MAX, u64::MAX, 1));
    }

    async fn settings_pool() -> sqlx::Pool<sqlx::Sqlite> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[test]
    fn cancellation_is_idempotent_and_completion_removes_token() {
        let registry = TransferCancellationRegistry::default();
        let token = registry.register("transfer-1");

        assert_eq!(
            registry.request_cancel("transfer-1"),
            CancellationRequest::Requested
        );
        assert_eq!(
            registry.request_cancel("transfer-1"),
            CancellationRequest::AlreadyRequested
        );
        assert!(token.load(Ordering::Acquire));
        assert!(registry.is_cancelled("transfer-1"));
        assert!(registry.complete("transfer-1"));
        assert_eq!(
            registry.request_cancel("transfer-1"),
            CancellationRequest::NotFound
        );
    }

    #[test]
    fn expired_execution_cannot_remove_replacement_cancellation_handle() {
        let registry = TransferCancellationRegistry::default();
        let old = registry.register_execution("transfer");
        let current = registry.register_execution("transfer");
        assert!(old.load(Ordering::Acquire));
        registry.complete_execution("transfer", &old);
        assert_eq!(
            registry.request_cancel("transfer"),
            CancellationRequest::Requested
        );
        assert!(current.load(Ordering::Acquire));
        registry.complete_execution("transfer", &current);
        assert_eq!(
            registry.request_cancel("transfer"),
            CancellationRequest::NotFound
        );
    }

    #[tokio::test]
    async fn max_parallel_channels_defaults_and_valid_values_round_trip() {
        let pool = settings_pool().await;

        assert_eq!(
            load_max_parallel_channels(&pool).await.unwrap(),
            DEFAULT_MAX_PARALLEL_CHANNELS
        );

        for channels in MAX_PARALLEL_CHANNEL_OPTIONS {
            save_max_parallel_channels(&pool, channels).await.unwrap();
            assert_eq!(load_max_parallel_channels(&pool).await.unwrap(), channels);
        }
    }

    #[tokio::test]
    async fn invalid_max_parallel_channels_does_not_replace_valid_value() {
        let pool = settings_pool().await;
        save_max_parallel_channels(&pool, 8).await.unwrap();

        let error = save_max_parallel_channels(&pool, 12).await.unwrap_err();

        assert!(error.contains("4, 8, or 16"));
        assert_eq!(load_max_parallel_channels(&pool).await.unwrap(), 8);
    }

    #[tokio::test]
    async fn malformed_max_parallel_channels_falls_back_to_four() {
        let pool = settings_pool().await;
        crate::db::set_setting(&pool, MAX_PARALLEL_CHANNELS_SETTING_KEY, "many")
            .await
            .unwrap();

        assert_eq!(
            load_max_parallel_channels(&pool).await.unwrap(),
            DEFAULT_MAX_PARALLEL_CHANNELS
        );
    }

    #[tokio::test]
    async fn generations_share_budget_and_shrink_waits_for_old_requests() {
        let controller = TransferConcurrencyController::default();

        let first = controller.generation(4).unwrap();
        let same = controller.generation(4).unwrap();
        let next = controller.generation(8).unwrap();

        assert!(Arc::ptr_eq(&first.budget, &same.budget));
        assert!(Arc::ptr_eq(&first.budget, &next.budget));
        assert_eq!(first.limit(), 4);
        assert_eq!(next.limit(), 8);
        let token = Arc::new(AtomicBool::new(false));
        let mut held = Vec::new();
        for _ in 0..8 {
            held.push(first.acquire(&token).await.unwrap());
        }
        let reduced = controller.generation(4).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), reduced.acquire(&token))
                .await
                .is_err()
        );
        held.truncate(4);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), next.acquire(&token))
                .await
                .is_err()
        );
        held.pop();
        let _permit = next.acquire(&token).await.unwrap();
        assert_eq!(first.budget.lock().active, 4);
        assert!(first.budget.lock().waiters.is_empty());
    }

    #[tokio::test]
    async fn max_parallel_channels_generation_enforces_limit_and_cancels_waiter() {
        let generation = TransferConcurrencyController::default()
            .generation(4)
            .unwrap();
        let active = Arc::new(AtomicBool::new(false));
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(generation.acquire(&active).await.unwrap());
        }
        assert_eq!(generation.budget.lock().active, 4);

        let queued_generation = generation.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let queued_cancelled = cancelled.clone();
        let waiter =
            tokio::spawn(async move { queued_generation.acquire(&queued_cancelled).await });
        tokio::task::yield_now().await;
        cancelled.store(true, Ordering::Release);

        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("cancelled permit wait should finish")
            .unwrap();
        assert_eq!(result.unwrap_err(), TransferPermitError::Cancelled);

        drop(held);
        assert_eq!(generation.budget.lock().active, 0);
        assert!(generation.budget.lock().waiters.is_empty());
    }

    #[tokio::test]
    async fn every_supported_max_parallel_channels_value_is_enforced() {
        let controller = TransferConcurrencyController::default();
        let active = Arc::new(AtomicBool::new(false));

        for limit in MAX_PARALLEL_CHANNEL_OPTIONS {
            let generation = controller.generation(limit).unwrap();
            let mut held = Vec::new();
            for _ in 0..limit {
                held.push(generation.acquire(&active).await.unwrap());
            }
            assert_eq!(generation.budget.lock().active, usize::from(limit));
            assert!(generation.budget.try_acquire("").is_err());
            drop(held);
            assert_eq!(generation.budget.lock().active, 0);
        }
    }

    #[tokio::test]
    async fn saturated_peer_does_not_block_another_peer_or_leak_waiters() {
        let generation = TransferConcurrencyController::default()
            .generation(8)
            .unwrap();
        let token = Arc::new(AtomicBool::new(false));
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(generation.acquire_for_peer("slow", &token).await.unwrap());
        }
        let queued = generation.acquire_for_peer("slow", &token);
        tokio::pin!(queued);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut queued)
            .await
            .is_err());
        let other = tokio::time::timeout(
            Duration::from_millis(100),
            generation.acquire_for_peer("healthy", &token),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(generation.budget.lock().active, 5);
        drop(other);
        let receive = StreamBudget::new(8);
        let mut incoming = Vec::new();
        for _ in 0..4 {
            incoming.push(receive.try_acquire("one").unwrap());
        }
        assert!(receive.try_acquire("one").is_err());
        for _ in 0..4 {
            incoming.push(receive.try_acquire("two").unwrap());
        }
        assert!(receive.try_acquire("three").is_err());
        drop(incoming);
        assert_eq!(receive.lock().active, 0);
    }
}
