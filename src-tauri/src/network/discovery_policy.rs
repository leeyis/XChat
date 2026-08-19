use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::OnceLock;

const DISCOVERY_MULTICAST: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
const DISCOVERY_SETTINGS_KEY: &str = "network.discovery.settings.v1";
pub(crate) const MAX_INTERFACE_DATAGRAMS_PER_CYCLE: usize = 48;
static DISCOVERY_SETTINGS_CHANGED: OnceLock<tokio::sync::Notify> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceCategory {
    PhysicalLan,
    MeshVpn,
    ProxyTun,
    VirtualMachine,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySettings {
    #[serde(default = "default_enabled")]
    pub local_discovery: bool,
    #[serde(default = "default_enabled")]
    pub vpn_discovery: bool,
    #[serde(default)]
    pub interface_overrides: BTreeMap<String, bool>,
}

impl Default for DiscoverySettings {
    fn default() -> Self {
        Self {
            local_discovery: true,
            vpn_discovery: true,
            interface_overrides: BTreeMap::new(),
        }
    }
}

const fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkInterfaceAddress {
    pub ipv4: String,
    pub prefix_length: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkInterfaceView {
    pub id: String,
    pub name: String,
    pub index: Option<u32>,
    pub addresses: Vec<NetworkInterfaceAddress>,
    pub category: InterfaceCategory,
    pub is_up: bool,
    pub default_enabled: bool,
    pub selected: bool,
    pub enabled: bool,
    pub exclusion_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveryNetworkSnapshot {
    pub settings: DiscoverySettings,
    pub interfaces: Vec<NetworkInterfaceView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscoveryTargetKind {
    Broadcast,
    Multicast,
}

impl DiscoveryTargetKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Broadcast => "broadcast",
            Self::Multicast => "multicast",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveryTarget {
    pub interface_id: String,
    pub interface_index: Option<u32>,
    pub source_ip: Ipv4Addr,
    pub destination: SocketAddrV4,
    pub kind: DiscoveryTargetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoverySendPlan {
    pub targets: Vec<DiscoveryTarget>,
    pub budget: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawInterface {
    name: String,
    system_name: String,
    index: Option<u32>,
    ipv4: Ipv4Addr,
    prefix_length: Option<u8>,
    is_up: bool,
    is_loopback: bool,
}

fn stable_interface_id(interface: &RawInterface) -> String {
    let name = interface.system_name.trim().to_lowercase();
    format!("if:name:{name}")
}

fn migrate_interface_id(id: &str) -> Option<String> {
    let Some(suffix) = id.strip_prefix("if:") else {
        return Some(id.to_string());
    };
    if suffix.parse::<u32>().is_ok() {
        return None;
    }
    let Some((legacy_index, system_name)) = suffix.split_once(':') else {
        return Some(id.to_string());
    };
    if legacy_index.parse::<u32>().is_ok() && !system_name.trim().is_empty() {
        return Some(format!("if:name:{}", system_name.trim().to_lowercase()));
    }
    Some(id.to_string())
}

fn classify_interface(name: &str) -> InterfaceCategory {
    let name = name.trim().to_lowercase();

    if name.contains("wireguard")
        || name.contains("tailscale")
        || name.contains("zerotier")
        || name.contains("zero tier")
        || name.starts_with("wg")
        || name.starts_with("zt")
    {
        return InterfaceCategory::MeshVpn;
    }

    if name.contains("mihomo")
        || name.contains("clash")
        || name.contains("meta tunnel")
        || name.contains("sing-box")
        || name.starts_with("utun")
        || name.starts_with("tun")
        || name.starts_with("tap")
    {
        return InterfaceCategory::ProxyTun;
    }

    if name.contains("docker")
        || name.contains("veth")
        || name.contains("virbr")
        || name.contains("vmnet")
        || name.contains("vmware")
        || name.contains("vbox")
        || name.contains("virtualbox")
        || name.contains("hyper-v")
        || name.contains("vethernet")
        || name.contains("wsl")
        || name.starts_with("br-")
        || name.starts_with("bridge")
    {
        return InterfaceCategory::VirtualMachine;
    }

    // macOS 的通用 utun 名称无法可靠区分组网 VPN 与代理 TUN，保持保守排除；
    // 用户仍可在 A1 设置里显式启用。能识别的真实产品名已在前面的 mesh 分支处理。
    if name == "wifi"
        || name.contains("wi-fi")
        || name.contains("wireless")
        || name.contains("802.11")
        || name.contains("ethernet")
        || name.contains("以太网")
        || name.contains("无线网络")
        || name.starts_with("en")
        || name.starts_with("eth")
        || name.starts_with("wlan")
        || name.starts_with("wl")
        || name.contains("android network")
    {
        return InterfaceCategory::PhysicalLan;
    }

    InterfaceCategory::Unknown
}

fn category_default_enabled(category: InterfaceCategory) -> bool {
    matches!(
        category,
        InterfaceCategory::PhysicalLan | InterfaceCategory::MeshVpn
    )
}

fn category_master_enabled(category: InterfaceCategory, settings: &DiscoverySettings) -> bool {
    match category {
        InterfaceCategory::PhysicalLan => settings.local_discovery,
        InterfaceCategory::MeshVpn => settings.vpn_discovery,
        InterfaceCategory::ProxyTun
        | InterfaceCategory::VirtualMachine
        | InterfaceCategory::Unknown => true,
    }
}

fn exclusion_reason(
    is_up: bool,
    selected: bool,
    master_enabled: bool,
    category: InterfaceCategory,
) -> Option<String> {
    if !is_up {
        Some("interface_down".to_string())
    } else if !selected {
        Some(
            match category {
                InterfaceCategory::ProxyTun => "proxy_tun_default_excluded",
                InterfaceCategory::VirtualMachine => "virtual_interface_default_excluded",
                InterfaceCategory::Unknown => "unknown_interface_default_excluded",
                InterfaceCategory::PhysicalLan | InterfaceCategory::MeshVpn => "user_disabled",
            }
            .to_string(),
        )
    } else if !master_enabled {
        Some("category_disabled".to_string())
    } else {
        None
    }
}

fn network_snapshot_from_raw(
    interfaces: Vec<RawInterface>,
    settings: DiscoverySettings,
) -> DiscoveryNetworkSnapshot {
    let mut grouped = BTreeMap::<String, NetworkInterfaceView>::new();

    for interface in interfaces.into_iter().filter(|interface| {
        !interface.is_loopback && !interface.ipv4.is_loopback() && !interface.ipv4.is_unspecified()
    }) {
        let id = stable_interface_id(&interface);
        let category = classify_interface(&interface.name);
        let default_enabled = category_default_enabled(category);
        let selected = settings
            .interface_overrides
            .get(&id)
            .copied()
            .unwrap_or(default_enabled);
        let master_enabled = category_master_enabled(category, &settings);
        let view = grouped
            .entry(id.clone())
            .or_insert_with(|| NetworkInterfaceView {
                id,
                name: interface.name.clone(),
                index: interface.index,
                addresses: Vec::new(),
                category,
                is_up: interface.is_up,
                default_enabled,
                selected,
                enabled: interface.is_up && selected && master_enabled,
                exclusion_reason: exclusion_reason(
                    interface.is_up,
                    selected,
                    master_enabled,
                    category,
                ),
            });
        view.is_up |= interface.is_up;
        let address = NetworkInterfaceAddress {
            ipv4: interface.ipv4.to_string(),
            prefix_length: interface.prefix_length,
        };
        if !view.addresses.contains(&address) {
            view.addresses.push(address);
        }
    }

    let mut interfaces = grouped.into_values().collect::<Vec<_>>();
    for interface in &mut interfaces {
        interface
            .addresses
            .sort_by(|left, right| left.ipv4.cmp(&right.ipv4));
        let master_enabled = category_master_enabled(interface.category, &settings);
        interface.enabled = interface.is_up && interface.selected && master_enabled;
        interface.exclusion_reason = exclusion_reason(
            interface.is_up,
            interface.selected,
            master_enabled,
            interface.category,
        );
    }

    DiscoveryNetworkSnapshot {
        settings,
        interfaces,
    }
}

fn directed_broadcast(ip: Ipv4Addr, prefix_length: u8) -> Option<Ipv4Addr> {
    if prefix_length > 30 {
        return None;
    }
    let mask = u32::MAX.checked_shl(u32::from(32 - prefix_length))?;
    let broadcast = Ipv4Addr::from(u32::from(ip) | !mask);
    (broadcast != Ipv4Addr::BROADCAST).then_some(broadcast)
}

fn prefix_length(netmask: Ipv4Addr) -> Option<u8> {
    let mask = u32::from(netmask);
    let prefix = mask.leading_ones();
    let expected = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (mask == expected).then_some(prefix as u8)
}

#[cfg(not(target_os = "android"))]
fn interface_is_operational(flags: getifaddrs::InterfaceFlags) -> bool {
    flags.contains(getifaddrs::InterfaceFlags::UP)
        && flags.contains(getifaddrs::InterfaceFlags::RUNNING)
}

#[cfg(not(target_os = "android"))]
fn enumerate_raw_interfaces() -> Result<Vec<RawInterface>, String> {
    use getifaddrs::{getifaddrs, Address, InterfaceFlags};

    let interfaces = getifaddrs().map_err(|error| error.to_string())?;
    let mut raw = Vec::new();
    for interface in interfaces {
        let Address::V4(address) = interface.address else {
            continue;
        };
        let system_name = interface.name.clone();
        #[cfg(not(target_os = "windows"))]
        let name = system_name.clone();
        #[cfg(target_os = "windows")]
        let name = if interface.description.trim().is_empty() {
            system_name.clone()
        } else {
            interface.description
        };
        raw.push(RawInterface {
            name,
            system_name,
            index: interface.index,
            ipv4: address.address,
            prefix_length: address.netmask.and_then(prefix_length),
            is_up: interface_is_operational(interface.flags),
            is_loopback: interface.flags.contains(InterfaceFlags::LOOPBACK),
        });
    }
    Ok(raw)
}

#[cfg(target_os = "android")]
fn enumerate_raw_interfaces() -> Result<Vec<RawInterface>, String> {
    use std::net::UdpSocket;

    let mut addresses = BTreeSet::new();
    for target in [
        Ipv4Addr::new(224, 0, 0, 167),
        Ipv4Addr::new(172, 20, 10, 1),
        Ipv4Addr::new(192, 168, 43, 1),
        Ipv4Addr::new(192, 168, 137, 1),
        Ipv4Addr::new(10, 0, 0, 1),
    ] {
        let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
            continue;
        };
        if socket.connect((target, 8888)).is_err() {
            continue;
        }
        let Ok(local) = socket.local_addr() else {
            continue;
        };
        if let std::net::IpAddr::V4(ipv4) = local.ip() {
            if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                addresses.insert(ipv4);
            }
        }
    }

    Ok(addresses
        .into_iter()
        .map(|ipv4| RawInterface {
            name: "Android network".to_string(),
            system_name: "android-network".to_string(),
            index: Some(0),
            ipv4,
            prefix_length: None,
            is_up: true,
            is_loopback: false,
        })
        .collect())
}

fn snapshot_from_inventory(
    settings: DiscoverySettings,
    inventory: Result<Vec<RawInterface>, String>,
) -> DiscoveryNetworkSnapshot {
    match inventory {
        Ok(interfaces) => network_snapshot_from_raw(interfaces, settings),
        Err(error) => {
            eprintln!("[UDP][discovery.inventory] interface enumeration failed: {error}");
            DiscoveryNetworkSnapshot {
                settings,
                interfaces: Vec::new(),
            }
        }
    }
}

pub(crate) fn system_network_snapshot(settings: DiscoverySettings) -> DiscoveryNetworkSnapshot {
    snapshot_from_inventory(settings, enumerate_raw_interfaces())
}

fn settings_changed() -> &'static tokio::sync::Notify {
    DISCOVERY_SETTINGS_CHANGED.get_or_init(tokio::sync::Notify::new)
}

pub(crate) async fn wait_for_settings_change() {
    settings_changed().notified().await;
}

pub(crate) fn notify_settings_changed() {
    settings_changed().notify_waiters();
}

pub(crate) async fn load_settings(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<DiscoverySettings, String> {
    let Some(encoded) = crate::db::get_setting(pool, DISCOVERY_SETTINGS_KEY).await? else {
        return Ok(DiscoverySettings::default());
    };
    match serde_json::from_str::<DiscoverySettings>(&encoded) {
        Ok(mut settings) => {
            // A1 开发早期曾使用 `if:<index>:<system-name>`，但接口索引在重启和
            // 热插拔后可能变化。迁移到仅基于系统接口名的稳定 ID；无法归属的
            // `if:<index>` 仍丢弃，避免把旧选择误套到另一张网卡。
            settings.interface_overrides = settings
                .interface_overrides
                .into_iter()
                .filter_map(|(id, enabled)| migrate_interface_id(&id).map(|id| (id, enabled)))
                .collect();
            Ok(settings)
        }
        Err(error) => {
            eprintln!("[UDP][discovery.settings] invalid saved settings; using defaults: {error}");
            Ok(DiscoverySettings::default())
        }
    }
}

fn validate_settings(settings: &DiscoverySettings) -> Result<(), String> {
    if settings.interface_overrides.len() > 128 {
        return Err("discovery settings support at most 128 interface overrides".to_string());
    }
    for interface_id in settings.interface_overrides.keys() {
        if interface_id.trim().is_empty() {
            return Err("discovery interface id is required".to_string());
        }
        if interface_id.len() > 256 {
            return Err("discovery interface id must not exceed 256 bytes".to_string());
        }
    }
    Ok(())
}

pub(crate) async fn save_settings(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    settings: DiscoverySettings,
) -> Result<(), String> {
    validate_settings(&settings)?;
    let encoded = serde_json::to_string(&settings)
        .map_err(|error| format!("serialize discovery settings failed: {error}"))?;
    crate::db::set_setting(pool, DISCOVERY_SETTINGS_KEY, &encoded).await?;
    settings_changed().notify_waiters();
    Ok(())
}

pub(crate) async fn network_snapshot(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<DiscoveryNetworkSnapshot, String> {
    Ok(system_network_snapshot(load_settings(pool).await?))
}

pub(crate) fn build_send_plan(snapshot: &DiscoveryNetworkSnapshot, port: u16) -> DiscoverySendPlan {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();

    'interfaces: for interface in snapshot.interfaces.iter().filter(|item| item.enabled) {
        for address in &interface.addresses {
            let Ok(source_ip) = address.ipv4.parse::<Ipv4Addr>() else {
                continue;
            };
            if let Some(destination_ip) = address
                .prefix_length
                .and_then(|prefix| directed_broadcast(source_ip, prefix))
            {
                let destination = SocketAddrV4::new(destination_ip, port);
                if seen.insert((source_ip, destination)) {
                    targets.push(DiscoveryTarget {
                        interface_id: interface.id.clone(),
                        interface_index: interface.index,
                        source_ip,
                        destination,
                        kind: DiscoveryTargetKind::Broadcast,
                    });
                }
            }

            let destination = SocketAddrV4::new(DISCOVERY_MULTICAST, port);
            if seen.insert((source_ip, destination)) {
                targets.push(DiscoveryTarget {
                    interface_id: interface.id.clone(),
                    interface_index: interface.index,
                    source_ip,
                    destination,
                    kind: DiscoveryTargetKind::Multicast,
                });
            }

            if targets.len() >= MAX_INTERFACE_DATAGRAMS_PER_CYCLE {
                targets.truncate(MAX_INTERFACE_DATAGRAMS_PER_CYCLE);
                break 'interfaces;
            }
        }
    }

    DiscoverySendPlan {
        targets,
        budget: MAX_INTERFACE_DATAGRAMS_PER_CYCLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn raw(name: &str, index: u32, ipv4: [u8; 4], prefix_length: Option<u8>) -> RawInterface {
        RawInterface {
            name: name.to_string(),
            system_name: name.to_string(),
            index: Some(index),
            ipv4: Ipv4Addr::from(ipv4),
            prefix_length,
            is_up: true,
            is_loopback: false,
        }
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn interface_requires_both_up_and_running_flags() {
        use getifaddrs::InterfaceFlags;

        assert!(!interface_is_operational(InterfaceFlags::UP));
        assert!(!interface_is_operational(InterfaceFlags::RUNNING));
        assert!(interface_is_operational(
            InterfaceFlags::UP | InterfaceFlags::RUNNING
        ));
    }

    #[test]
    fn interface_classification_uses_safe_recommendations() {
        let fixtures = [
            ("en0", InterfaceCategory::PhysicalLan, true),
            ("Wi-Fi", InterfaceCategory::PhysicalLan, true),
            ("Ethernet", InterfaceCategory::PhysicalLan, true),
            ("WireGuard Tunnel", InterfaceCategory::MeshVpn, true),
            ("tailscale0", InterfaceCategory::MeshVpn, true),
            ("ZeroTier One", InterfaceCategory::MeshVpn, true),
            ("ztabcdef123", InterfaceCategory::MeshVpn, true),
            ("Meta Tunnel", InterfaceCategory::ProxyTun, false),
            ("Clash TUN", InterfaceCategory::ProxyTun, false),
            ("tun0", InterfaceCategory::ProxyTun, false),
            ("Docker Desktop", InterfaceCategory::VirtualMachine, false),
            ("vEthernet (WSL)", InterfaceCategory::VirtualMachine, false),
            ("Hyper-V Adapter", InterfaceCategory::VirtualMachine, false),
            ("以太网 2", InterfaceCategory::PhysicalLan, true),
            ("utun7", InterfaceCategory::ProxyTun, false),
            ("mystery0", InterfaceCategory::Unknown, false),
        ];

        for (name, expected_category, expected_default) in fixtures {
            let snapshot = network_snapshot_from_raw(
                vec![raw(name, 7, [10, 0, 0, 2], Some(24))],
                DiscoverySettings::default(),
            );
            assert_eq!(snapshot.interfaces[0].category, expected_category, "{name}");
            assert_eq!(
                snapshot.interfaces[0].default_enabled, expected_default,
                "{name}",
            );
        }
    }

    #[test]
    fn stable_interface_selection_survives_address_changes_and_master_switches() {
        let initial = network_snapshot_from_raw(
            vec![raw("en0", 7, [192, 168, 10, 20], Some(24))],
            DiscoverySettings::default(),
        );
        let stable_id = initial.interfaces[0].id.clone();
        assert_eq!(stable_id, "if:name:en0");

        let mut overrides = BTreeMap::new();
        overrides.insert(stable_id.clone(), true);
        let disabled = network_snapshot_from_raw(
            vec![raw("en0", 42, [192, 168, 20, 30], Some(24))],
            DiscoverySettings {
                local_discovery: false,
                vpn_discovery: true,
                interface_overrides: overrides.clone(),
            },
        );

        assert_eq!(disabled.interfaces[0].id, stable_id);
        assert!(disabled.interfaces[0].selected);
        assert!(!disabled.interfaces[0].enabled);
        assert_eq!(disabled.settings.interface_overrides, overrides);
    }

    #[test]
    fn explicit_override_can_enable_an_excluded_proxy_interface() {
        let interface = raw("Meta Tunnel", 12, [198, 18, 0, 1], Some(30));
        let default_snapshot =
            network_snapshot_from_raw(vec![interface.clone()], DiscoverySettings::default());
        assert!(!default_snapshot.interfaces[0].enabled);

        let id = default_snapshot.interfaces[0].id.clone();
        let snapshot = network_snapshot_from_raw(
            vec![interface],
            DiscoverySettings {
                interface_overrides: BTreeMap::from([(id, true)]),
                ..DiscoverySettings::default()
            },
        );
        assert!(snapshot.interfaces[0].selected);
        assert!(snapshot.interfaces[0].enabled);
    }

    #[test]
    fn send_plan_uses_directed_broadcast_and_interface_multicast_only() {
        let snapshot = network_snapshot_from_raw(
            vec![raw("en0", 7, [192, 168, 10, 178], Some(23))],
            DiscoverySettings::default(),
        );
        let plan = build_send_plan(&snapshot, 8888);

        assert_eq!(plan.targets.len(), 2);
        assert!(plan.targets.iter().any(|target| {
            target.kind == DiscoveryTargetKind::Broadcast
                && target.source_ip == Ipv4Addr::new(192, 168, 10, 178)
                && target.destination == SocketAddrV4::new(Ipv4Addr::new(192, 168, 11, 255), 8888)
        }));
        assert!(plan.targets.iter().any(|target| {
            target.kind == DiscoveryTargetKind::Multicast
                && target.destination == SocketAddrV4::new(Ipv4Addr::new(224, 0, 0, 167), 8888)
        }));
        assert!(plan.targets.iter().all(|target| {
            target.destination.ip() != &Ipv4Addr::BROADCAST
                && target.destination.ip() != &Ipv4Addr::new(192, 168, 0, 255)
        }));
    }

    #[test]
    fn unknown_or_host_prefixes_use_multicast_without_guessing_a_broadcast() {
        for prefix_length in [None, Some(31), Some(32), Some(33)] {
            let snapshot = network_snapshot_from_raw(
                vec![raw("en0", 7, [10, 0, 0, 2], prefix_length)],
                DiscoverySettings::default(),
            );
            let plan = build_send_plan(&snapshot, 8888);
            assert_eq!(plan.targets.len(), 1, "prefix {prefix_length:?}");
            assert_eq!(plan.targets[0].kind, DiscoveryTargetKind::Multicast);
        }
    }

    #[test]
    fn broad_route_prefix_never_generates_limited_broadcast() {
        let snapshot = network_snapshot_from_raw(
            vec![raw("Meta Tunnel", 12, [198, 18, 0, 1], Some(1))],
            DiscoverySettings {
                interface_overrides: BTreeMap::from([("if:name:meta tunnel".to_string(), true)]),
                ..DiscoverySettings::default()
            },
        );
        let plan = build_send_plan(&snapshot, 8888);

        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].kind, DiscoveryTargetKind::Multicast);
        assert_ne!(*plan.targets[0].destination.ip(), Ipv4Addr::BROADCAST);
    }

    #[test]
    fn inventory_failure_preserves_settings_and_returns_an_empty_snapshot() {
        let settings = DiscoverySettings {
            local_discovery: false,
            vpn_discovery: false,
            interface_overrides: BTreeMap::from([("if:name:en0".to_string(), false)]),
        };
        let snapshot = snapshot_from_inventory(settings.clone(), Err("enumeration failed".into()));

        assert_eq!(snapshot.settings, settings);
        assert!(snapshot.interfaces.is_empty());
    }

    #[test]
    fn send_plan_deduplicates_targets_and_enforces_its_budget() {
        let interfaces = (0..40)
            .map(|index| raw("en0", index, [10, index as u8, 0, 2], Some(24)))
            .collect();
        let snapshot = network_snapshot_from_raw(interfaces, DiscoverySettings::default());
        let plan = build_send_plan(&snapshot, 8888);

        assert_eq!(plan.budget, MAX_INTERFACE_DATAGRAMS_PER_CYCLE);
        assert_eq!(plan.targets.len(), MAX_INTERFACE_DATAGRAMS_PER_CYCLE);
        let unique = plan
            .targets
            .iter()
            .map(|target| (target.source_ip, target.destination))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), plan.targets.len());
        assert!(plan.targets.iter().all(|target| {
            target.destination.ip() != &Ipv4Addr::BROADCAST
                && !(target.destination.ip().octets()[0] == 192
                    && target.destination.ip().octets()[1] == 168
                    && target.destination.ip().octets()[3] == 255)
        }));
    }

    #[test]
    fn netmask_prefix_rejects_non_contiguous_masks() {
        assert_eq!(prefix_length(Ipv4Addr::new(255, 255, 254, 0)), Some(23));
        assert_eq!(prefix_length(Ipv4Addr::new(255, 255, 255, 255)), Some(32));
        assert_eq!(prefix_length(Ipv4Addr::UNSPECIFIED), Some(0));
        assert_eq!(prefix_length(Ipv4Addr::new(255, 0, 255, 0)), None);
    }

    async fn settings_pool() -> sqlx::Pool<sqlx::Sqlite> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn discovery_settings_round_trip_and_keep_forward_compatibility() {
        let pool = settings_pool().await;
        assert_eq!(
            load_settings(&pool).await.unwrap(),
            DiscoverySettings::default()
        );

        let settings = DiscoverySettings {
            local_discovery: false,
            vpn_discovery: true,
            interface_overrides: BTreeMap::from([
                ("if:name:en0".to_string(), false),
                ("if:name:meta tunnel".to_string(), true),
            ]),
        };
        save_settings(&pool, settings.clone()).await.unwrap();
        assert_eq!(load_settings(&pool).await.unwrap(), settings);

        crate::db::set_setting(
            &pool,
            DISCOVERY_SETTINGS_KEY,
            r#"{"local_discovery":false,"vpn_discovery":true,"interface_overrides":{"if:name:en0":true},"future_field":"ignored"}"#,
        )
        .await
        .unwrap();
        assert_eq!(
            load_settings(&pool).await.unwrap(),
            DiscoverySettings {
                local_discovery: false,
                vpn_discovery: true,
                interface_overrides: BTreeMap::from([("if:name:en0".to_string(), true)]),
            },
        );

        crate::db::set_setting(
            &pool,
            DISCOVERY_SETTINGS_KEY,
            r#"{"local_discovery":true,"vpn_discovery":true,"interface_overrides":{"if:7":false,"if:7:en0":true}}"#,
        )
        .await
        .unwrap();
        assert_eq!(
            load_settings(&pool).await.unwrap().interface_overrides,
            BTreeMap::from([("if:name:en0".to_string(), true)]),
        );
    }

    #[tokio::test]
    async fn corrupt_discovery_settings_fall_back_to_safe_defaults() {
        let pool = settings_pool().await;
        crate::db::set_setting(&pool, DISCOVERY_SETTINGS_KEY, "{not-json")
            .await
            .unwrap();

        assert_eq!(
            load_settings(&pool).await.unwrap(),
            DiscoverySettings::default()
        );
    }

    #[tokio::test]
    async fn discovery_settings_reject_unbounded_or_invalid_override_ids() {
        let pool = settings_pool().await;
        let too_many = (0..129)
            .map(|index| (format!("if:{index}"), true))
            .collect();
        let error = save_settings(
            &pool,
            DiscoverySettings {
                interface_overrides: too_many,
                ..DiscoverySettings::default()
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("128"));

        for invalid_id in ["", "   "] {
            let error = save_settings(
                &pool,
                DiscoverySettings {
                    interface_overrides: BTreeMap::from([(invalid_id.to_string(), true)]),
                    ..DiscoverySettings::default()
                },
            )
            .await
            .unwrap_err();
            assert!(error.contains("interface id"));
        }

        let error = save_settings(
            &pool,
            DiscoverySettings {
                interface_overrides: BTreeMap::from([("x".repeat(257), true)]),
                ..DiscoverySettings::default()
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("256"));
    }
}
