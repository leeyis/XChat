# Cross-Subnet Verified Endpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent NAT-rewritten UDP source addresses from replacing a device's identity-verified fixed endpoint while preserving ordinary LAN discovery.

**Architecture:** `network::peer_identity` will derive one deterministic, validated `device_id -> endpoint` snapshot from persisted custom-peer records. Both desktop and web UDP listeners will share one pure endpoint selector backed by that snapshot, while UDP replies continue targeting the observed packet source. `PeerManager::load_from_db` will apply the same snapshot to historical peers without changing presence state.

**Tech Stack:** Rust, Tokio, SQLx/SQLite, existing LANChat discovery and peer-management modules

**Spec:** `/Users/eason/Documents/ChatGPT/内网网络排查/2026-08-20-xchat-cross-subnet-endpoint-handoff.md`

## Global Constraints

- Do not change the UI, UDP/HTTP/WebSocket wire formats, or SQLite schema.
- Only identity-verified `CustomPeerRecord` values with a matching non-empty `device_id` may override an observed UDP source.
- Preserve fixed-endpoint ports; never combine a fixed endpoint host with the announced UDP port.
- Keep `FixedPeerSourceResolver::identity_allowed` and the sending layer's backend-address preference intact.
- Keep UDP discovery replies directed to the observed source address.
- Do not run repository-wide `cargo fmt`; format only touched Rust files if needed and inspect the diff immediately.

---

### Task 1: Deterministic verified endpoint snapshot

**Files:**
- Modify: `src-tauri/src/network/peer_identity.rs`

**Interfaces:**
- Consumes: `crate::db::CustomPeerRecord` and existing `normalize_peer_endpoint`.
- Produces: `pub(crate) fn verified_endpoints_by_device_id(records: &[CustomPeerRecord]) -> HashMap<String, String>`.

- [ ] **Step 1: Write failing mapping tests**

Add tests proving that legacy/unverified or malformed records are ignored, the newest `last_verified_at` wins, `None` is oldest, and equal timestamps choose the lexicographically smallest normalized endpoint. Use literal expected maps so the assertions do not reuse production selection logic.

```rust
fn custom_peer(
    endpoint: &str,
    device_id: Option<&str>,
    last_verified_at: Option<i64>,
) -> crate::db::CustomPeerRecord {
    crate::db::CustomPeerRecord {
        endpoint: endpoint.into(),
        device_id: device_id.map(str::to_string),
        name: None,
        hostname: None,
        mac_address: None,
        app_version: None,
        last_verified_at,
    }
}

#[test]
fn verified_endpoint_snapshot_ignores_unverified_and_invalid_records() {
    let records = vec![
        custom_peer("192.168.20.105:8888", None, Some(10)),
        custom_peer("not/a/peer", Some("peer-invalid"), Some(20)),
    ];

    assert!(verified_endpoints_by_device_id(&records).is_empty());
}

#[test]
fn verified_endpoint_snapshot_is_deterministic_for_duplicate_device_ids() {
    let records = vec![
        custom_peer("192.168.20.109:8888", Some("peer-latest"), Some(10)),
        custom_peer("192.168.20.105:18888", Some("peer-latest"), Some(20)),
        custom_peer("peer-z.local:8888", Some("peer-tie"), Some(30)),
        custom_peer("peer-a.local:8888", Some("peer-tie"), Some(30)),
        custom_peer("192.168.20.111:8888", Some("peer-none"), None),
        custom_peer("192.168.20.112:8888", Some("peer-none"), Some(-1)),
    ];

    let endpoints = verified_endpoints_by_device_id(&records);
    assert_eq!(endpoints["peer-latest"], "192.168.20.105:18888");
    assert_eq!(endpoints["peer-tie"], "peer-a.local:8888");
    assert_eq!(endpoints["peer-none"], "192.168.20.112:8888");
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib verified_endpoint_snapshot
```

Expected: compilation fails because `verified_endpoints_by_device_id` does not exist.

- [ ] **Step 3: Implement the minimal snapshot builder**

Add `HashMap` to the imports and implement one pass over records. Trim the device ID, require `record.is_verified()`, require a stored endpoint with an explicit port, normalize it, and compare `(last_verified_at, endpoint)` with `None < Some(_)` and lexical ascending as the tie-breaker.

```rust
pub(crate) fn verified_endpoints_by_device_id(
    records: &[crate::db::CustomPeerRecord],
) -> HashMap<String, String> {
    let mut selected = HashMap::<String, (Option<i64>, String)>::new();
    for record in records {
        let Some(device_id) = record
            .device_id
            .as_deref()
            .map(str::trim)
            .filter(|id| record.is_verified() && !id.is_empty())
        else {
            continue;
        };
        let raw = record.endpoint.trim();
        if raw.parse::<IpAddr>().is_ok() || !raw.contains(':') {
            continue;
        }
        let Ok(endpoint) = normalize_peer_endpoint(raw, 8888) else {
            continue;
        };
        let replace = selected.get(device_id).is_none_or(|(time, current)| {
            record.last_verified_at > *time
                || (record.last_verified_at == *time
                    && endpoint.as_str() < current.as_str())
        });
        if replace {
            selected.insert(device_id.to_string(), (record.last_verified_at, endpoint));
        }
    }
    selected
        .into_iter()
        .map(|(device_id, (_, endpoint))| (device_id, endpoint))
        .collect()
}
```

- [ ] **Step 4: Verify GREEN**

Run the focused command from Step 2 and expect both mapping tests to pass.

- [ ] **Step 5: Commit**

```bash
rtk git add docs/superpowers/plans/2026-08-20-cross-subnet-endpoint.md src-tauri/src/network/peer_identity.rs
rtk git commit -m "fix(network): derive verified peer endpoints"
```

### Task 2: Select the authoritative endpoint in both UDP listeners

**Files:**
- Modify: `src-tauri/src/network/discovery.rs`

**Interfaces:**
- Consumes: `peer_identity::verified_endpoints_by_device_id` and `DiscoveryAnnouncement::port`.
- Produces: `select_peer_endpoint(peer_id, observed_ip, announced_port, verified_endpoints) -> SelectedPeerEndpoint`; `FixedPeerSourceResolver::verified_endpoints_by_device_id`.

- [ ] **Step 1: Write failing endpoint-selection tests**

Add separate tests for the NAT regression, ordinary LAN fallback, a fixed record belonging to another ID, fixed-port authority, and fallback after the resolver snapshot is cleared.

```rust
#[test]
fn verified_endpoint_overrides_nat_rewritten_udp_source() {
    let endpoints = HashMap::from([(
        "peer-20".to_string(),
        "192.168.20.105:8888".to_string(),
    )]);
    let selected = select_peer_endpoint(
        "peer-20",
        "192.168.10.120".parse().unwrap(),
        8888,
        &endpoints,
    );
    assert_eq!(selected.endpoint, "192.168.20.105:8888");
    assert_eq!(selected.source, PeerEndpointSource::VerifiedFixed);
}

#[test]
fn ordinary_discovery_uses_observed_udp_endpoint() {
    let selected = select_peer_endpoint(
        "peer-lan",
        "192.168.20.106".parse().unwrap(),
        8888,
        &HashMap::new(),
    );
    assert_eq!(selected.endpoint, "192.168.20.106:8888");
    assert_eq!(selected.source, PeerEndpointSource::ObservedUdp);
}

#[test]
fn fixed_endpoint_for_another_device_is_not_reused() {
    let endpoints = HashMap::from([(
        "peer-a".to_string(),
        "192.168.20.105:8888".to_string(),
    )]);
    let selected = select_peer_endpoint(
        "peer-b",
        "192.168.20.106".parse().unwrap(),
        8888,
        &endpoints,
    );
    assert_eq!(selected.endpoint, "192.168.20.106:8888");
    assert_eq!(selected.source, PeerEndpointSource::ObservedUdp);
}

#[test]
fn verified_endpoint_keeps_its_verified_port() {
    let endpoints = HashMap::from([(
        "peer-20".to_string(),
        "192.168.20.105:18888".to_string(),
    )]);
    let selected = select_peer_endpoint(
        "peer-20",
        "192.168.10.120".parse().unwrap(),
        8888,
        &endpoints,
    );
    assert_eq!(selected.endpoint, "192.168.20.105:18888");
}

#[test]
fn removing_verified_endpoint_restores_observed_udp_endpoint() {
    let mut resolver = FixedPeerSourceResolver::default();
    resolver.bind_verified_identities(&[crate::db::CustomPeerRecord {
        endpoint: "192.168.20.105:8888".into(),
        device_id: Some("peer-20".into()),
        name: None,
        hostname: None,
        mac_address: None,
        app_version: None,
        last_verified_at: Some(20),
    }]);
    assert_eq!(
        select_peer_endpoint(
            "peer-20",
            "192.168.10.120".parse().unwrap(),
            8888,
            &resolver.verified_endpoints_by_device_id,
        )
        .endpoint,
        "192.168.20.105:8888",
    );

    resolver.bind_verified_identities(&[]);
    assert_eq!(
        select_peer_endpoint(
            "peer-20",
            "192.168.10.120".parse().unwrap(),
            8888,
            &resolver.verified_endpoints_by_device_id,
        )
        .endpoint,
        "192.168.10.120:8888",
    );
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib peer_endpoint
```

Expected: compilation fails because the selector, source enum, and resolver endpoint snapshot do not exist.

- [ ] **Step 3: Implement the pure selector and resolver snapshot**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerEndpointSource {
    VerifiedFixed,
    ObservedUdp,
}

struct SelectedPeerEndpoint {
    endpoint: String,
    source: PeerEndpointSource,
}

fn select_peer_endpoint(
    peer_id: &str,
    observed_ip: IpAddr,
    announced_port: u16,
    verified_endpoints: &HashMap<String, String>,
) -> SelectedPeerEndpoint {
    match verified_endpoints.get(peer_id) {
        Some(endpoint) => SelectedPeerEndpoint {
            endpoint: endpoint.clone(),
            source: PeerEndpointSource::VerifiedFixed,
        },
        None => SelectedPeerEndpoint {
            endpoint: SocketAddr::new(observed_ip, announced_port).to_string(),
            source: PeerEndpointSource::ObservedUdp,
        },
    }
}
```

Extend `FixedPeerSourceResolver` with the snapshot and rebuild it in `bind_verified_identities`. Do not weaken or bypass `expected_ids_by_source`.

```rust
#[derive(Default)]
struct FixedPeerSourceResolver {
    dns_cache: DnsCache,
    retry_states: HashMap<String, FixedPeerRetryState>,
    resolved_by_endpoint: HashMap<String, HashSet<Ipv4Addr>>,
    expected_ids_by_source: HashMap<Ipv4Addr, HashSet<String>>,
    verified_endpoints_by_device_id: HashMap<String, String>,
    cursor: usize,
}

fn bind_verified_identities(&mut self, records: &[crate::db::CustomPeerRecord]) {
    self.verified_endpoints_by_device_id =
        super::peer_identity::verified_endpoints_by_device_id(records);
    self.expected_ids_by_source.clear();
    for record in records {
        let Some(device_id) = record
            .device_id
            .as_deref()
            .map(str::trim)
            .filter(|device_id| !device_id.is_empty())
        else {
            continue;
        };
        let Some(sources) = self.resolved_by_endpoint.get(&record.endpoint) else {
            continue;
        };
        for source in sources {
            self.expected_ids_by_source
                .entry(*source)
                .or_default()
                .insert(device_id.to_string());
        }
    }
}
```

- [ ] **Step 4: Wire both listener implementations to the selector**

Use `announcement.port` as the parsed `u16`. Compute the selected endpoint once and use it unchanged for `PeerManager`, SQLite persistence, emitted events, legacy pending-message resend, and workspace resend. Log an override only for a new/reconnected peer or when the selected endpoint differs from the previous in-memory endpoint. Leave `send_discovery_reply` targeting `SocketAddr::new(addr.ip(), announcement.port)`.

Apply this block in both desktop and web listeners immediately after deduplication:

```rust
let observed_peer_addr = SocketAddr::new(addr.ip(), announcement.port).to_string();
let selected = select_peer_endpoint(
    &peer_id,
    addr.ip(),
    announcement.port,
    &fixed_peer_resolver.verified_endpoints_by_device_id,
);
let using_verified_override = selected.source == PeerEndpointSource::VerifiedFixed
    && selected.endpoint != observed_peer_addr;
let endpoint_changed = using_verified_override
    && peer_manager
        .get_all_peers()
        .into_iter()
        .find(|peer| peer.id == peer_id)
        .is_none_or(|peer| peer.addr != selected.endpoint);
let peer_addr = selected.endpoint;
```

After `add_or_update_with_details`, emit the diagnostic only when `using_verified_override && (is_new_or_reconnected || endpoint_changed)`. Every downstream consumer in that listener block receives `peer_addr`. The UDP reply target continues to use `addr.ip()` and `announcement.port` directly.

- [ ] **Step 5: Verify GREEN**

Run the focused command from Step 2 and expect all endpoint-selection tests to pass.

- [ ] **Step 6: Commit**

```bash
rtk git add src-tauri/src/network/discovery.rs
rtk git commit -m "fix(network): prefer verified endpoints in discovery"
```

### Task 3: Reconcile historical peer addresses at startup

**Files:**
- Modify: `src-tauri/src/peers.rs`

**Interfaces:**
- Consumes: `peer_identity::verified_endpoints_by_device_id`.
- Produces: private `reconcile_verified_peer_endpoints(&mut HashMap<String, Peer>, &HashMap<String, String>)` used by `PeerManager::load_from_db`.

- [ ] **Step 1: Write the failing reconciliation test**

```rust
#[test]
fn verified_endpoint_reconciles_historical_address_without_changing_presence() {
    let mut peers = HashMap::from([(
        "peer-20".to_string(),
        Peer {
            id: "peer-20".into(),
            name: "Mac".into(),
            addr: "192.168.10.120:8888".into(),
            last_seen: 42,
            is_offline: true,
            available_memory_mb: 0,
            hostname: None,
            mac_address: None,
            remark: None,
            discovery_source: Some("lan".into()),
            capabilities: Vec::new(),
            app_version: None,
        },
    )]);
    let endpoints = HashMap::from([(
        "peer-20".to_string(),
        "192.168.20.105:8888".to_string(),
    )]);

    reconcile_verified_peer_endpoints(&mut peers, &endpoints);

    assert_eq!(peers["peer-20"].addr, "192.168.20.105:8888");
    assert!(peers["peer-20"].is_offline);
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib verified_endpoint_reconciles
```

Expected: compilation fails because the reconciliation helper does not exist.

- [ ] **Step 3: Implement startup reconciliation**

In `load_from_db`, load custom-peer records, derive the verified endpoint snapshot, build historical peers exactly as today, and then call the helper while holding the existing write lock. The helper may only replace `Peer.addr`; it must not modify `last_seen`, `is_offline`, metadata, or persistence.

```rust
fn reconcile_verified_peer_endpoints(
    peers: &mut HashMap<String, Peer>,
    verified_endpoints: &HashMap<String, String>,
) {
    for (device_id, endpoint) in verified_endpoints {
        if let Some(peer) = peers.get_mut(device_id) {
            peer.addr = endpoint.clone();
        }
    }
}

pub async fn load_from_db(&self, pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<(), String> {
    println!("[PeerManager] 从数据库加载历史用户...");
    let users = crate::db::list_users_with_metadata(pool).await?;
    let custom_peers = crate::db::get_custom_peer_records(pool).await;
    let verified_endpoints =
        crate::network::peer_identity::verified_endpoints_by_device_id(&custom_peers);
    let mut peers = self.peers.write().unwrap();
    for user in users {
        let peer = Peer {
            id: user.id.clone(),
            name: user.name,
            addr: user.addr,
            last_seen: user.last_seen as u64,
            is_offline: user.is_offline,
            available_memory_mb: user.available_memory_mb as u64,
            hostname: user.hostname,
            mac_address: user.mac_address,
            remark: user.remark,
            discovery_source: user.discovery_source,
            capabilities: Vec::new(),
            app_version: user.app_version,
        };
        peers.insert(user.id, peer);
    }
    reconcile_verified_peer_endpoints(&mut peers, &verified_endpoints);
    println!("[PeerManager] 已加载 {} 个历史用户", peers.len());
    Ok(())
}
```

- [ ] **Step 4: Verify GREEN**

Run the focused command from Step 2 and expect the reconciliation test to pass.

- [ ] **Step 5: Commit**

```bash
rtk git add src-tauri/src/peers.rs
rtk git commit -m "fix(network): reconcile stored peer endpoints"
```

### Task 4: Audit and regression verification

**Files:**
- Inspect: `src-tauri/src/commands.rs`
- Inspect: `src-tauri/src/network/messaging.rs`
- Inspect: `src-tauri/src/network/conversation_file.rs`
- Inspect: `src-tauri/src/network/protocol.rs`
- Inspect: all modified files

**Interfaces:**
- Consumes: the selected `Peer.addr` propagated by discovery and startup loading.
- Produces: verified desktop and web builds with no sending-layer workaround or protocol/schema/UI changes.

- [ ] **Step 1: Audit address consumers**

Confirm through graph traces and targeted source inspection that messages, files, receipts, recalls, and pending resends consume either the listener-selected address or the current `PeerManager` address. Do not add per-feature endpoint overrides.

- [ ] **Step 2: Run focused and full tests**

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib peer_endpoint
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib verified_endpoint
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib
```

- [ ] **Step 3: Compile both supported feature paths**

```bash
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop --lib
rtk cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features web --bin lanchat-web
```

- [ ] **Step 4: Check the final diff**

```bash
rtk git diff main...HEAD --check
rtk git diff main...HEAD --stat
rtk git status --short --branch
```

Confirm there are no UI, schema, wire-format, generated Android, or broad formatting changes.

- [ ] **Step 5: Merge after review**

Run the repository code-review workflow against `main`, address any verified findings, repeat the relevant tests, then use the finishing-development-branch workflow to merge `agent/fix-cross-subnet-endpoint` into `main` as explicitly requested.
