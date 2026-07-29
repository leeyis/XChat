use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
};

pub type TransferCancellationToken = Arc<AtomicBool>;

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
}
