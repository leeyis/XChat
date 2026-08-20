use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
    time::Duration,
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

pub async fn load_max_parallel_channels(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<u8, String> {
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
    crate::db::set_setting(pool, MAX_PARALLEL_CHANNELS_SETTING_KEY, &value.to_string()).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPermitError {
    Cancelled,
    Closed,
}

#[derive(Debug, Clone)]
pub struct TransferConcurrencyGeneration {
    limit: u8,
    semaphore: Arc<Semaphore>,
}

impl TransferConcurrencyGeneration {
    pub fn limit(&self) -> u8 {
        self.limit
    }

    pub async fn acquire(
        &self,
        cancellation: &TransferCancellationToken,
    ) -> Result<OwnedSemaphorePermit, TransferPermitError> {
        if cancellation.load(Ordering::Acquire) {
            return Err(TransferPermitError::Cancelled);
        }

        let acquire = self.semaphore.clone().acquire_owned();
        tokio::pin!(acquire);
        loop {
            tokio::select! {
                permit = &mut acquire => {
                    return permit.map_err(|_| TransferPermitError::Closed);
                }
                _ = tokio::time::sleep(PERMIT_CANCEL_POLL_INTERVAL) => {
                    if cancellation.load(Ordering::Acquire) {
                        return Err(TransferPermitError::Cancelled);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct TransferConcurrencyController {
    current: Mutex<Option<TransferConcurrencyGeneration>>,
}

impl TransferConcurrencyController {
    pub fn generation(&self, limit: u8) -> Result<TransferConcurrencyGeneration, String> {
        let limit = validate_max_parallel_channels(limit)?;
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(generation) = current.as_ref().filter(|current| current.limit == limit) {
            return Ok(generation.clone());
        }

        let generation = TransferConcurrencyGeneration {
            limit,
            semaphore: Arc::new(Semaphore::new(usize::from(limit))),
        };
        *current = Some(generation.clone());
        Ok(generation)
    }
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
    async fn max_parallel_channels_generations_share_rotate_and_keep_their_limits() {
        let controller = TransferConcurrencyController::default();

        let first = controller.generation(4).unwrap();
        let same = controller.generation(4).unwrap();
        let next = controller.generation(8).unwrap();

        assert!(Arc::ptr_eq(&first.semaphore, &same.semaphore));
        assert!(!Arc::ptr_eq(&first.semaphore, &next.semaphore));
        assert_eq!(first.limit(), 4);
        assert_eq!(next.limit(), 8);
        assert_eq!(first.semaphore.available_permits(), 4);
        assert_eq!(next.semaphore.available_permits(), 8);
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
        assert_eq!(generation.semaphore.available_permits(), 0);

        let queued_generation = generation.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let queued_cancelled = cancelled.clone();
        let waiter = tokio::spawn(async move {
            queued_generation.acquire(&queued_cancelled).await
        });
        tokio::task::yield_now().await;
        cancelled.store(true, Ordering::Release);

        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("cancelled permit wait should finish")
            .unwrap();
        assert_eq!(result.unwrap_err(), TransferPermitError::Cancelled);

        drop(held);
        assert_eq!(generation.semaphore.available_permits(), 4);
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
            assert_eq!(generation.semaphore.available_permits(), 0);
            assert!(generation.semaphore.clone().try_acquire_owned().is_err());
            drop(held);
            assert_eq!(
                generation.semaphore.available_permits(),
                usize::from(limit)
            );
        }
    }
}
