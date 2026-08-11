use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::{Arc, OnceLock};
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
static LOCAL_IP_ADDRESS: OnceLock<Option<String>> = OnceLock::new();

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
        }))
    }

    pub fn encode(&self) -> String {
        format!(
            "LANChat|ONLINE|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.peer_id,
            self.name,
            self.port,
            self.available_memory_mb,
            u8::from(self.is_reply),
            self.protocol_version,
            self.hostname.as_deref().unwrap_or_default(),
            self.mac_address.as_deref().unwrap_or_default(),
            self.capabilities.join(",")
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

pub(crate) fn local_ip_address() -> Option<String> {
    LOCAL_IP_ADDRESS
        .get_or_init(|| {
            let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
            socket
                .connect((Ipv4Addr::new(224, 0, 0, 167), 8888))
                .ok()?;
            let address = socket.local_addr().ok()?.ip();
            (!address.is_unspecified() && !address.is_loopback()).then(|| address.to_string())
        })
        .clone()
}

fn local_announcement(
    peer_id: String,
    name: String,
    port: u16,
    available_memory_mb: u64,
    is_reply: bool,
    hostname: Option<String>,
    mac_address: Option<String>,
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
        let interface: Ipv4Addr = "0.0.0.0".parse().unwrap();
        let _ = std_socket.join_multicast_v4(&multi_addr, &interface);
    } else {
        std_socket.set_broadcast(true)?;
        let _ = std_socket.set_multicast_ttl_v4(1);
    }

    Ok(std_socket)
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
        )
        .encode();

        // 核心：遍历所有可能地址，仅路由存在的网卡能发送成功
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
                    )
                    .encode();
                    let target = format!("{}:{}", addr.ip(), peer_port);
                    let _ = socket.send_to(reply.as_bytes(), &target);
                }
            }
        }
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
    let msg = local_announcement(user_id, username, port, 0, false, hostname, mac_address).encode();
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
        };

        let encoded = announcement.encode();
        assert!(encoded.starts_with("LANChat|ONLINE|peer-1|Alice|8888|512|0|2|"));
        assert_eq!(
            DiscoveryAnnouncement::parse(&encoded).unwrap(),
            Some(announcement)
        );
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
    fn local_ip_address_never_returns_loopback_or_unspecified() {
        if let Some(address) = local_ip_address() {
            let address: std::net::IpAddr = address.parse().unwrap();
            assert!(!address.is_loopback());
            assert!(!address.is_unspecified());
        }
    }
}
