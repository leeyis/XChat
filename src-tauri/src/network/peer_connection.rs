//! Candidate discovery is separate from a verified message-service connection.
use crate::peers::PeerManager;
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const VERIFIED_TTL: u64 = 30;
const CANDIDATE_TTL: u64 = 90;
const MAX_CANDIDATES: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConnectionStatus {
    pub peer_id: String,
    pub status: String,
    pub address: String,
    pub previous_address: Option<String>,
    pub verified_at: Option<u64>,
    pub error: Option<String>,
}

impl PeerConnectionStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self.status.as_str(), "ready" | "updated")
    }
}

struct ConnectionEntry {
    status: PeerConnectionStatus,
    observed: Vec<(String, u64)>,
    gate: Arc<tokio::sync::Mutex<()>>,
    persistence_gate: Arc<tokio::sync::Mutex<()>>,
    revision: u64,
    completed: Option<Instant>,
    scheduled: bool,
}

#[derive(Default)]
pub(crate) struct ConnectionRegistry {
    entries: Mutex<HashMap<String, ConnectionEntry>>,
    discovery_gate: tokio::sync::Mutex<Option<Instant>>,
}

fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

impl ConnectionRegistry {
    pub(crate) fn remove(&self, id: &str) {
        self.entries.lock().unwrap().remove(id);
    }

    pub(crate) fn observe(&self, id: &str, address: &str) -> String {
        let timestamp = now();
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .entry(id.to_string())
            .or_insert_with(|| ConnectionEntry {
                status: PeerConnectionStatus {
                    peer_id: id.to_string(),
                    status: "stale".into(),
                    address: address.to_string(),
                    previous_address: None,
                    verified_at: None,
                    error: None,
                },
                observed: Vec::new(),
                gate: Arc::new(tokio::sync::Mutex::new(())),
                persistence_gate: Arc::new(tokio::sync::Mutex::new(())),
                revision: 0,
                completed: None,
                scheduled: false,
            });
        entry
            .observed
            .retain(|(_, seen)| timestamp.saturating_sub(*seen) <= CANDIDATE_TTL);
        if let Some((_, seen)) = entry
            .observed
            .iter_mut()
            .find(|(candidate, _)| candidate == address)
        {
            *seen = timestamp;
        } else if !address.is_empty() {
            entry.observed.push((address.to_string(), timestamp));
            entry.revision = entry.revision.wrapping_add(1);
            // A new address should trigger validation, without replacing the last good address.
            if !matches!(entry.status.status.as_str(), "discovering" | "verifying") {
                entry.status.status = "stale".into();
            }
            entry.completed = None;
        }
        if entry.observed.len() > MAX_CANDIDATES {
            entry
                .observed
                .sort_by_key(|(_, seen)| std::cmp::Reverse(*seen));
            entry.observed.truncate(MAX_CANDIDATES);
        }
        entry.status.address.clone()
    }

    pub(crate) fn snapshot(&self, id: &str) -> Option<PeerConnectionStatus> {
        let entries = self.entries.lock().unwrap();
        let mut status = entries.get(id)?.status.clone();
        if status.is_connected()
            && status
                .verified_at
                .is_none_or(|verified| now().saturating_sub(verified) > VERIFIED_TTL)
        {
            status.status = "stale".into();
        }
        Some(status)
    }

    fn update(&self, id: &str, phase: &str, error: Option<String>) -> Option<PeerConnectionStatus> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get_mut(id)?;
        entry.status.status = phase.to_string();
        entry.status.error = error;
        Some(entry.status.clone())
    }

    pub(crate) async fn persistence_guard(
        &self,
        id: &str,
    ) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        let gate = self
            .entries
            .lock()
            .unwrap()
            .get(id)?
            .persistence_gate
            .clone();
        Some(gate.lock_owned().await)
    }

    fn revision(&self, id: &str) -> Option<u64> {
        self.entries
            .lock()
            .unwrap()
            .get(id)
            .map(|entry| entry.revision)
    }
}

fn publish(status: &PeerConnectionStatus) {
    crate::web_server::publish_peer_connection_changed(status);
}

pub fn invalidate_peer_connection(manager: &PeerManager, peer_id: &str, error: &str) {
    if let Some(status) = manager
        .connections
        .update(peer_id, "missing", Some(error.to_string()))
    {
        publish(&status);
    }
}

pub fn network_changed(pool: &Pool<Sqlite>, manager: &PeerManager) {
    let statuses = {
        let mut entries = manager.connections.entries.lock().unwrap();
        entries
            .values_mut()
            .map(|entry| {
                entry.revision = entry.revision.wrapping_add(1);
                entry.completed = None;
                entry.status.status = "stale".into();
                entry.status.verified_at = None;
                entry.status.error = None;
                entry.status.clone()
            })
            .collect::<Vec<_>>()
    };
    for status in statuses {
        publish(&status);
        schedule_validation(pool, manager, &status.peer_id);
    }
}

fn probe_budget() -> &'static tokio::sync::Semaphore {
    static BUDGET: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    BUDGET.get_or_init(|| tokio::sync::Semaphore::new(12))
}

fn automatic_candidate_allowed(
    snapshot: &super::discovery_policy::DiscoveryNetworkSnapshot,
    address: &str,
) -> bool {
    // UDP connect selects a route without transmitting a datagram. Check that the
    // actual egress interface is enabled, including cached addresses from a VPN.
    let Ok(destination) = address.parse::<std::net::SocketAddr>() else {
        return false;
    };
    let bind = if destination.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let Ok(socket) = std::net::UdpSocket::bind(bind) else {
        return false;
    };
    if socket.connect(destination).is_err() {
        return false;
    }
    let Ok(local) = socket.local_addr() else {
        return false;
    };
    route_allowed(snapshot, local.ip())
}

fn route_allowed(
    snapshot: &super::discovery_policy::DiscoveryNetworkSnapshot,
    source: std::net::IpAddr,
) -> bool {
    snapshot.interfaces.iter().any(|interface| {
        interface.enabled
            && interface
                .addresses
                .iter()
                .any(|address| address.ipv4.parse::<std::net::IpAddr>().ok() == Some(source))
    })
}

fn prioritize_candidates(
    fixed: Vec<String>,
    observed: Vec<String>,
    previous: Option<String>,
) -> Vec<String> {
    let mut result = Vec::new();
    // Reserve space for recent discovery even after many manual DHCP corrections.
    for address in fixed
        .iter()
        .take(1)
        .chain(observed.iter().take(2))
        .chain(previous.iter())
        .chain(observed.iter().skip(2))
        .chain(fixed.iter().skip(1))
    {
        if !address.is_empty() && !result.contains(address) {
            result.push(address.clone());
            if result.len() == MAX_CANDIDATES {
                break;
            }
        }
    }
    result
}

async fn candidates(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    peer_id: &str,
) -> (Vec<String>, bool) {
    let records = crate::db::get_custom_peer_records(pool).await;
    let mut fixed = records
        .into_iter()
        .filter(|record| record.device_id.as_deref() == Some(peer_id))
        .collect::<Vec<_>>();
    fixed.sort_by_key(|record| std::cmp::Reverse(record.last_verified_at));
    let has_fixed = !fixed.is_empty();
    let snapshot = super::discovery_policy::network_snapshot(pool).await.ok();
    let auto_enabled = snapshot.as_ref().is_some_and(|snapshot| {
        !super::discovery_policy::build_send_plan(
            snapshot,
            super::discovery::active_discovery_port(),
        )
        .targets
        .is_empty()
    });
    let fixed = fixed
        .into_iter()
        .filter_map(|record| {
            super::peer_identity::normalize_peer_endpoint(&record.endpoint, 8888).ok()
        })
        .collect::<Vec<_>>();
    let mut recent = Vec::new();
    let mut previous = None;
    if auto_enabled {
        if let Some(entry) = manager.connections.entries.lock().unwrap().get(peer_id) {
            if snapshot.as_ref().is_some_and(|snapshot| {
                automatic_candidate_allowed(snapshot, &entry.status.address)
            }) {
                // Persisted addresses remain probe candidates even after UDP observations expire.
                previous = Some(entry.status.address.clone());
            }
            let mut observed = entry
                .observed
                .iter()
                .rev()
                .filter(|(_, seen)| now().saturating_sub(*seen) <= CANDIDATE_TTL)
                .cloned()
                .collect::<Vec<_>>();
            observed.sort_by_key(|(_, seen)| std::cmp::Reverse(*seen));
            for (address, _) in observed {
                if snapshot
                    .as_ref()
                    .is_some_and(|snapshot| automatic_candidate_allowed(snapshot, &address))
                {
                    recent.push(address);
                }
            }
        }
    }
    (
        prioritize_candidates(fixed, recent, previous),
        auto_enabled || has_fixed,
    )
}

/// Validate known candidates. Concurrent callers share the completed result of one attempt.
pub async fn resolve_peer_connection(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    peer_id: &str,
) -> Result<PeerConnectionStatus, String> {
    let requested = Instant::now();
    let gate = {
        let entries = manager.connections.entries.lock().unwrap();
        entries
            .get(peer_id)
            .ok_or_else(|| "设备不存在".to_string())?
            .gate
            .clone()
    };
    let _guard = gate.lock().await;
    {
        let entries = manager.connections.entries.lock().unwrap();
        let entry = entries
            .get(peer_id)
            .ok_or_else(|| "设备不存在".to_string())?;
        if entry
            .completed
            .is_some_and(|completed| completed >= requested)
        {
            return Ok(entry.status.clone());
        }
    }
    let previous = manager
        .connection_snapshot(peer_id)
        .ok_or_else(|| "设备不存在".to_string())?
        .address;
    if let Some(status) = manager.connections.update(peer_id, "verifying", None) {
        publish(&status);
    }
    let deadline = Instant::now() + Duration::from_secs(7);
    let mut attempted = Vec::new();
    let mut errors = Vec::new();
    let mut selected = None;
    let mut enabled = true;
    let mut checked_revision = manager.connections.revision(peer_id);
    // Re-read after probing so a concurrent discovery packet can contribute its new address.
    for _ in 0..2 {
        checked_revision = manager.connections.revision(peer_id);
        let (addresses, allowed) = candidates(pool, manager, peer_id).await;
        enabled = allowed;
        let pending = addresses
            .into_iter()
            .filter(|address| !attempted.contains(address))
            .collect::<Vec<_>>();
        attempted.extend(pending.iter().cloned());
        let probes = stream::iter(pending.into_iter().enumerate().map(
            |(order, address)| async move {
                let result = async {
                    let _permit = probe_budget()
                        .acquire()
                        .await
                        .map_err(|error| error.to_string())?;
                    super::messaging::verify_peer_connection(&address, peer_id).await
                };
                let probe_deadline = deadline.min(Instant::now() + Duration::from_secs(4));
                let result =
                    tokio::time::timeout_at(tokio::time::Instant::from_std(probe_deadline), result)
                        .await
                        .unwrap_or_else(|_| Err("消息连接验证超时".into()));
                (order, address, result)
            },
        ))
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await;
        let mut successes = Vec::new();
        for (order, address, result) in probes {
            match result {
                Ok(()) => successes.push((order, address)),
                Err(error) => errors.push(error),
            }
        }
        successes.sort_by_key(|(order, _)| *order);
        if let Some((_, address)) = successes.into_iter().next() {
            selected = Some(address);
            if manager.connections.revision(peer_id) == checked_revision {
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    let _persistence = manager
        .connections
        .persistence_guard(peer_id)
        .await
        .ok_or_else(|| "设备已删除".to_string())?;
    let (phase, address, error, verified_at) = if let Some(address) = selected {
        // Write the chosen address before publishing it. All validation for a device is serialized.
        sqlx::query("UPDATE users SET addr = ?, last_seen = ?, is_offline = 0 WHERE id = ?")
            .bind(&address)
            .bind(now() as i64)
            .bind(peer_id)
            .execute(pool)
            .await
            .map_err(|error| error.to_string())?;
        if !manager.set_verified_address(peer_id, &address) {
            return Err("设备已删除".into());
        }
        (
            if previous == address {
                "ready"
            } else {
                "updated"
            },
            address,
            None,
            Some(now()),
        )
    } else {
        let mismatch = errors
            .iter()
            .any(|error| error.contains("身份") || error.contains("409"));
        let phase = if !enabled {
            "policy_disabled"
        } else if mismatch {
            "mismatch"
        } else {
            "missing"
        };
        let error = if !enabled {
            "自动发现已关闭且没有可验证的固定地址".to_string()
        } else {
            errors
                .into_iter()
                .next()
                .unwrap_or_else(|| "暂未发现可连接的地址".to_string())
        };
        (phase, previous.clone(), Some(error), None)
    };
    let status = {
        let mut entries = manager.connections.entries.lock().unwrap();
        let entry = entries
            .get_mut(peer_id)
            .ok_or_else(|| "设备已删除".to_string())?;
        entry.status = PeerConnectionStatus {
            peer_id: peer_id.to_string(),
            status: if Some(entry.revision) != checked_revision {
                "stale".into()
            } else {
                phase.into()
            },
            address,
            previous_address: if phase == "updated" {
                Some(previous)
            } else {
                entry.status.previous_address.clone()
            },
            verified_at: verified_at.or(entry.status.verified_at),
            error,
        };
        entry.completed = if Some(entry.revision) == checked_revision {
            Some(Instant::now())
        } else {
            None
        };
        entry.status.clone()
    };
    publish(&status);
    Ok(status)
}

pub async fn ensure_peer_connection(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    peer_id: &str,
) -> Result<String, String> {
    if let Some(status) = manager.connection_snapshot(peer_id) {
        if status.is_connected() {
            return Ok(status.address);
        }
    }
    let result = resolve_peer_connection(pool, manager, peer_id).await?;
    if result.is_connected() {
        Ok(result.address)
    } else {
        Err(result.error.unwrap_or_else(|| "设备连接未确认".into()))
    }
}

async fn discover(pool: &Pool<Sqlite>, manager: &PeerManager) {
    let mut last = manager.connections.discovery_gate.lock().await;
    if last.is_some_and(|last| last.elapsed() < Duration::from_secs(2)) {
        return;
    }
    super::discovery_policy::notify_settings_changed();
    // Let replies arrive on the existing listener; don't create a competing UDP listener.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let _ = pool;
    *last = Some(Instant::now());
}

pub async fn refresh_peer_connection(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    peer_id: &str,
) -> Result<PeerConnectionStatus, String> {
    if let Some(status) = manager.connections.update(peer_id, "discovering", None) {
        publish(&status);
    } else {
        return Err("设备不存在".into());
    }
    discover(pool, manager).await;
    resolve_and_resume(pool, manager, peer_id).await
}

async fn resolve_and_resume(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    peer_id: &str,
) -> Result<PeerConnectionStatus, String> {
    let result = resolve_peer_connection(pool, manager, peer_id).await?;
    if result.is_connected() {
        crate::db::wake_delivery_for_peer(pool, peer_id).await?;
        let pool = pool.clone();
        let manager = manager.clone();
        let peer = peer_id.to_string();
        let address = result.address.clone();
        tokio::spawn(async move {
            let _ = crate::workspace::resend_for_peer(&pool, &manager, &peer, &address).await;
        });
    }
    Ok(result)
}

pub async fn rediscover_peers(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
) -> Result<Vec<PeerConnectionStatus>, String> {
    discover(pool, manager).await;
    let peers = manager.get_all_peers();
    let results = stream::iter(peers.into_iter().map(|peer| {
        let pool = pool.clone();
        let manager = manager.clone();
        async move { resolve_and_resume(&pool, &manager, &peer.id).await }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;
    results.into_iter().collect()
}

pub fn schedule_validation(pool: &Pool<Sqlite>, manager: &PeerManager, peer_id: &str) {
    let resume_pending = {
        let mut entries = manager.connections.entries.lock().unwrap();
        let Some(entry) = entries.get_mut(peer_id) else {
            return;
        };
        if entry.scheduled
            || entry
                .completed
                .is_some_and(|last| last.elapsed() < Duration::from_secs(10))
        {
            return;
        }
        if entry.status.is_connected()
            && entry
                .status
                .verified_at
                .is_some_and(|verified| now().saturating_sub(verified) < VERIFIED_TTL)
        {
            return;
        }
        entry.scheduled = true;
        // Routine TTL renewal must not erase each message's ACK retry backoff.
        !entry.status.is_connected()
    };
    let pool = pool.clone();
    let manager = manager.clone();
    let peer_id = peer_id.to_string();
    tokio::spawn(async move {
        let result = resolve_peer_connection(&pool, &manager, &peer_id).await;
        if let Some(entry) = manager
            .connections
            .entries
            .lock()
            .unwrap()
            .get_mut(&peer_id)
        {
            entry.scheduled = false;
        }
        if let Ok(result) = result {
            if result.is_connected() && resume_pending {
                let _ = crate::db::wake_delivery_for_peer(&pool, &peer_id).await;
                let _ =
                    crate::workspace::resend_for_peer(&pool, &manager, &peer_id, &result.address)
                        .await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::discovery_policy::{
        DiscoveryNetworkSnapshot, DiscoverySettings, InterfaceCategory, NetworkInterfaceAddress,
        NetworkInterfaceView,
    };
    use super::*;

    #[test]
    fn connection_candidates_reserve_fresh_addresses_and_respect_egress_policy() {
        let fixed = (1..=9).map(|n| format!("192.168.1.{n}:8888")).collect();
        let fresh = "192.168.1.100:8888".to_string();
        let active = "192.168.1.99:8888".to_string();
        let candidates = prioritize_candidates(fixed, vec![fresh.clone()], Some(active.clone()));
        assert_eq!(candidates.len(), MAX_CANDIDATES);
        assert_eq!(candidates[1], fresh);
        assert_eq!(candidates[2], active);
        let advertised = (101..=104).map(|n| format!("192.168.1.{n}:8888")).collect::<Vec<_>>();
        let candidates = prioritize_candidates(
            (1..=9).map(|n| format!("192.168.1.{n}:8888")).collect(),
            advertised.clone(), Some(active),
        );
        assert!(advertised.iter().all(|address| candidates.contains(address)));
        let interface = |name: &str, ip: &str, enabled| NetworkInterfaceView {
            id: name.into(),
            name: name.into(),
            index: None,
            addresses: vec![NetworkInterfaceAddress {
                ipv4: ip.into(),
                prefix_length: Some(24),
            }],
            category: InterfaceCategory::PhysicalLan,
            is_up: true,
            default_enabled: true,
            selected: enabled,
            enabled,
            exclusion_reason: None,
        };
        let snapshot = DiscoveryNetworkSnapshot {
            settings: DiscoverySettings::default(),
            interfaces: vec![
                interface("lan", "192.168.1.2", true),
                interface("vpn", "10.0.0.2", false),
            ],
        };
        assert!(route_allowed(&snapshot, "192.168.1.2".parse().unwrap()));
        assert!(!route_allowed(&snapshot, "10.0.0.2".parse().unwrap()));
        assert!(!route_allowed(&snapshot, "172.16.1.2".parse().unwrap()));
    }

    async fn test_pool() -> Pool<Sqlite> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for schema in [
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            "CREATE TABLE users (id TEXT PRIMARY KEY, addr TEXT, last_seen INTEGER, is_offline INTEGER)",
        ] {
            sqlx::query(schema).execute(&pool).await.unwrap();
        }
        crate::db::set_setting(
            &pool,
            "network.discovery.settings.v1",
            r#"{"local_discovery":false,"vpn_discovery":false}"#,
        )
        .await
        .unwrap();
        pool
    }

    async fn save_fixed(pool: &Pool<Sqlite>, address: &str, verified: i64) {
        crate::db::save_custom_peer_record(
            pool,
            &crate::db::CustomPeerRecord {
                endpoint: address.into(),
                device_id: Some("expected".into()),
                name: None,
                hostname: None,
                mac_address: None,
                app_version: None,
                last_verified_at: Some(verified),
            },
        )
        .await
        .unwrap();
    }

    async fn identity_server(identity: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        use futures_util::StreamExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(stream, move |
                _: &tokio_tungstenite::tungstenite::handshake::server::Request,
                mut response: tokio_tungstenite::tungstenite::handshake::server::Response,
            | {
                response.headers_mut().insert("x-xchat-device-id", identity.parse().unwrap());
                Ok(response)
            }).await.unwrap();
            while let Some(Ok(message)) = socket.next().await {
                assert!(
                    !message.is_text(),
                    "verification must never send message contents"
                );
                if message.is_close() {
                    break;
                }
            }
        });
        (address, task)
    }

    #[tokio::test]
    async fn connection_rejects_reassigned_ip_then_atomically_commits_verified_fallback() {
        let pool = test_pool().await;
        let manager = PeerManager::new();
        let (wrong, wrong_task) = identity_server("different-device").await;
        manager.add_or_update("expected".into(), "Peer".into(), wrong.clone());
        sqlx::query("INSERT INTO users VALUES ('expected', ?, 0, 1)")
            .bind(&wrong)
            .execute(&pool)
            .await
            .unwrap();
        save_fixed(&pool, &wrong, 2).await;
        // Concurrent requests must share one identity probe and both retain the old address on failure.
        let (first, second) = tokio::join!(
            resolve_peer_connection(&pool, &manager, "expected"),
            resolve_peer_connection(&pool, &manager, "expected")
        );
        for outcome in [first.unwrap(), second.unwrap()] {
            assert_eq!(outcome.status, "mismatch");
            assert_eq!(outcome.address, wrong);
        }
        wrong_task.await.unwrap();
        let (good, good_task) = identity_server("expected").await;
        save_fixed(&pool, &good, 1).await;
        manager.connections.observe("expected", &good);
        let outcome = resolve_peer_connection(&pool, &manager, "expected")
            .await
            .unwrap();
        assert_eq!(outcome.status, "updated");
        assert_eq!(outcome.previous_address, Some(wrong));
        assert_eq!(outcome.address, good);
        assert_eq!(
            manager
                .get_all_peers()
                .into_iter()
                .find(|peer| peer.id == "expected")
                .unwrap()
                .addr,
            good
        );
        let saved: String = sqlx::query_scalar("SELECT addr FROM users WHERE id = 'expected'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(saved, good);
        // A late discovery packet supplies a candidate but cannot roll back the verified address.
        manager.add_or_update("expected".into(), "Peer".into(), "192.0.2.1:8888".into());
        assert_eq!(
            manager
                .get_all_peers()
                .into_iter()
                .find(|peer| peer.id == "expected")
                .unwrap()
                .addr,
            good
        );
        good_task.await.unwrap();
    }
}
