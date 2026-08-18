use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};

use crate::peers::PeerManager;

const MULTICAST_IP: &str = "224.0.0.167";
pub const DISCOVERY_PROTOCOL_VERSION: u16 = 2;
pub const DISCOVERY_CAPABILITIES: &[&str] = &[
    "group_chat",
    "receipts",
    "transfer_cancel",
    "parallel_file_v2",
];
static LOCAL_DEVICE_METADATA: OnceLock<(Option<String>, Option<String>)> = OnceLock::new();
/// 当前对外展示/使用的本机 IP。外层 `None` 表示尚未探测过，
/// 内层 `None` 表示探测过但没有可用地址（避免每次快照都重复全量探测）。
static LOCAL_IP_ADDRESS: RwLock<Option<Option<String>>> = RwLock::new(None);
static ALL_LOCAL_IPS: RwLock<Vec<String>> = RwLock::new(Vec::new());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryAnnouncement {
    pub peer_id: String,
    pub name: String,
    pub port: u16,
    pub available_memory_mb: u64,
    pub is_reply: bool,
    pub protocol_version: u16,
    pub hostname: Option<String>,
    pub mac_address: Option<String>,
    pub capabilities: Vec<String>,
    pub app_version: Option<String>,
}

impl DiscoveryAnnouncement {
    fn has_authoritative_metadata(&self) -> bool {
        !self.is_reply || !self.capabilities.is_empty()
    }

    pub fn parse(message: &str) -> Result<Option<Self>, String> {
        let parts: Vec<&str> = message.split('|').collect();
        if parts.len() < 6 || parts[0] != "LANChat" || parts[1] != "ONLINE" {
            return Ok(None);
        }

        let port = parts[4]
            .parse()
            .map_err(|_| "invalid discovery port".to_string())?;
        let available_memory_mb = parts[5]
            .parse()
            .map_err(|_| "invalid discovery memory".to_string())?;
        let protocol_version = parts
            .get(7)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| "invalid discovery protocol version".to_string())
            })
            .transpose()?
            .unwrap_or(1);

        Ok(Some(Self {
            peer_id: parts[2].to_string(),
            name: parts[3].to_string(),
            port,
            available_memory_mb,
            is_reply: parts.get(6).is_some_and(|value| *value == "1"),
            protocol_version,
            hostname: optional_part(parts.get(8)),
            mac_address: optional_part(parts.get(9)),
            capabilities: parts
                .get(10)
                .map(|value| {
                    value
                        .split(',')
                        .filter(|item| !item.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            app_version: optional_part(parts.get(11)),
        }))
    }

    pub fn encode(&self) -> String {
        format!(
            "LANChat|ONLINE|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.peer_id,
            self.name,
            self.port,
            self.available_memory_mb,
            u8::from(self.is_reply),
            self.protocol_version,
            self.hostname.as_deref().unwrap_or_default(),
            self.mac_address.as_deref().unwrap_or_default(),
            self.capabilities.join(","),
            self.app_version.as_deref().unwrap_or_default()
        )
    }
}

fn optional_part(value: Option<&&str>) -> Option<String> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| (*value).to_string())
}

pub(crate) fn local_device_metadata() -> (Option<String>, Option<String>) {
    LOCAL_DEVICE_METADATA
        .get_or_init(|| {
            let hostname = sysinfo::System::host_name();
            let networks = sysinfo::Networks::new_with_refreshed_list();
            let mac_address = networks.iter().find_map(|(_, network)| {
                let address = network.mac_address();
                (!address.is_unspecified()).then(|| address.to_string())
            });
            (hostname, mac_address)
        })
        .clone()
}

/// 用一个目标地址反查内核为该路由选择的源 IP。
/// UDP connect 不会发包，只做路由表查询，所以可以安全地大量探测。
fn probe_local_ip(target: Ipv4Addr) -> Option<String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((target, 8888)).ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_unspecified() && !address.is_loopback()).then(|| address.to_string())
}

/// 探测目标集合：覆盖组播、公网默认路由与常见私有网段。
/// 每个存在对应网卡的网段都会返回该网卡的源 IP，没有路由的会直接失败。
fn ip_probe_targets() -> Vec<Ipv4Addr> {
    let mut targets = Vec::with_capacity(280);
    targets.push(Ipv4Addr::new(224, 0, 0, 167));
    targets.push(Ipv4Addr::new(8, 8, 8, 8));
    for third in 0..=255u8 {
        targets.push(Ipv4Addr::new(192, 168, third, 1));
    }
    for second in 16..=31u8 {
        targets.push(Ipv4Addr::new(172, second, 0, 1));
    }
    targets.push(Ipv4Addr::new(10, 0, 0, 1));
    targets
}

/// 枚举所有可用的本机 IPv4 地址（不含回环与未指定地址）
pub(crate) fn get_all_local_ips() -> Vec<String> {
    let mut ips: Vec<String> = Vec::new();
    for target in ip_probe_targets() {
        if let Some(ip) = probe_local_ip(target) {
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    ips
}

/// 默认地址取组播路由的出口（UDP 发现实际使用的那张卡），列表为空时返回 None
fn default_local_ip(all_ips: &[String]) -> Option<String> {
    probe_local_ip(Ipv4Addr::new(224, 0, 0, 167))
        .filter(|ip| all_ips.contains(ip))
        .or_else(|| all_ips.first().cloned())
}

/// 后台重扫时的取舍：手动选的地址还在就保留，没了才回落到默认
pub(crate) fn keep_or_reselect(current: Option<&str>, all_ips: &[String]) -> Option<String> {
    match current {
        Some(ip) if all_ips.iter().any(|candidate| candidate == ip) => Some(ip.to_string()),
        _ => default_local_ip(all_ips),
    }
}

/// 重新探测本机 IP 列表，并把当前 IP 重置为默认出口（设置页手动刷新用）
pub(crate) fn refresh_local_ips() -> Vec<String> {
    let all_ips = get_all_local_ips();
    let preferred = default_local_ip(&all_ips);

    *ALL_LOCAL_IPS.write().unwrap() = all_ips.clone();
    *LOCAL_IP_ADDRESS.write().unwrap() = Some(preferred.clone());

    println!("[IP] 刷新本机 IP: 当前 {:?}，可用 {:?}", preferred, all_ips);
    all_ips
}

/// 心跳循环里的周期性重扫：更新可用列表，但不覆盖用户手动选的地址
pub(crate) fn rescan_local_ips() -> Vec<String> {
    let all_ips = get_all_local_ips();
    let current = LOCAL_IP_ADDRESS.read().unwrap().clone().flatten();
    let preferred = keep_or_reselect(current.as_deref(), &all_ips);

    *ALL_LOCAL_IPS.write().unwrap() = all_ips.clone();
    *LOCAL_IP_ADDRESS.write().unwrap() = Some(preferred);

    all_ips
}

pub(crate) fn local_ip_address() -> Option<String> {
    if let Some(cached) = LOCAL_IP_ADDRESS.read().unwrap().clone() {
        return cached;
    }
    refresh_local_ips();
    LOCAL_IP_ADDRESS.read().unwrap().clone().flatten()
}

/// 从已探测到的列表中手动切换当前 IP
#[cfg(feature = "desktop")]
pub(crate) fn set_local_ip(ip: String) -> bool {
    if !ALL_LOCAL_IPS.read().unwrap().contains(&ip) {
        eprintln!("[IP] 目标地址不在可用列表中: {}", ip);
        return false;
    }
    *LOCAL_IP_ADDRESS.write().unwrap() = Some(Some(ip.clone()));
    println!("[IP] 手动切换本机 IP 为: {}", ip);
    true
}

/// 读取缓存的 IP 列表；若尚未探测过则先探测一次
pub(crate) fn get_all_cached_ips() -> Vec<String> {
    {
        let cached = ALL_LOCAL_IPS.read().unwrap();
        if !cached.is_empty() {
            return cached.clone();
        }
    }
    refresh_local_ips()
}

#[allow(clippy::too_many_arguments)]
fn local_announcement(
    peer_id: String,
    name: String,
    port: u16,
    available_memory_mb: u64,
    is_reply: bool,
    hostname: Option<String>,
    mac_address: Option<String>,
    app_version: Option<String>,
) -> DiscoveryAnnouncement {
    DiscoveryAnnouncement {
        peer_id,
        name,
        port,
        available_memory_mb,
        is_reply,
        protocol_version: DISCOVERY_PROTOCOL_VERSION,
        hostname,
        mac_address,
        capabilities: DISCOVERY_CAPABILITIES
            .iter()
            .map(|capability| (*capability).to_string())
            .collect(),
        app_version,
    }
}

// 创建支持广播和组播的 UDP socket
fn create_discovery_socket(
    bind_addr: &str,
    is_listener: bool,
) -> Result<UdpSocket, std::io::Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

    #[cfg(target_os = "windows")]
    socket.set_reuse_address(true)?;

    #[cfg(not(target_os = "windows"))]
    {
        socket.set_reuse_address(true)?;
        socket.set_reuse_port(true)?;
    }

    let addr: std::net::SocketAddr = bind_addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    socket.bind(&addr.into())?;

    let std_socket: UdpSocket = socket.into();

    if is_listener {
        let multi_addr: Ipv4Addr = MULTICAST_IP.parse().unwrap();
        // 先按默认接口加入，保证单网卡与 Android（读不到网卡列表）场景不退化
        let _ = std_socket.join_multicast_v4(&multi_addr, &Ipv4Addr::UNSPECIFIED);
        // 再对每张探测到的网卡各加入一次，多网卡才不会只收到默认卡的组播
        for ip in get_all_cached_ips() {
            if let Ok(interface) = ip.parse::<Ipv4Addr>() {
                let _ = std_socket.join_multicast_v4(&multi_addr, &interface);
            }
        }
    } else {
        std_socket.set_broadcast(true)?;
        let _ = std_socket.set_multicast_ttl_v4(1);
    }

    Ok(std_socket)
}

/// 把发送 socket 绑到某张网卡的地址上。
/// 绑定源地址会强制包从这张卡出去，因此不需要知道子网掩码：
/// 受限广播 255.255.255.255 与组播都是链路本地的，不经过路由器。
fn bind_interface_socket(local_ip: Ipv4Addr) -> Result<UdpSocket, std::io::Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    let addr = std::net::SocketAddr::from((local_ip, 0));
    socket.bind(&addr.into())?;
    socket.set_broadcast(true)?;
    let _ = socket.set_multicast_ttl_v4(1);
    // IP_MULTICAST_IF：仅靠绑定源地址在部分平台不足以决定组播出口，必须显式指定
    let _ = socket.set_multicast_if_v4(&local_ip);
    Ok(socket.into())
}

/// 为每个探测到的本机地址建一个发送 socket；绑不上的地址（已失效）直接跳过
fn interface_sockets(local_ips: &[String]) -> Vec<(Ipv4Addr, UdpSocket)> {
    local_ips
        .iter()
        .filter_map(|ip| ip.parse::<Ipv4Addr>().ok())
        .filter_map(|ip| bind_interface_socket(ip).ok().map(|socket| (ip, socket)))
        .collect()
}

/// 每张网卡都要发的目标：受限广播 + 组播。绑定源地址后二者都只走本网卡。
fn per_interface_targets(port: u16) -> Vec<std::net::SocketAddr> {
    vec![
        std::net::SocketAddr::from((Ipv4Addr::BROADCAST, port)),
        std::net::SocketAddr::from((MULTICAST_IP.parse::<Ipv4Addr>().unwrap(), port)),
    ]
}

// 核心黑科技：生成全网段广播地址（绕过 Android 网卡读取限制）
fn get_smart_broadcast_addresses(port: u16) -> Vec<String> {
    let mut addrs = Vec::with_capacity(260);

    // 1. 全局受限广播 (应对普通路由器)
    addrs.push(format!("255.255.255.255:{}", port));
    // 2. 组播 (PC端互联完美生效)
    addrs.push(format!("{}:{}", MULTICAST_IP, port));
    // 3. 苹果 iOS 热点固定广播地址
    addrs.push(format!("172.20.10.15:{}", port));
    // 4. 常见企业路由器网段
    addrs.push(format!("10.0.0.255:{}", port));

    // 5. Android 随机热点网段 "暴力"覆盖 (192.168.0.255 ~ 192.168.255.255)
    // 那些没有对应网卡的地址会在微秒级被内核路由表直接丢弃，不会产生网络风暴
    for i in 0..=255 {
        addrs.push(format!("192.168.{}.255:{}", i, port));
    }

    addrs
}

/// 每隔多少个心跳周期重扫一次网卡（2 秒一拍，约 30 秒）
const IP_RESCAN_TICKS: u32 = 15;

pub async fn start_announcing(port: u16, user_id: String, pool: sqlx::Pool<sqlx::Sqlite>) {
    let socket = match create_discovery_socket("0.0.0.0:0", false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[UDP] 创建发送 socket 失败: {}", e);
            return;
        }
    };

    println!("[UDP] 开始通过智能路由遍历发送心跳...");

    use std::collections::HashMap;
    use std::time::Instant;
    use sysinfo::System;
    let mut sys = System::new();
    let target_addrs = get_smart_broadcast_addresses(port);
    let (hostname, mac_address) = local_device_metadata();

    // 逐网卡发送：绑定源地址强制包从每张卡出去，多网卡/多网段才不会漏
    let interface_targets = per_interface_targets(port);
    let mut known_ips = get_all_cached_ips();
    let mut nic_sockets = interface_sockets(&known_ips);
    println!("[UDP] 逐网卡心跳已就绪: {:?}", known_ips);
    let mut tick: u32 = 0;

    // DNS 缓存：hostname → (解析结果, 过期时间)，TTL 60 秒
    let mut dns_cache: HashMap<String, (Vec<std::net::SocketAddr>, Instant)> = HashMap::new();
    const DNS_TTL: Duration = Duration::from_secs(60);

    loop {
        let username = match crate::db::get_username(&pool).await {
            Ok(name) => name,
            Err(_) => "Unknown".to_string(),
        };

        sys.refresh_memory();
        let available_memory_mb = sys.available_memory() / (1024 * 1024);

        let msg = local_announcement(
            user_id.clone(),
            username,
            port,
            available_memory_mb,
            false,
            hostname.clone(),
            mac_address.clone(),
            Some(env!("CARGO_PKG_VERSION").to_string()),
        )
        .encode();

        // 逐网卡发一遍受限广播与组播：不依赖子网掩码，多网卡都能覆盖
        for (_ip, nic_socket) in &nic_sockets {
            for target in &interface_targets {
                let _ = nic_socket.send_to(msg.as_bytes(), target);
            }
        }

        // 兜底：遍历所有可能地址，仅路由存在的网卡能发送成功
        // （Android 读不到网卡列表时，这条路径仍然有效）
        for addr in &target_addrs {
            let _ = socket.send_to(msg.as_bytes(), addr);
        }

        // 发送单播到自定义设备（支持 IP 和 域名/主机名）
        let custom_peers = crate::db::get_custom_peers(&pool).await;
        let now = Instant::now();
        for peer in &custom_peers {
            if let Ok(addr) = peer.parse::<std::net::SocketAddr>() {
                // 快速路径：纯 IP:port
                let _ = socket.send_to(msg.as_bytes(), addr);
            } else {
                // DNS 路径：域名/主机名
                let with_port = if peer.contains(':') {
                    peer.clone()
                } else {
                    format!("{}:{}", peer, port)
                };

                // 查缓存
                let addrs = if let Some((cached, expiry)) = dns_cache.get(&with_port) {
                    if *expiry > now {
                        Some(cached.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let addrs = match addrs {
                    Some(a) => a,
                    None => {
                        // DNS 解析
                        match tokio::net::lookup_host(&with_port).await {
                            Ok(resolved) => {
                                let list: Vec<_> = resolved.collect();
                                if !list.is_empty() {
                                    dns_cache
                                        .insert(with_port.clone(), (list.clone(), now + DNS_TTL));
                                }
                                list
                            }
                            Err(e) => {
                                eprintln!("[UDP] DNS 解析失败 ({}): {}", peer, e);
                                Vec::new()
                            }
                        }
                    }
                };

                for addr in &addrs {
                    let _ = socket.send_to(msg.as_bytes(), addr);
                }
            }
        }

        // 周期性重扫网卡：插拔网线、连断 VPN 后自动跟上，不覆盖手动选择
        tick = tick.wrapping_add(1);
        if tick % IP_RESCAN_TICKS == 0 {
            let latest = rescan_local_ips();
            if latest != known_ips {
                println!("[UDP] 网卡变化 {:?} -> {:?}，重建逐网卡心跳", known_ips, latest);
                nic_sockets = interface_sockets(&latest);
                known_ips = latest;
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// 桌面端版本 - 带 AppHandle
#[cfg(all(feature = "desktop", not(feature = "web")))]
pub async fn start_listening(
    port: u16,
    my_id: String,
    my_name: String,
    app: Option<AppHandle>,
    peer_manager: Arc<PeerManager>,
    pool: sqlx::Pool<sqlx::Sqlite>,
) {
    let bind_addr = format!("0.0.0.0:{}", port);
    let socket = match create_discovery_socket(&bind_addr, true) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[UDP] 创建监听 socket 失败: {}", e);
            return;
        }
    };

    let mut buf = [0u8; 1024];
    let (hostname, mac_address) = local_device_metadata();
    println!("[UDP] 正在端口 {} 监听邻居...", port);

    loop {
        if let Ok((size, addr)) = socket.recv_from(&mut buf) {
            let msg = String::from_utf8_lossy(&buf[..size]);
            let parts: Vec<&str> = msg.split('|').collect();

            if parts.len() >= 6 && parts[0] == "LANChat" {
                let announcement = match DiscoveryAnnouncement::parse(&msg) {
                    Ok(Some(announcement)) => announcement,
                    _ => continue,
                };
                let peer_id = parts[2].to_string();
                let name = parts[3].to_string();
                let peer_port = parts[4];
                let available_memory_mb: u64 = parts[5].parse().unwrap_or(0);
                if peer_id == my_id {
                    continue;
                }

                let peer_addr = format!("{}:{}", addr.ip(), peer_port);

                let is_new_or_reconnected = peer_manager.add_or_update_with_details(
                    peer_id.clone(),
                    name.clone(),
                    peer_addr.clone(),
                    available_memory_mb,
                    announcement.hostname.clone(),
                    announcement.mac_address.clone(),
                    Some("lan".to_string()),
                    announcement.capabilities.clone(),
                    announcement.has_authoritative_metadata(),
                );

                // 保存或更新用户到数据库
                let _ = crate::db::save_or_update_discovered_user(
                    &pool,
                    &peer_id,
                    &name,
                    &peer_addr,
                    available_memory_mb,
                    announcement.hostname.as_deref(),
                    announcement.mac_address.as_deref(),
                    Some("lan"),
                    announcement.has_authoritative_metadata(),
                )
                .await;

                // 只在新用户或重新上线时打印日志
                if is_new_or_reconnected {
                    println!(
                        "[UDP] 发现用户: {} ({}) at {} (可用内存: {} MB)，准备检查补发队列...",
                        name, peer_id, peer_addr, available_memory_mb
                    );
                    if let Some(app_handle) = &app {
                        let _ = app_handle.emit(
                            "peer-online",
                            serde_json::json!({
                                "id": peer_id,
                                "name": name,
                                "addr": peer_addr,
                            }),
                        );
                    }

                    let pool_clone = pool.clone();
                    let peer_id_clone = peer_id.clone();
                    let peer_addr_clone = peer_addr.clone();
                    let app_clone = app.clone();
                    let peer_manager_clone = peer_manager.clone();

                    // 扔进后台线程执行，不要阻挡 UDP 监听其他用户的广播！
                    tokio::spawn(async move {
                        if let Err(e) = crate::network::messaging::resend_pending_messages(
                            &pool_clone,
                            &peer_id_clone,
                            &peer_addr_clone,
                            app_clone,
                        )
                        .await
                        {
                            eprintln!("[UDP] 补发消息严重失败: {}", e);
                        }
                        if let Err(e) = crate::workspace::resend_for_peer(
                            &pool_clone,
                            &peer_manager_clone,
                            &peer_id_clone,
                            &peer_addr_clone,
                        )
                        .await
                        {
                            eprintln!("[UDP] 新协议补发失败: {}", e);
                        }
                    });
                }

                if let Some(app_handle) = &app {
                    let _ = app_handle.emit(
                        "new-peer",
                        serde_json::json!({
                            "id": peer_id, "name": name, "addr": peer_addr,
                            "available_memory_mb": available_memory_mb,
                            "hostname": announcement.hostname,
                            "mac_address": announcement.mac_address,
                            "discovery_source": "lan",
                            "capabilities": announcement.capabilities,
                            "protocol_version": announcement.protocol_version
                        }),
                    );
                }

                if !announcement.is_reply {
                    // 从数据库读取最新用户名（用户改名后动态生效）
                    let reply_name = match crate::db::get_username(&pool).await {
                        Ok(name) => name,
                        Err(_) => my_name.clone(),
                    };
                    let reply = local_announcement(
                        my_id.clone(),
                        reply_name,
                        port,
                        0,
                        true,
                        hostname.clone(),
                        mac_address.clone(),
                        Some(env!("CARGO_PKG_VERSION").to_string()),
                    )
                    .encode();
                    let target = format!("{}:{}", addr.ip(), peer_port);
                    let _ = socket.send_to(reply.as_bytes(), &target);
                }
            }
        }
    }
}

// Web 端版本 - 不带 AppHandle
#[cfg(all(feature = "web", not(feature = "desktop")))]
pub async fn start_listening(
    port: u16,
    my_id: String,
    my_name: String,
    peer_manager: Arc<PeerManager>,
    pool: sqlx::Pool<sqlx::Sqlite>,
) {
    let bind_addr = format!("0.0.0.0:{}", port);
    let socket = match create_discovery_socket(&bind_addr, true) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[UDP] Web端创建监听 socket 失败: {}", e);
            return;
        }
    };

    let mut buf = [0u8; 1024];
    let (hostname, mac_address) = local_device_metadata();
    loop {
        if let Ok((size, addr)) = socket.recv_from(&mut buf) {
            let msg = String::from_utf8_lossy(&buf[..size]);
            let parts: Vec<&str> = msg.split('|').collect();

            if parts.len() >= 6 && parts[0] == "LANChat" {
                let announcement = match DiscoveryAnnouncement::parse(&msg) {
                    Ok(Some(announcement)) => announcement,
                    _ => continue,
                };
                let peer_id = parts[2].to_string();
                let name = parts[3].to_string();
                let peer_port = parts[4];
                let available_memory_mb: u64 = parts[5].parse().unwrap_or(0);
                if peer_id == my_id {
                    continue;
                }
                let peer_addr = format!("{}:{}", addr.ip(), peer_port);

                let is_new_or_reconnected = peer_manager.add_or_update_with_details(
                    peer_id.clone(),
                    name.clone(),
                    peer_addr.clone(),
                    available_memory_mb,
                    announcement.hostname.clone(),
                    announcement.mac_address.clone(),
                    Some("lan".to_string()),
                    announcement.capabilities.clone(),
                    announcement.has_authoritative_metadata(),
                );

                // 保存或更新用户到数据库
                let _ = crate::db::save_or_update_discovered_user(
                    &pool,
                    &peer_id,
                    &name,
                    &peer_addr,
                    available_memory_mb,
                    announcement.hostname.as_deref(),
                    announcement.mac_address.as_deref(),
                    Some("lan"),
                    announcement.has_authoritative_metadata(),
                )
                .await;

                // 用户重新上线，补发挂起的消息
                if is_new_or_reconnected {
                    println!("[UDP] 发现用户或重新上线，准备检查补发队列...");
                    let pool_clone = pool.clone();
                    let peer_id_clone = peer_id.clone();
                    let peer_addr_clone = peer_addr.clone();
                    let peer_manager_clone = peer_manager.clone();

                    // 扔进后台线程执行，不要阻挡 UDP 监听其他用户的广播！
                    tokio::spawn(async move {
                        if let Err(e) = crate::network::messaging::resend_pending_messages(
                            &pool_clone,
                            &peer_id_clone,
                            &peer_addr_clone,
                            None,
                        )
                        .await
                        {
                            eprintln!("[UDP] 补发消息严重失败: {}", e);
                        }
                        if let Err(e) = crate::workspace::resend_for_peer(
                            &pool_clone,
                            &peer_manager_clone,
                            &peer_id_clone,
                            &peer_addr_clone,
                        )
                        .await
                        {
                            eprintln!("[UDP] 新协议补发失败: {}", e);
                        }
                    });
                }

                if !announcement.is_reply {
                    // 从数据库读取最新用户名（用户改名后动态生效）
                    let reply_name = match crate::db::get_username(&pool).await {
                        Ok(name) => name,
                        Err(_) => my_name.clone(),
                    };
                    let reply = local_announcement(
                        my_id.clone(),
                        reply_name,
                        port,
                        0,
                        true,
                        hostname.clone(),
                        mac_address.clone(),
                        Some(env!("CARGO_PKG_VERSION").to_string()),
                    )
                    .encode();
                    let target = format!("{}:{}", addr.ip(), peer_port);
                    let _ = socket.send_to(reply.as_bytes(), &target);
                }
            }
        }
    }
}

/// 离线看门狗的扫描间隔。原先离线判定只在前端拉快照时顺带发生，
/// 没人拉就永远不翻转，于是既没有离线通知、也不会触发上线补发。
/// 必须明显小于 PEER_OFFLINE_TIMEOUT_SECS(10s)，否则实际感知延迟是
/// 超时 + 扫描间隔。2 秒一扫，最坏 12 秒内出提示。
const OFFLINE_SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// 主动扫描超时未见的用户并广播离线事件。
/// 必须独立于前端轮询运行：`is_offline` 是补发链路的触发条件，
/// 不能依赖界面是否恰好在拉数据。
/// 每隔多少次扫描顺带清一次在线用户的待发队列（2 秒一拍，约 30 秒）。
/// 补发本来只靠「离线→上线」跳变触发，跳变一旦错过消息就永久卡住；
/// 这条兜底保证队列最终一定会被清掉。
const RESEND_SWEEP_TICKS: u32 = 15;

/// 扫描一轮：把超时未见的用户标离线，并周期性重试在线用户的待发队列
async fn offline_scan_tick(
    peer_manager: &Arc<PeerManager>,
    pool: &sqlx::Pool<sqlx::Sqlite>,
    tick: u32,
) -> Vec<crate::peers::Peer> {
    let newly_offline = peer_manager.mark_stale_as_offline();

    // 落库，否则重启后这人又会显示在线（内存表是从 users 表重建的）
    for peer in &newly_offline {
        if let Err(error) = crate::db::mark_user_offline(pool, &peer.id).await {
            eprintln!("[UDP] 持久化离线状态失败 {}: {error}", peer.id);
        }
    }

    if tick % RESEND_SWEEP_TICKS == 0 {
        for peer in peer_manager.get_active_peers() {
            if let Err(error) =
                crate::workspace::resend_for_peer(pool, peer_manager, &peer.id, &peer.addr).await
            {
                eprintln!("[UDP] 队列兜底补发失败 {}: {error}", peer.id);
            }
        }
    }

    newly_offline
}

#[cfg(feature = "desktop")]
pub async fn start_offline_watchdog(
    peer_manager: Arc<PeerManager>,
    pool: sqlx::Pool<sqlx::Sqlite>,
    app: Option<AppHandle>,
) {
    println!(
        "[UDP] 离线看门狗已启动，每 {:?} 扫描一次",
        OFFLINE_SCAN_INTERVAL
    );
    let mut tick: u32 = 0;
    loop {
        tokio::time::sleep(OFFLINE_SCAN_INTERVAL).await;
        tick = tick.wrapping_add(1);

        for peer in offline_scan_tick(&peer_manager, &pool, tick).await {
            if let Some(app_handle) = &app {
                let _ = app_handle.emit(
                    "peer-offline",
                    serde_json::json!({
                        "id": peer.id,
                        "name": peer.name,
                        "addr": peer.addr,
                    }),
                );
            }
        }
    }
}

// Web 端版本 - 没有 AppHandle 可以 emit，只做状态翻转与队列兜底
#[cfg(all(feature = "web", not(feature = "desktop")))]
pub async fn start_offline_watchdog(
    peer_manager: Arc<PeerManager>,
    pool: sqlx::Pool<sqlx::Sqlite>,
) {
    println!(
        "[UDP] 离线看门狗已启动，每 {:?} 扫描一次",
        OFFLINE_SCAN_INTERVAL
    );
    let mut tick: u32 = 0;
    loop {
        tokio::time::sleep(OFFLINE_SCAN_INTERVAL).await;
        tick = tick.wrapping_add(1);
        let _ = offline_scan_tick(&peer_manager, &pool, tick).await;
    }
}

// 发送单次广播
pub async fn send_single_broadcast(
    port: u16,
    user_id: String,
    username: String,
) -> Result<(), String> {
    let socket = create_discovery_socket("0.0.0.0:0", false)
        .map_err(|e| format!("创建发送socket失败: {}", e))?;

    let (hostname, mac_address) = local_device_metadata();
    let msg = local_announcement(
        user_id,
        username,
        port,
        0,
        false,
        hostname,
        mac_address,
        Some(env!("CARGO_PKG_VERSION").to_string()),
    )
    .encode();
    let target_addrs = get_smart_broadcast_addresses(port);

    for addr in target_addrs {
        let _ = socket.send_to(msg.as_bytes(), &addr);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_is_noticed_within_a_couple_seconds_of_the_timeout() {
        // 实际感知延迟 = 离线超时 + 扫描间隔。扫描间隔一旦接近甚至超过超时，
        // 「10 秒判离线」在用户看来就变成 15 秒以上。
        let timeout = crate::peers::PEER_OFFLINE_TIMEOUT_SECS;
        let scan = OFFLINE_SCAN_INTERVAL.as_secs();
        assert!(
            scan * 4 <= timeout,
            "扫描间隔 {scan}s 相对离线超时 {timeout}s 太粗，感知会明显滞后"
        );

        // 兜底补发是网络开销，不能因为扫描变快就跟着变频繁；保持 30 秒左右
        let sweep = scan * u64::from(RESEND_SWEEP_TICKS);
        assert!(
            (20..=40).contains(&sweep),
            "队列兜底补发间隔 {sweep}s 偏离预期的 30 秒"
        );
    }

    #[test]
    fn discovery_parser_accepts_legacy_frames() {
        let announce = DiscoveryAnnouncement::parse("LANChat|ONLINE|peer-1|Alice|8888|512")
            .unwrap()
            .unwrap();
        assert!(!announce.is_reply);
        assert_eq!(announce.protocol_version, 1);
        assert!(announce.capabilities.is_empty());

        let reply = DiscoveryAnnouncement::parse("LANChat|ONLINE|peer-1|Alice|8888|0|1")
            .unwrap()
            .unwrap();
        assert!(reply.is_reply);
    }

    #[test]
    fn discovery_extension_round_trips_after_legacy_prefix() {
        assert!(DISCOVERY_CAPABILITIES.contains(&"parallel_file_v2"));
        let announcement = DiscoveryAnnouncement {
            peer_id: "peer-1".into(),
            name: "Alice".into(),
            port: 8888,
            available_memory_mb: 512,
            is_reply: false,
            protocol_version: DISCOVERY_PROTOCOL_VERSION,
            hostname: Some("alice-mac".into()),
            mac_address: Some("01:02:03:04:05:06".into()),
            capabilities: vec!["group_chat".into(), "receipts".into()],
            app_version: Some("0.1.5".into()),
        };

        let encoded = announcement.encode();
        assert!(encoded.starts_with("LANChat|ONLINE|peer-1|Alice|8888|512|0|2|"));
        assert_eq!(
            DiscoveryAnnouncement::parse(&encoded).unwrap(),
            Some(announcement)
        );
    }

    #[test]
    fn app_version_round_trips_and_defaults_to_none() {
        let with_version = DiscoveryAnnouncement::parse(
            "LANChat|ONLINE|peer-1|Alice|8888|512|0|2|alice-mac|01:02:03:04:05:06|group_chat|0.1.5",
        )
        .unwrap()
        .unwrap();
        assert_eq!(with_version.app_version.as_deref(), Some("0.1.5"));
        assert_eq!(with_version.encode().split('|').count(), 12);

        let legacy = DiscoveryAnnouncement::parse(
            "LANChat|ONLINE|peer-1|Alice|8888|512|0|2|alice-mac|01:02:03:04:05:06|group_chat",
        )
        .unwrap()
        .unwrap();
        assert_eq!(legacy.app_version, None);

        // 12 段帧但末尾 app_version 为空字符串 → 应回落为 None
        let empty_version = DiscoveryAnnouncement::parse(
            "LANChat|ONLINE|peer-1|Alice|8888|512|0|2|alice-mac|01:02:03:04:05:06|group_chat|",
        )
        .unwrap()
        .unwrap();
        assert_eq!(empty_version.app_version, None);
    }

    #[test]
    fn capable_reply_is_authoritative_but_legacy_reply_is_not() {
        let capable = DiscoveryAnnouncement::parse(
            "LANChat|ONLINE|peer-1|Alice|8888|512|1|2|||group_chat,receipts",
        )
        .unwrap()
        .unwrap();
        let legacy = DiscoveryAnnouncement::parse("LANChat|ONLINE|peer-2|Bob|8888|512|1")
            .unwrap()
            .unwrap();

        assert!(capable.has_authoritative_metadata());
        assert!(!legacy.has_authoritative_metadata());
    }

    #[test]
    fn manual_ip_choice_survives_a_background_rescan() {
        let available = vec!["192.168.10.178".to_string(), "198.18.0.1".to_string()];
        assert_eq!(
            keep_or_reselect(Some("192.168.10.178"), &available).as_deref(),
            Some("192.168.10.178"),
        );
    }

    #[test]
    fn vanished_ip_falls_back_instead_of_sticking_to_a_dead_address() {
        // 拔掉网线/断开 VPN 后原地址消失，必须回落而不是继续显示失效地址
        let available = vec!["192.168.10.178".to_string()];
        let chosen = keep_or_reselect(Some("192.168.20.139"), &available);
        assert_ne!(chosen.as_deref(), Some("192.168.20.139"));
        assert_eq!(chosen.as_deref(), Some("192.168.10.178"));
    }

    #[test]
    fn empty_ip_list_selects_nothing_rather_than_panicking() {
        assert_eq!(keep_or_reselect(Some("192.168.10.178"), &[]), None);
        assert_eq!(keep_or_reselect(None, &[]), None);
    }

    #[test]
    fn interface_sockets_skip_addresses_that_are_not_local() {
        // 203.0.113.7 是 RFC 5737 文档网段，本机不可能持有 → 必须被跳过
        let sockets = interface_sockets(&[
            "127.0.0.1".to_string(),
            "203.0.113.7".to_string(),
            "not-an-ip".to_string(),
        ]);
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].0, Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn per_interface_targets_cover_limited_broadcast_and_multicast() {
        // 绑定源地址后这两个目标都只走本网卡，因此无需知道子网掩码
        let targets = per_interface_targets(8888);
        let ips: Vec<_> = targets.iter().map(|target| target.ip().to_string()).collect();
        assert!(ips.contains(&"255.255.255.255".to_string()));
        assert!(ips.contains(&MULTICAST_IP.to_string()));
        assert!(targets.iter().all(|target| target.port() == 8888));
    }

    #[test]
    fn brute_force_fallback_still_covers_android_hotspot_ranges() {
        // 逐网卡发送是新增路径，不能顺手削弱 Android 的兜底覆盖
        let addrs = get_smart_broadcast_addresses(8888);
        assert!(addrs.contains(&"255.255.255.255:8888".to_string()));
        assert!(addrs.contains(&"172.20.10.15:8888".to_string()));
        assert!(addrs.contains(&"192.168.0.255:8888".to_string()));
        assert!(addrs.contains(&"192.168.255.255:8888".to_string()));
    }

    #[test]
    fn local_ip_address_never_returns_loopback_or_unspecified() {
        if let Some(address) = local_ip_address() {
            let address: std::net::IpAddr = address.parse().unwrap();
            assert!(!address.is_loopback());
            assert!(!address.is_unspecified());
        }
    }
}
