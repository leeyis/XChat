use futures_util::stream::{FuturesUnordered, StreamExt};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};

use crate::network::discovery_policy::{
    self, DiscoveryNetworkSnapshot, DiscoverySendPlan, DiscoverySettings,
    MAX_INTERFACE_DATAGRAMS_PER_CYCLE,
};
use crate::peers::PeerManager;

pub const DISCOVERY_PROTOCOL_VERSION: u16 = 2;
pub const DISCOVERY_CAPABILITIES: &[&str] = &[
    "group_chat",
    "receipts",
    "transfer_cancel",
    "parallel_file_v2",
    "parallel_file_v3:16",
];
static LOCAL_DEVICE_METADATA: OnceLock<(Option<String>, Option<String>)> = OnceLock::new();
/// 当前对外展示/使用的本机 IP。外层 `None` 表示尚未探测过，
/// 内层 `None` 表示探测过但没有可用地址（避免每次快照都重复全量探测）。
static LOCAL_IP_ADDRESS: RwLock<Option<Option<String>>> = RwLock::new(None);
static ALL_LOCAL_IPS: RwLock<Option<Vec<String>>> = RwLock::new(None);

const DISCOVERY_STEADY_INTERVAL: Duration = Duration::from_secs(30);
const FIXED_PEER_INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const FIXED_PEER_MAX_BACKOFF: Duration = Duration::from_secs(300);

#[derive(Debug, Default)]
struct DiscoveryCadence {
    burst_sends: u8,
}

impl DiscoveryCadence {
    fn delay_after_send(&mut self, jitter_sample: u16) -> Duration {
        match self.burst_sends {
            0 => {
                self.burst_sends = 1;
                Duration::from_millis(400)
            }
            1 => {
                self.burst_sends = 2;
                Duration::from_millis(1100)
            }
            _ => {
                self.burst_sends = 3;
                steady_discovery_delay(jitter_sample)
            }
        }
    }

    fn reset_burst(&mut self) {
        self.burst_sends = 0;
    }
}

fn steady_discovery_delay(jitter_sample: u16) -> Duration {
    let sample = u64::from(jitter_sample.min(1000));
    Duration::from_millis(24_000 + sample * 12)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReplyDedupeKey {
    peer_id: String,
    source_ip: Ipv4Addr,
    frame_digest: u64,
}

#[derive(Debug)]
struct ReplyDeduper {
    ttl: Duration,
    seen: HashMap<ReplyDedupeKey, Instant>,
}

impl ReplyDeduper {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            seen: HashMap::new(),
        }
    }

    fn should_accept(
        &mut self,
        peer_id: &str,
        source_ip: Ipv4Addr,
        frame: &str,
        now: Instant,
    ) -> bool {
        self.seen
            .retain(|_, seen_at| now.saturating_duration_since(*seen_at) < self.ttl);
        let mut hasher = DefaultHasher::new();
        frame.hash(&mut hasher);
        let key = ReplyDedupeKey {
            peer_id: peer_id.to_string(),
            source_ip,
            frame_digest: hasher.finish(),
        };
        if self.seen.contains_key(&key) {
            return false;
        }
        self.seen.insert(key, now);
        true
    }
}

#[derive(Debug, Clone)]
struct FixedPeerRetryState {
    consecutive_failures: u8,
    next_attempt: Instant,
}

impl FixedPeerRetryState {
    fn new(now: Instant) -> Self {
        Self {
            consecutive_failures: 0,
            next_attempt: now,
        }
    }

    fn can_attempt(&self, now: Instant) -> bool {
        now >= self.next_attempt
    }

    fn record_failure(&mut self, now: Instant) {
        let multiplier = 1u64 << u32::from(self.consecutive_failures.min(6));
        let delay = Duration::from_secs(
            (FIXED_PEER_INITIAL_BACKOFF.as_secs() * multiplier)
                .min(FIXED_PEER_MAX_BACKOFF.as_secs()),
        );
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.next_attempt = now + delay;
    }

    fn record_success(&mut self, now: Instant) {
        self.consecutive_failures = 0;
        self.next_attempt = now + DISCOVERY_STEADY_INTERVAL;
    }

    fn reset(&mut self, now: Instant) {
        self.consecutive_failures = 0;
        self.next_attempt = now;
    }
}

#[derive(Debug, Default, serde::Serialize)]
struct DiscoverySendCounter {
    attempts: u64,
    success: u64,
    failure: u64,
}

#[derive(Debug, Default, serde::Serialize)]
struct DiscoveryReceiveCounter {
    announcements: u64,
    deduplicated: u64,
    replies: u64,
}

#[derive(Debug)]
struct DiscoveryMetricsWindow {
    started_at: Instant,
    send: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, DiscoverySendCounter>,
    >,
    receive: DiscoveryReceiveCounter,
}

impl DiscoveryMetricsWindow {
    fn new(now: Instant) -> Self {
        Self {
            started_at: now,
            send: std::collections::BTreeMap::new(),
            receive: DiscoveryReceiveCounter::default(),
        }
    }

    fn record_send(&mut self, interface_id: &str, target_kind: &str, success: bool) {
        let counter = self
            .send
            .entry(interface_id.to_string())
            .or_default()
            .entry(target_kind.to_string())
            .or_default();
        counter.attempts += 1;
        if success {
            counter.success += 1;
        } else {
            counter.failure += 1;
        }
    }

    fn record_receive(&mut self, deduplicated: bool, replied: bool) {
        self.receive.announcements += 1;
        self.receive.deduplicated += u64::from(deduplicated);
        self.receive.replies += u64::from(replied);
    }

    fn record_reply(&mut self) {
        self.receive.replies += 1;
    }

    fn report_if_due(
        &mut self,
        now: Instant,
        send_budget: usize,
        excluded_interfaces: &[(String, String)],
    ) -> Option<serde_json::Value> {
        if now.saturating_duration_since(self.started_at) < Duration::from_secs(60) {
            return None;
        }
        self.started_at = now;
        let send = std::mem::take(&mut self.send);
        let receive = std::mem::take(&mut self.receive);
        Some(serde_json::json!({
            "window_seconds": 60,
            "send_budget": send_budget,
            "send": send,
            "receive": receive,
            "excluded_interfaces": excluded_interfaces
                .iter()
                .map(|(name, reason)| serde_json::json!({
                    "name": name,
                    "reason": reason,
                }))
                .collect::<Vec<_>>(),
        }))
    }
}

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

fn enabled_local_ips(snapshot: &DiscoveryNetworkSnapshot) -> Vec<String> {
    snapshot
        .interfaces
        .iter()
        .filter(|interface| interface.enabled)
        .flat_map(|interface| interface.addresses.iter())
        .map(|address| address.ipv4.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
fn local_ip_for_snapshot(snapshot: &DiscoveryNetworkSnapshot) -> Option<String> {
    default_local_ip(&enabled_local_ips(snapshot))
}

#[cfg(test)]
fn cached_ip_list(cached: &Option<Vec<String>>) -> Option<Vec<String>> {
    cached.clone()
}

/// 默认展示地址取真实接口清单中的首个地址，不再通过默认路由探测。
fn default_local_ip(all_ips: &[String]) -> Option<String> {
    all_ips.first().cloned()
}

/// 后台重扫时的取舍：手动选的地址还在就保留，没了才回落到默认
pub(crate) fn keep_or_reselect(current: Option<&str>, all_ips: &[String]) -> Option<String> {
    match current {
        Some(ip) if all_ips.iter().any(|candidate| candidate == ip) => Some(ip.to_string()),
        _ => default_local_ip(all_ips),
    }
}

/// 用持久化发现策略的当前快照刷新 IP 列表；全关时保持已探测为空，不回退默认策略。
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub(crate) fn refresh_local_ips(snapshot: &DiscoveryNetworkSnapshot) -> Vec<String> {
    let all_ips = enabled_local_ips(snapshot);
    let preferred = default_local_ip(&all_ips);

    *ALL_LOCAL_IPS.write().unwrap() = Some(all_ips.clone());
    *LOCAL_IP_ADDRESS.write().unwrap() = Some(preferred.clone());

    println!("[IP] 刷新本机 IP: 当前 {:?}，可用 {:?}", preferred, all_ips);
    all_ips
}

pub(crate) fn local_ip_address() -> Option<String> {
    LOCAL_IP_ADDRESS.read().unwrap().clone().flatten()
}

/// 从已探测到的列表中手动切换当前 IP
#[cfg(feature = "desktop")]
pub(crate) fn set_local_ip(ip: String) -> bool {
    if !ALL_LOCAL_IPS
        .read()
        .unwrap()
        .as_ref()
        .is_some_and(|addresses| addresses.contains(&ip))
    {
        eprintln!("[IP] 目标地址不在可用列表中: {}", ip);
        return false;
    }
    *LOCAL_IP_ADDRESS.write().unwrap() = Some(Some(ip.clone()));
    println!("[IP] 手动切换本机 IP 为: {}", ip);
    true
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
fn create_discovery_socket(bind_addr: &str) -> Result<UdpSocket, std::io::Error> {
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

    Ok(socket.into())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "visionos",
    target_os = "tvos",
    target_os = "watchos"
))]
fn bind_socket_to_interface_index(
    socket: &Socket,
    interface_index: Option<u32>,
) -> std::io::Result<()> {
    let index = interface_index
        .and_then(std::num::NonZeroU32::new)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing interface index")
        })?;
    socket.bind_device_by_index_v4(Some(index))
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "visionos",
    target_os = "tvos",
    target_os = "watchos"
)))]
fn bind_socket_to_interface_index(
    _socket: &Socket,
    _interface_index: Option<u32>,
) -> std::io::Result<()> {
    Ok(())
}

fn bind_source_socket(
    local_ip: Ipv4Addr,
    interface_index: Option<u32>,
) -> Result<UdpSocket, std::io::Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    bind_socket_to_interface_index(&socket, interface_index)?;
    socket.bind(&SocketAddr::from((local_ip, 0)).into())?;
    Ok(socket.into())
}

/// 把发送 socket 绑到某张网卡的地址上。
/// 绑定源地址会强制包从这张卡出去；目标由真实前缀计算定向广播，
/// 组播同时显式指定出口并限制 TTL，均不依赖默认路由兜底。
fn bind_interface_socket(
    local_ip: Ipv4Addr,
    interface_index: Option<u32>,
) -> Result<UdpSocket, std::io::Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    bind_socket_to_interface_index(&socket, interface_index)?;
    let addr = std::net::SocketAddr::from((local_ip, 0));
    socket.bind(&addr.into())?;
    socket.set_broadcast(true)?;
    socket.set_multicast_ttl_v4(1)?;
    // IP_MULTICAST_IF：仅靠绑定源地址在部分平台不足以决定组播出口，必须显式指定
    socket.set_multicast_if_v4(&local_ip)?;
    Ok(socket.into())
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg(any(test, target_os = "windows"))]
struct WindowsControlHeader {
    length: usize,
    level: i32,
    kind: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[cfg(any(test, target_os = "windows"))]
struct WindowsIpv4PacketInfo {
    address: u32,
    interface_index: u32,
}

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_IPPROTO_IP: i32 = 0;
#[cfg(any(test, target_os = "windows"))]
const WINDOWS_IP_PKTINFO: i32 = 19;

#[cfg(any(test, target_os = "windows"))]
fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value & !(alignment - 1))
}

#[cfg(any(test, target_os = "windows"))]
fn windows_cmsg_data_offset() -> usize {
    align_up(
        std::mem::size_of::<WindowsControlHeader>(),
        std::mem::align_of::<usize>(),
    )
    .expect("control header alignment is representable")
}

#[cfg(any(test, target_os = "windows"))]
fn parse_windows_pktinfo_control(control: &[u8]) -> Option<u32> {
    let header_size = std::mem::size_of::<WindowsControlHeader>();
    let data_offset = windows_cmsg_data_offset();
    let packet_info_size = std::mem::size_of::<WindowsIpv4PacketInfo>();
    let mut offset = 0usize;

    while control.len().saturating_sub(offset) >= header_size {
        let header = unsafe {
            control
                .as_ptr()
                .add(offset)
                .cast::<WindowsControlHeader>()
                .read_unaligned()
        };
        let remaining = control.len() - offset;
        if header.length < data_offset || header.length > remaining {
            return None;
        }
        if header.level == WINDOWS_IPPROTO_IP
            && header.kind == WINDOWS_IP_PKTINFO
            && header.length >= data_offset + packet_info_size
        {
            let packet_info = unsafe {
                control
                    .as_ptr()
                    .add(offset + data_offset)
                    .cast::<WindowsIpv4PacketInfo>()
                    .read_unaligned()
            };
            return (packet_info.interface_index != 0).then_some(packet_info.interface_index);
        }

        let step = align_up(header.length, std::mem::align_of::<WindowsControlHeader>())?;
        if step == 0 || step > remaining {
            return None;
        }
        offset += step;
    }

    None
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn enable_ingress_interface_metadata(socket: &UdpSocket) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let enabled: libc::c_int = 1;
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_PKTINFO,
            (&enabled as *const libc::c_int).cast(),
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
fn enable_ingress_interface_metadata(socket: &UdpSocket) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let enabled: libc::c_int = 1;
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_RECVIF,
            (&enabled as *const libc::c_int).cast(),
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))
))]
fn enable_ingress_interface_metadata(_socket: &UdpSocket) -> std::io::Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
static WINDOWS_WSA_RECV_MSG: OnceLock<windows_sys::Win32::Networking::WinSock::LPFN_WSARECVMSG> =
    OnceLock::new();

#[cfg(target_os = "windows")]
fn windows_socket_error() -> std::io::Error {
    use windows_sys::Win32::Networking::WinSock;

    std::io::Error::from_raw_os_error(unsafe { WinSock::WSAGetLastError() })
}

#[cfg(target_os = "windows")]
fn windows_socket_handle(
    socket: &impl std::os::windows::io::AsRawSocket,
) -> windows_sys::Win32::Networking::WinSock::SOCKET {
    socket.as_raw_socket() as windows_sys::Win32::Networking::WinSock::SOCKET
}

#[cfg(target_os = "windows")]
fn load_windows_wsa_recv_msg(
    socket: &impl std::os::windows::io::AsRawSocket,
) -> windows_sys::Win32::Networking::WinSock::LPFN_WSARECVMSG {
    use windows_sys::Win32::Networking::WinSock;

    let guid = WinSock::WSAID_WSARECVMSG;
    let mut receiver: WinSock::LPFN_WSARECVMSG = None;
    let mut bytes_returned = 0u32;
    let result = unsafe {
        WinSock::WSAIoctl(
            windows_socket_handle(socket),
            WinSock::SIO_GET_EXTENSION_FUNCTION_POINTER,
            (&guid as *const windows_sys::core::GUID).cast(),
            std::mem::size_of_val(&guid) as u32,
            (&mut receiver as *mut WinSock::LPFN_WSARECVMSG).cast(),
            std::mem::size_of_val(&receiver) as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
            None,
        )
    };
    if result == 0 && bytes_returned as usize == std::mem::size_of_val(&receiver) {
        receiver
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn windows_wsa_recv_msg(
    socket: &impl std::os::windows::io::AsRawSocket,
) -> std::io::Result<windows_sys::Win32::Networking::WinSock::LPFN_WSARECVMSG> {
    let receiver = *WINDOWS_WSA_RECV_MSG.get_or_init(|| load_windows_wsa_recv_msg(socket));
    if receiver.is_some() {
        Ok(receiver)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Windows network stack does not support WSARecvMsg",
        ))
    }
}

#[cfg(target_os = "windows")]
fn enable_ingress_interface_metadata(socket: &UdpSocket) -> std::io::Result<()> {
    use windows_sys::Win32::Networking::WinSock;

    debug_assert_eq!(WINDOWS_IPPROTO_IP, WinSock::IPPROTO_IP);
    debug_assert_eq!(WINDOWS_IP_PKTINFO, WinSock::IP_PKTINFO);
    debug_assert_eq!(
        std::mem::size_of::<WindowsControlHeader>(),
        std::mem::size_of::<WinSock::CMSGHDR>(),
    );
    debug_assert_eq!(
        std::mem::size_of::<WindowsIpv4PacketInfo>(),
        std::mem::size_of::<WinSock::IN_PKTINFO>(),
    );

    let enabled = 1u32;
    let result = unsafe {
        WinSock::setsockopt(
            windows_socket_handle(socket),
            WinSock::IPPROTO_IP,
            WinSock::IP_PKTINFO,
            (&enabled as *const u32).cast(),
            std::mem::size_of_val(&enabled) as i32,
        )
    };
    if result != 0 {
        return Err(windows_socket_error());
    }
    windows_wsa_recv_msg(socket)?;
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn enable_ingress_interface_metadata(_socket: &UdpSocket) -> std::io::Result<()> {
    Ok(())
}

fn create_listener_socket(bind_addr: &str) -> std::io::Result<tokio::net::UdpSocket> {
    let socket = create_discovery_socket(bind_addr)?;
    enable_ingress_interface_metadata(&socket)?;
    socket.set_nonblocking(true)?;
    tokio::net::UdpSocket::from_std(socket)
}

#[derive(Debug)]
struct ReceivedDiscoveryPacket {
    size: usize,
    source: SocketAddr,
    ingress_index: Option<u32>,
}

#[cfg(unix)]
fn try_recv_discovery_packet(
    socket: &tokio::net::UdpSocket,
    buffer: &mut [u8],
) -> std::io::Result<ReceivedDiscoveryPacket> {
    use std::os::fd::AsRawFd;

    let mut source: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut iov = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: buffer.len(),
    };
    let mut control = [0 as libc::c_long; 32];
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_name = (&mut source as *mut libc::sockaddr_storage).cast();
    message.msg_namelen = std::mem::size_of_val(&source) as libc::socklen_t;
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = std::mem::size_of_val(&control) as _;

    let received = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut message, libc::MSG_DONTWAIT) };
    if received < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if source.ss_family != libc::AF_INET as libc::sa_family_t {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "discovery packet source is not IPv4",
        ));
    }
    let source_v4 =
        unsafe { &*((&source as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>()) };
    let source = SocketAddr::from((
        Ipv4Addr::from(source_v4.sin_addr.s_addr.to_ne_bytes()),
        u16::from_be(source_v4.sin_port),
    ));

    let mut ingress_index = None;
    let mut header = unsafe { libc::CMSG_FIRSTHDR(&message) };
    while !header.is_null() {
        let header_ref = unsafe { &*header };
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if header_ref.cmsg_level == libc::IPPROTO_IP && header_ref.cmsg_type == libc::IP_PKTINFO {
            let packet_info = unsafe { &*(libc::CMSG_DATA(header).cast::<libc::in_pktinfo>()) };
            ingress_index = u32::try_from(packet_info.ipi_ifindex).ok();
        }
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        ))]
        if header_ref.cmsg_level == libc::IPPROTO_IP && header_ref.cmsg_type == libc::IP_RECVIF {
            let link = unsafe { &*(libc::CMSG_DATA(header).cast::<libc::sockaddr_dl>()) };
            ingress_index = Some(u32::from(link.sdl_index));
        }
        header = unsafe { libc::CMSG_NXTHDR(&message, header) };
    }

    Ok(ReceivedDiscoveryPacket {
        size: received as usize,
        source,
        ingress_index,
    })
}

#[cfg(target_os = "windows")]
fn try_recv_discovery_packet(
    socket: &tokio::net::UdpSocket,
    buffer: &mut [u8],
) -> std::io::Result<ReceivedDiscoveryPacket> {
    use windows_sys::Win32::Networking::WinSock;

    #[repr(align(8))]
    struct AlignedControl([u8; 128]);

    let receiver = windows_wsa_recv_msg(socket)?.expect("WSARecvMsg was checked above");
    let mut source = WinSock::SOCKADDR_IN::default();
    let mut data = WinSock::WSABUF {
        len: buffer.len().min(u32::MAX as usize) as u32,
        buf: buffer.as_mut_ptr(),
    };
    let mut control = AlignedControl([0; 128]);
    let mut message = WinSock::WSAMSG {
        name: (&mut source as *mut WinSock::SOCKADDR_IN).cast(),
        namelen: std::mem::size_of_val(&source) as i32,
        lpBuffers: &mut data,
        dwBufferCount: 1,
        Control: WinSock::WSABUF {
            len: control.0.len() as u32,
            buf: control.0.as_mut_ptr(),
        },
        dwFlags: 0,
    };
    let mut received = 0u32;
    let result = unsafe {
        receiver(
            windows_socket_handle(socket),
            &mut message,
            &mut received,
            std::ptr::null_mut(),
            None,
        )
    };
    if result != 0 {
        return Err(windows_socket_error());
    }
    if source.sin_family != WinSock::AF_INET {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "discovery packet source is not IPv4",
        ));
    }

    let source_ip = Ipv4Addr::from(unsafe { source.sin_addr.S_un.S_addr }.to_ne_bytes());
    let source = SocketAddr::from((source_ip, u16::from_be(source.sin_port)));
    let control_length = (message.Control.len as usize).min(control.0.len());
    let ingress_index = parse_windows_pktinfo_control(&control.0[..control_length]);

    Ok(ReceivedDiscoveryPacket {
        size: received as usize,
        source,
        ingress_index,
    })
}

#[cfg(any(unix, target_os = "windows"))]
async fn recv_discovery_packet(
    socket: &tokio::net::UdpSocket,
    buffer: &mut [u8],
) -> std::io::Result<ReceivedDiscoveryPacket> {
    socket
        .async_io(tokio::io::Interest::READABLE, || {
            try_recv_discovery_packet(socket, buffer)
        })
        .await
}

#[cfg(not(any(unix, target_os = "windows")))]
async fn recv_discovery_packet(
    socket: &tokio::net::UdpSocket,
    buffer: &mut [u8],
) -> std::io::Result<ReceivedDiscoveryPacket> {
    let (size, source) = socket.recv_from(buffer).await?;
    Ok(ReceivedDiscoveryPacket {
        size,
        source,
        ingress_index: None,
    })
}

fn sync_listener_multicast_memberships(
    socket: &tokio::net::UdpSocket,
    joined: &mut BTreeSet<Ipv4Addr>,
    desired: &BTreeSet<Ipv4Addr>,
) {
    let multicast = Ipv4Addr::new(224, 0, 0, 167);
    for interface in joined.difference(desired).copied().collect::<Vec<_>>() {
        match socket.leave_multicast_v4(multicast, interface) {
            Ok(()) => {
                joined.remove(&interface);
            }
            Err(error) => {
                eprintln!(
                    "[UDP][discovery.listener] leave multicast failed on {interface}: {error}"
                );
            }
        }
    }
    for interface in desired.difference(joined).copied().collect::<Vec<_>>() {
        match socket.join_multicast_v4(multicast, interface) {
            Ok(()) => {
                joined.insert(interface);
            }
            Err(error) => {
                eprintln!(
                    "[UDP][discovery.listener] join multicast failed on {interface}: {error}"
                );
            }
        }
    }
}

const DNS_TTL: Duration = Duration::from_secs(60);
const FIXED_PEER_RESOLVE_TIMEOUT: Duration = Duration::from_secs(2);
const INTERFACE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_FIXED_PEER_DATAGRAMS_PER_CYCLE: usize = 16;
const TOTAL_DISCOVERY_DATAGRAM_BUDGET: usize =
    MAX_INTERFACE_DATAGRAMS_PER_CYCLE + MAX_FIXED_PEER_DATAGRAMS_PER_CYCLE;

type DnsCache = HashMap<String, (Vec<std::net::SocketAddr>, Instant)>;

struct ResolvedFixedPeer {
    addresses: Vec<std::net::SocketAddr>,
    cache_key: Option<String>,
}

type FixedPeerResolutionFuture =
    Pin<Box<dyn Future<Output = (String, Result<ResolvedFixedPeer, String>)> + Send>>;

fn rotating_fixed_peer_endpoints(
    custom_peers: &[String],
    retry_states: &mut HashMap<String, FixedPeerRetryState>,
    cursor: &mut usize,
    now: Instant,
) -> Vec<String> {
    if custom_peers.is_empty() {
        *cursor = 0;
        return Vec::new();
    }
    let start = *cursor % custom_peers.len();
    let mut scanned = 0;
    let mut selected = Vec::new();
    for offset in 0..custom_peers.len() {
        let endpoint = &custom_peers[(start + offset) % custom_peers.len()];
        scanned = offset + 1;
        if retry_states
            .entry(endpoint.clone())
            .or_insert_with(|| FixedPeerRetryState::new(now))
            .can_attempt(now)
        {
            selected.push(endpoint.clone());
            if selected.len() == MAX_FIXED_PEER_DATAGRAMS_PER_CYCLE {
                break;
            }
        }
    }
    *cursor = (start + scanned) % custom_peers.len();
    selected
}

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

#[derive(Default)]
struct FixedPeerSourceResolver {
    dns_cache: DnsCache,
    retry_states: HashMap<String, FixedPeerRetryState>,
    resolved_by_endpoint: HashMap<String, HashSet<Ipv4Addr>>,
    expected_ids_by_source: HashMap<Ipv4Addr, HashSet<String>>,
    verified_endpoints_by_device_id: HashMap<String, String>,
    cursor: usize,
}

fn fixed_peer_identity_allowed(
    expected_ids_by_source: &HashMap<Ipv4Addr, HashSet<String>>,
    source_ip: Ipv4Addr,
    peer_id: &str,
) -> bool {
    expected_ids_by_source
        .get(&source_ip)
        .is_none_or(|expected_ids| expected_ids.contains(peer_id))
}

impl FixedPeerSourceResolver {
    fn reset_after_network_change(&mut self, now: Instant) {
        self.dns_cache.clear();
        self.resolved_by_endpoint.clear();
        self.expected_ids_by_source.clear();
        for state in self.retry_states.values_mut() {
            state.reset(now);
        }
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

    fn identity_allowed(&self, source_ip: Ipv4Addr, peer_id: &str) -> bool {
        fixed_peer_identity_allowed(&self.expected_ids_by_source, source_ip, peer_id)
    }

    async fn refresh(&mut self, custom_peers: &[String], port: u16) -> HashSet<Ipv4Addr> {
        let known = custom_peers.iter().cloned().collect::<HashSet<_>>();
        self.retry_states
            .retain(|endpoint, _| known.contains(endpoint));
        self.resolved_by_endpoint
            .retain(|endpoint, _| known.contains(endpoint));

        let now = Instant::now();
        let endpoints = rotating_fixed_peer_endpoints(
            custom_peers,
            &mut self.retry_states,
            &mut self.cursor,
            now,
        );
        let cache_snapshot = Arc::new(self.dns_cache.clone());
        let mut resolutions = FuturesUnordered::<FixedPeerResolutionFuture>::new();
        for endpoint in endpoints {
            let cache_snapshot = Arc::clone(&cache_snapshot);
            resolutions.push(Box::pin(async move {
                let result = bounded_resolution(
                    FIXED_PEER_RESOLVE_TIMEOUT,
                    resolve_fixed_peer(&endpoint, port, now, cache_snapshot.as_ref()),
                )
                .await
                .and_then(|result| result);
                (endpoint, result)
            }));
        }

        while let Some((endpoint, result)) = resolutions.next().await {
            let completed_at = Instant::now();
            match result {
                Ok(resolved) => {
                    if let Some(cache_key) = resolved.cache_key {
                        self.dns_cache.insert(
                            cache_key,
                            (resolved.addresses.clone(), completed_at + DNS_TTL),
                        );
                    }
                    let sources = resolved
                        .addresses
                        .into_iter()
                        .filter_map(|address| match address.ip() {
                            std::net::IpAddr::V4(ipv4) => Some(ipv4),
                            std::net::IpAddr::V6(_) => None,
                        })
                        .collect::<HashSet<_>>();
                    if !sources.is_empty() {
                        self.resolved_by_endpoint.insert(endpoint.clone(), sources);
                    }
                    if let Some(state) = self.retry_states.get_mut(&endpoint) {
                        state.record_success(completed_at);
                    }
                }
                Err(error) => {
                    let metric_id = fixed_peer_metric_id(&endpoint);
                    eprintln!("[UDP][discovery.listener] {metric_id} resolve failed: {error}");
                    if let Some(state) = self.retry_states.get_mut(&endpoint) {
                        state.record_failure(completed_at);
                    }
                }
            }
        }

        self.resolved_by_endpoint
            .values()
            .flat_map(|sources| sources.iter().copied())
            .collect()
    }
}

fn discovery_runtime_wake_delay(
    now: Instant,
    discovery_deadline: Instant,
    next_policy_check: Instant,
) -> Duration {
    [
        discovery_deadline.saturating_duration_since(now),
        next_policy_check.saturating_duration_since(now),
    ]
    .into_iter()
    .min()
    .unwrap_or_default()
}

async fn bounded_resolution<T, F>(timeout: Duration, future: F) -> Result<T, String>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| "resolution timed out".to_string())
}

fn fail_closed_network_snapshot() -> DiscoveryNetworkSnapshot {
    DiscoveryNetworkSnapshot {
        settings: DiscoverySettings {
            local_discovery: false,
            vpn_discovery: false,
            interface_overrides: Default::default(),
        },
        interfaces: Vec::new(),
    }
}

fn read_default_network_snapshot() -> DiscoveryNetworkSnapshot {
    discovery_policy::system_network_snapshot(DiscoverySettings::default())
}

fn snapshot_after_read(
    previous: Option<&DiscoveryNetworkSnapshot>,
    result: Result<DiscoveryNetworkSnapshot, String>,
) -> DiscoveryNetworkSnapshot {
    match result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("[UDP][discovery.inventory] settings load failed: {error}");
            previous
                .cloned()
                .unwrap_or_else(fail_closed_network_snapshot)
        }
    }
}

async fn read_network_snapshot(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    previous: Option<&DiscoveryNetworkSnapshot>,
) -> DiscoveryNetworkSnapshot {
    snapshot_after_read(previous, discovery_policy::network_snapshot(pool).await)
}

fn network_fingerprint(snapshot: &DiscoveryNetworkSnapshot) -> String {
    serde_json::to_string(snapshot).unwrap_or_default()
}

fn excluded_interfaces(snapshot: &DiscoveryNetworkSnapshot) -> Vec<(String, String)> {
    snapshot
        .interfaces
        .iter()
        .filter_map(|interface| {
            interface
                .exclusion_reason
                .as_ref()
                .map(|reason| (interface.name.clone(), reason.clone()))
        })
        .collect()
}

fn listener_multicast_memberships(snapshot: &DiscoveryNetworkSnapshot) -> BTreeSet<Ipv4Addr> {
    snapshot
        .interfaces
        .iter()
        .filter(|interface| interface.enabled)
        .flat_map(|interface| interface.addresses.iter())
        .filter_map(|address| address.ipv4.parse().ok())
        .collect()
}

fn address_contains_source(
    address: &discovery_policy::NetworkInterfaceAddress,
    source_ip: Ipv4Addr,
) -> bool {
    let Ok(local_ip) = address.ipv4.parse::<Ipv4Addr>() else {
        return false;
    };
    let Some(prefix) = address.prefix_length.filter(|prefix| *prefix <= 32) else {
        return false;
    };
    if prefix == 0 {
        return false;
    }
    let mask = u32::MAX << (32 - u32::from(prefix));
    u32::from(local_ip) & mask == u32::from(source_ip) & mask
}

fn interface_first_ipv4(interface: &discovery_policy::NetworkInterfaceView) -> Option<Ipv4Addr> {
    interface
        .addresses
        .first()
        .and_then(|address| address.ipv4.parse().ok())
}

fn reply_source_ip(
    snapshot: &DiscoveryNetworkSnapshot,
    source_ip: Ipv4Addr,
    ingress_index: Option<u32>,
) -> Option<Ipv4Addr> {
    reply_source_ip_with_unmapped_ingress(
        snapshot,
        source_ip,
        ingress_index,
        cfg!(target_os = "android"),
    )
}

fn reply_source_ip_with_unmapped_ingress(
    snapshot: &DiscoveryNetworkSnapshot,
    source_ip: Ipv4Addr,
    ingress_index: Option<u32>,
    allow_unmapped_ingress: bool,
) -> Option<Ipv4Addr> {
    if let Some(index) = ingress_index {
        return match snapshot
            .interfaces
            .iter()
            .find(|interface| interface.index == Some(index))
        {
            Some(interface) if interface.enabled => interface_first_ipv4(interface),
            Some(_) => None,
            None if allow_unmapped_ingress => snapshot
                .interfaces
                .iter()
                .find(|interface| interface.enabled)
                .and_then(interface_first_ipv4),
            None => None,
        };
    }

    snapshot
        .interfaces
        .iter()
        .filter(|interface| interface.enabled)
        .find_map(|interface| {
            interface
                .addresses
                .iter()
                .find(|address| address_contains_source(address, source_ip))
                .and_then(|address| address.ipv4.parse().ok())
        })
        .or_else(|| {
            allow_unmapped_ingress.then(|| {
                snapshot
                    .interfaces
                    .iter()
                    .find(|interface| interface.enabled)
                    .and_then(interface_first_ipv4)
            })?
        })
}

fn reply_source_ip_for_packet(
    snapshot: &DiscoveryNetworkSnapshot,
    source_ip: Ipv4Addr,
    ingress_index: Option<u32>,
    explicitly_fixed: bool,
) -> Option<Ipv4Addr> {
    reply_source_ip(snapshot, source_ip, ingress_index).or_else(|| {
        explicitly_fixed.then(|| {
            if let Some(index) = ingress_index {
                return snapshot
                    .interfaces
                    .iter()
                    .find(|interface| interface.index == Some(index))
                    .and_then(interface_first_ipv4);
            }
            snapshot.interfaces.iter().find_map(|interface| {
                interface
                    .addresses
                    .iter()
                    .find(|address| address_contains_source(address, source_ip))
                    .and_then(|address| address.ipv4.parse().ok())
            })
        })?
    })
}

fn local_interface_index(snapshot: &DiscoveryNetworkSnapshot, local_ip: Ipv4Addr) -> Option<u32> {
    snapshot.interfaces.iter().find_map(|interface| {
        interface
            .addresses
            .iter()
            .any(|address| address.ipv4.parse::<Ipv4Addr>() == Ok(local_ip))
            .then_some(interface.index)
            .flatten()
    })
}

fn discovery_packet_allowed(
    snapshot: &DiscoveryNetworkSnapshot,
    source_ip: Ipv4Addr,
    ingress_index: Option<u32>,
    fixed_peer_ips: &HashSet<Ipv4Addr>,
) -> bool {
    discovery_packet_allowed_with_unmapped_ingress(
        snapshot,
        source_ip,
        ingress_index,
        fixed_peer_ips,
        cfg!(target_os = "android"),
    )
}

fn discovery_packet_allowed_with_unmapped_ingress(
    snapshot: &DiscoveryNetworkSnapshot,
    source_ip: Ipv4Addr,
    ingress_index: Option<u32>,
    fixed_peer_ips: &HashSet<Ipv4Addr>,
    allow_unmapped_ingress: bool,
) -> bool {
    if fixed_peer_ips.contains(&source_ip) {
        return true;
    }
    if let Some(index) = ingress_index {
        if let Some(interface) = snapshot
            .interfaces
            .iter()
            .find(|interface| interface.index == Some(index))
        {
            return interface.enabled;
        }
        return allow_unmapped_ingress
            && snapshot
                .interfaces
                .iter()
                .any(|interface| interface.enabled);
    }

    if allow_unmapped_ingress
        && snapshot
            .interfaces
            .iter()
            .any(|interface| interface.enabled)
    {
        return true;
    }

    reply_source_ip(snapshot, source_ip, None).is_some() || fixed_peer_ips.contains(&source_ip)
}

pub(crate) fn update_local_ip_cache(snapshot: &DiscoveryNetworkSnapshot) -> Vec<String> {
    let all_ips = enabled_local_ips(snapshot);
    let current = LOCAL_IP_ADDRESS.read().unwrap().clone().flatten();
    let preferred = keep_or_reselect(current.as_deref(), &all_ips);
    *ALL_LOCAL_IPS.write().unwrap() = Some(all_ips.clone());
    *LOCAL_IP_ADDRESS.write().unwrap() = Some(preferred);
    all_ips
}

fn log_network_plan(snapshot: &DiscoveryNetworkSnapshot, plan: &DiscoverySendPlan) {
    println!(
        "[UDP][discovery.plan] {}",
        serde_json::json!({
            "interfaces": snapshot.interfaces,
            "interface_targets": plan.targets.len(),
            "interface_budget": plan.budget,
            "fixed_peer_budget": MAX_FIXED_PEER_DATAGRAMS_PER_CYCLE,
            "total_budget": TOTAL_DISCOVERY_DATAGRAM_BUDGET,
        })
    );
}

fn send_interface_announcements(
    plan: &DiscoverySendPlan,
    message: &str,
    metrics: &mut DiscoveryMetricsWindow,
) {
    let mut sockets = HashMap::<(String, Ipv4Addr), Option<UdpSocket>>::new();
    for target in &plan.targets {
        let socket = sockets
            .entry((target.interface_id.clone(), target.source_ip))
            .or_insert_with(|| {
                match bind_interface_socket(target.source_ip, target.interface_index) {
                    Ok(socket) => Some(socket),
                    Err(error) => {
                        eprintln!(
                            "[UDP][discovery.send] bind failed on {} ({}): {error}",
                            target.interface_id, target.source_ip
                        );
                        None
                    }
                }
            });
        let success = socket.as_ref().is_some_and(|socket| {
            socket
                .send_to(message.as_bytes(), target.destination)
                .is_ok()
        });
        metrics.record_send(&target.interface_id, target.kind.as_str(), success);
    }
}

fn fixed_peer_metric_id(endpoint: &str) -> String {
    let mut hasher = DefaultHasher::new();
    endpoint.hash(&mut hasher);
    format!("fixed:{:08x}", hasher.finish() as u32)
}

async fn resolve_fixed_peer(
    endpoint: &str,
    port: u16,
    now: Instant,
    dns_cache: &DnsCache,
) -> Result<ResolvedFixedPeer, String> {
    if let Ok(address) = endpoint.parse::<std::net::SocketAddr>() {
        return Ok(ResolvedFixedPeer {
            addresses: vec![address],
            cache_key: None,
        });
    }
    if let Ok(ip) = endpoint.parse::<std::net::IpAddr>() {
        return Ok(ResolvedFixedPeer {
            addresses: vec![std::net::SocketAddr::new(ip, port)],
            cache_key: None,
        });
    }
    let with_port = if endpoint.contains(':') {
        endpoint.to_string()
    } else {
        format!("{endpoint}:{port}")
    };
    if let Some((cached, expiry)) = dns_cache.get(&with_port) {
        if *expiry > now {
            return Ok(ResolvedFixedPeer {
                addresses: cached.clone(),
                cache_key: None,
            });
        }
    }
    let addresses = tokio::net::lookup_host(&with_port)
        .await
        .map_err(|error| error.to_string())?
        .filter(|address| address.is_ipv4())
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err("no IPv4 address resolved".to_string());
    }
    Ok(ResolvedFixedPeer {
        addresses,
        cache_key: Some(with_port),
    })
}

async fn refresh_listener_policy(
    socket: &tokio::net::UdpSocket,
    pool: &sqlx::Pool<sqlx::Sqlite>,
    port: u16,
    snapshot: &mut DiscoveryNetworkSnapshot,
    joined: &mut BTreeSet<Ipv4Addr>,
    fixed_peer_resolver: &mut FixedPeerSourceResolver,
    fixed_peer_ips: &mut HashSet<Ipv4Addr>,
) {
    let latest = read_network_snapshot(pool, Some(snapshot)).await;
    if network_fingerprint(&latest) != network_fingerprint(snapshot) {
        fixed_peer_resolver.reset_after_network_change(Instant::now());
    }
    let desired = listener_multicast_memberships(&latest);
    sync_listener_multicast_memberships(socket, joined, &desired);
    *snapshot = latest;
    let records = crate::db::get_custom_peer_records(pool).await;
    let custom_peers = records
        .iter()
        .filter(|record| record.is_verified())
        .map(|record| record.endpoint.clone())
        .collect::<Vec<_>>();
    *fixed_peer_ips = fixed_peer_resolver.refresh(&custom_peers, port).await;
    fixed_peer_resolver.bind_verified_identities(&records);
}

fn send_discovery_reply(
    snapshot: &DiscoveryNetworkSnapshot,
    fixed_peer_ips: &HashSet<Ipv4Addr>,
    source_ip: Ipv4Addr,
    ingress_index: Option<u32>,
    target: SocketAddr,
    message: &[u8],
) -> bool {
    let explicitly_fixed = fixed_peer_ips.contains(&source_ip);
    let socket =
        match reply_source_ip_for_packet(snapshot, source_ip, ingress_index, explicitly_fixed) {
            Some(local_ip) => bind_source_socket(
                local_ip,
                ingress_index.or_else(|| local_interface_index(snapshot, local_ip)),
            ),
            None if explicitly_fixed => create_discovery_socket("0.0.0.0:0"),
            None => return false,
        };
    socket
        .and_then(|socket| socket.send_to(message, target))
        .is_ok()
}

async fn send_fixed_peer_announcements(
    socket: &UdpSocket,
    message: &str,
    custom_peers: &[String],
    port: u16,
    dns_cache: &mut DnsCache,
    retry_states: &mut HashMap<String, FixedPeerRetryState>,
    cursor: &mut usize,
    metrics: &mut DiscoveryMetricsWindow,
) {
    let known = custom_peers
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    retry_states.retain(|endpoint, _| known.contains(endpoint));
    let now = Instant::now();
    let cache_snapshot = Arc::new(dns_cache.clone());
    let mut resolutions = FuturesUnordered::<FixedPeerResolutionFuture>::new();
    for endpoint in rotating_fixed_peer_endpoints(custom_peers, retry_states, cursor, now) {
        let cache_snapshot = Arc::clone(&cache_snapshot);
        resolutions.push(Box::pin(async move {
            let result = bounded_resolution(
                FIXED_PEER_RESOLVE_TIMEOUT,
                resolve_fixed_peer(&endpoint, port, now, cache_snapshot.as_ref()),
            )
            .await
            .and_then(|result| result);
            (endpoint, result)
        }));
    }

    while let Some((endpoint, resolved)) = resolutions.next().await {
        let metric_id = fixed_peer_metric_id(&endpoint);
        let completed_at = Instant::now();
        let any_success = match resolved {
            Ok(resolved) => {
                if let Some(cache_key) = resolved.cache_key {
                    dns_cache.insert(
                        cache_key,
                        (resolved.addresses.clone(), completed_at + DNS_TTL),
                    );
                }
                let success = resolved
                    .addresses
                    .into_iter()
                    .next()
                    .is_some_and(|address| socket.send_to(message.as_bytes(), address).is_ok());
                metrics.record_send(&metric_id, "fixed_unicast", success);
                success
            }
            Err(error) => {
                metrics.record_send(&metric_id, "fixed_unicast", false);
                eprintln!("[UDP][discovery.fixed] {metric_id} resolve/send failed: {error}");
                false
            }
        };

        if let Some(state) = retry_states.get_mut(&endpoint) {
            if any_success {
                state.record_success(completed_at);
            } else {
                state.record_failure(completed_at);
            }
        }
    }
}

pub async fn start_announcing(port: u16, user_id: String, pool: sqlx::Pool<sqlx::Sqlite>) {
    use rand::Rng;
    use sysinfo::System;

    let unicast_socket = match create_discovery_socket("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("[UDP] 创建固定地址发送 socket 失败: {error}");
            return;
        }
    };
    let (hostname, mac_address) = local_device_metadata();
    let mut system = System::new();
    let mut snapshot = read_network_snapshot(&pool, None).await;
    let mut fingerprint = network_fingerprint(&snapshot);
    let mut cadence = DiscoveryCadence::default();
    let mut dns_cache = DnsCache::new();
    let mut retry_states = HashMap::new();
    let mut fixed_peer_cursor = 0;
    let mut metrics = DiscoveryMetricsWindow::new(Instant::now());
    update_local_ip_cache(&snapshot);
    log_network_plan(
        &snapshot,
        &discovery_policy::build_send_plan(&snapshot, port),
    );

    loop {
        let username = crate::db::get_username(&pool)
            .await
            .unwrap_or_else(|_| "Unknown".to_string());
        system.refresh_memory();
        let available_memory_mb = system.available_memory() / (1024 * 1024);
        let message = local_announcement(
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

        let plan = discovery_policy::build_send_plan(&snapshot, port);
        send_interface_announcements(&plan, &message, &mut metrics);
        let custom_peers = crate::db::get_custom_peers(&pool).await;
        send_fixed_peer_announcements(
            &unicast_socket,
            &message,
            &custom_peers,
            port,
            &mut dns_cache,
            &mut retry_states,
            &mut fixed_peer_cursor,
            &mut metrics,
        )
        .await;
        if let Some(report) = metrics.report_if_due(
            Instant::now(),
            TOTAL_DISCOVERY_DATAGRAM_BUDGET,
            &excluded_interfaces(&snapshot),
        ) {
            println!("[UDP][discovery.metrics] {report}");
        }

        let jitter_sample = rand::thread_rng().gen_range(0..=1000);
        let deadline = Instant::now() + cadence.delay_after_send(jitter_sample);
        let mut next_policy_check = Instant::now() + INTERFACE_POLL_INTERVAL;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let settings_changed = tokio::select! {
                _ = tokio::time::sleep(discovery_runtime_wake_delay(
                    now,
                    deadline,
                    next_policy_check,
                )) => false,
                _ = discovery_policy::wait_for_settings_change() => true,
            };
            let now = Instant::now();
            if now >= deadline && !settings_changed {
                break;
            }
            if !settings_changed && now < next_policy_check {
                continue;
            }
            next_policy_check = now + INTERFACE_POLL_INTERVAL;

            let latest = read_network_snapshot(&pool, Some(&snapshot)).await;
            let latest_fingerprint = network_fingerprint(&latest);
            if settings_changed || latest_fingerprint != fingerprint {
                snapshot = latest;
                fingerprint = latest_fingerprint;
                update_local_ip_cache(&snapshot);
                cadence.reset_burst();
                for state in retry_states.values_mut() {
                    state.reset(Instant::now());
                }
                dns_cache.clear();
                log_network_plan(
                    &snapshot,
                    &discovery_policy::build_send_plan(&snapshot, port),
                );
                break;
            }

            let custom_peers = crate::db::get_custom_peers(&pool).await;
            send_fixed_peer_announcements(
                &unicast_socket,
                &message,
                &custom_peers,
                port,
                &mut dns_cache,
                &mut retry_states,
                &mut fixed_peer_cursor,
                &mut metrics,
            )
            .await;
            let now = Instant::now();
            if let Some(report) = metrics.report_if_due(
                now,
                TOTAL_DISCOVERY_DATAGRAM_BUDGET,
                &excluded_interfaces(&snapshot),
            ) {
                println!("[UDP][discovery.metrics] {report}");
            }
        }
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
    let socket = match create_listener_socket(&bind_addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[UDP] 创建监听 socket 失败: {}", e);
            return;
        }
    };

    let mut buf = [0u8; 1024];
    let (hostname, mac_address) = local_device_metadata();
    let mut deduper = ReplyDeduper::new(Duration::from_secs(2));
    let mut metrics = DiscoveryMetricsWindow::new(Instant::now());
    let mut snapshot = fail_closed_network_snapshot();
    let mut joined = BTreeSet::new();
    let mut fixed_peer_resolver = FixedPeerSourceResolver::default();
    let mut fixed_peer_ips = HashSet::new();
    refresh_listener_policy(
        &socket,
        &pool,
        port,
        &mut snapshot,
        &mut joined,
        &mut fixed_peer_resolver,
        &mut fixed_peer_ips,
    )
    .await;
    let mut next_policy_refresh = tokio::time::Instant::now() + INTERFACE_POLL_INTERVAL;
    println!("[UDP] 正在端口 {} 监听邻居...", port);

    loop {
        let packet = tokio::select! {
            result = recv_discovery_packet(&socket, &mut buf) => match result {
                Ok(packet) => packet,
                Err(error) => {
                    eprintln!("[UDP][discovery.listener] receive failed: {error}");
                    continue;
                }
            },
            _ = tokio::time::sleep_until(next_policy_refresh) => {
                refresh_listener_policy(
                    &socket, &pool, port, &mut snapshot, &mut joined,
                    &mut fixed_peer_resolver, &mut fixed_peer_ips,
                ).await;
                next_policy_refresh = tokio::time::Instant::now() + INTERFACE_POLL_INTERVAL;
                continue;
            },
            _ = discovery_policy::wait_for_settings_change() => {
                refresh_listener_policy(
                    &socket, &pool, port, &mut snapshot, &mut joined,
                    &mut fixed_peer_resolver, &mut fixed_peer_ips,
                ).await;
                next_policy_refresh = tokio::time::Instant::now() + INTERFACE_POLL_INTERVAL;
                continue;
            },
        };
        let size = packet.size;
        let addr = packet.source;
        let ingress_index = packet.ingress_index;
        {
            let msg = String::from_utf8_lossy(&buf[..size]);
            let parts: Vec<&str> = msg.split('|').collect();

            if parts.len() >= 6 && parts[0] == "LANChat" {
                let announcement = match DiscoveryAnnouncement::parse(&msg) {
                    Ok(Some(announcement)) => announcement,
                    _ => continue,
                };
                let peer_id = parts[2].to_string();
                let name = parts[3].to_string();
                let peer_port = announcement.port;
                let available_memory_mb: u64 = parts[5].parse().unwrap_or(0);
                if peer_id == my_id {
                    continue;
                }

                let std::net::IpAddr::V4(source_ip) = addr.ip() else {
                    continue;
                };
                if !discovery_packet_allowed(&snapshot, source_ip, ingress_index, &fixed_peer_ips) {
                    continue;
                }
                if !fixed_peer_resolver.identity_allowed(source_ip, &peer_id) {
                    eprintln!(
                        "[UDP][security] ignored fixed-address announcement from {source_ip}: device identity did not match"
                    );
                    continue;
                }
                let now = Instant::now();
                if !deduper.should_accept(&peer_id, source_ip, &msg, now) {
                    metrics.record_receive(true, false);
                    if let Some(report) = metrics.report_if_due(
                        now,
                        TOTAL_DISCOVERY_DATAGRAM_BUDGET,
                        &excluded_interfaces(&snapshot),
                    ) {
                        println!("[UDP][discovery.metrics] {report}");
                    }
                    continue;
                }
                metrics.record_receive(false, false);

                let observed_peer_addr = SocketAddr::new(addr.ip(), peer_port).to_string();
                let selected = select_peer_endpoint(
                    &peer_id,
                    addr.ip(),
                    peer_port,
                    &fixed_peer_resolver.verified_endpoints_by_device_id,
                );
                let using_verified_override = selected.source == PeerEndpointSource::VerifiedFixed
                    && selected.endpoint != observed_peer_addr;
                let endpoint_changed = using_verified_override
                    && peer_manager
                        .get_active_peers()
                        .into_iter()
                        .find(|peer| peer.id == peer_id)
                        .is_none_or(|peer| peer.addr != selected.endpoint);
                let peer_addr = selected.endpoint;

                let is_new_or_reconnected = peer_manager.add_or_update_with_details(
                    peer_id.clone(),
                    name.clone(),
                    peer_addr.clone(),
                    available_memory_mb,
                    announcement.hostname.clone(),
                    announcement.mac_address.clone(),
                    Some("lan".to_string()),
                    announcement.capabilities.clone(),
                    announcement.app_version.clone(),
                    announcement.has_authoritative_metadata(),
                );

                if using_verified_override && (is_new_or_reconnected || endpoint_changed) {
                    eprintln!(
                        "[UDP][endpoint] peer {peer_id}: observed {observed_peer_addr}, using verified {peer_addr}"
                    );
                }

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
                    announcement.app_version.as_deref(),
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
                            "protocol_version": announcement.protocol_version,
                            "app_version": announcement.app_version
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
                    let target = SocketAddr::new(addr.ip(), peer_port);
                    if send_discovery_reply(
                        &snapshot,
                        &fixed_peer_ips,
                        source_ip,
                        ingress_index,
                        target,
                        reply.as_bytes(),
                    ) {
                        metrics.record_reply();
                    }
                }
                if let Some(report) = metrics.report_if_due(
                    Instant::now(),
                    TOTAL_DISCOVERY_DATAGRAM_BUDGET,
                    &excluded_interfaces(&snapshot),
                ) {
                    println!("[UDP][discovery.metrics] {report}");
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
    let socket = match create_listener_socket(&bind_addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[UDP] Web端创建监听 socket 失败: {}", e);
            return;
        }
    };

    let mut buf = [0u8; 1024];
    let (hostname, mac_address) = local_device_metadata();
    let mut deduper = ReplyDeduper::new(Duration::from_secs(2));
    let mut metrics = DiscoveryMetricsWindow::new(Instant::now());
    let mut snapshot = fail_closed_network_snapshot();
    let mut joined = BTreeSet::new();
    let mut fixed_peer_resolver = FixedPeerSourceResolver::default();
    let mut fixed_peer_ips = HashSet::new();
    refresh_listener_policy(
        &socket,
        &pool,
        port,
        &mut snapshot,
        &mut joined,
        &mut fixed_peer_resolver,
        &mut fixed_peer_ips,
    )
    .await;
    let mut next_policy_refresh = tokio::time::Instant::now() + INTERFACE_POLL_INTERVAL;
    loop {
        let packet = tokio::select! {
            result = recv_discovery_packet(&socket, &mut buf) => match result {
                Ok(packet) => packet,
                Err(error) => {
                    eprintln!("[UDP][discovery.listener] receive failed: {error}");
                    continue;
                }
            },
            _ = tokio::time::sleep_until(next_policy_refresh) => {
                refresh_listener_policy(
                    &socket, &pool, port, &mut snapshot, &mut joined,
                    &mut fixed_peer_resolver, &mut fixed_peer_ips,
                ).await;
                next_policy_refresh = tokio::time::Instant::now() + INTERFACE_POLL_INTERVAL;
                continue;
            },
            _ = discovery_policy::wait_for_settings_change() => {
                refresh_listener_policy(
                    &socket, &pool, port, &mut snapshot, &mut joined,
                    &mut fixed_peer_resolver, &mut fixed_peer_ips,
                ).await;
                next_policy_refresh = tokio::time::Instant::now() + INTERFACE_POLL_INTERVAL;
                continue;
            },
        };
        let size = packet.size;
        let addr = packet.source;
        let ingress_index = packet.ingress_index;
        {
            let msg = String::from_utf8_lossy(&buf[..size]);
            let parts: Vec<&str> = msg.split('|').collect();

            if parts.len() >= 6 && parts[0] == "LANChat" {
                let announcement = match DiscoveryAnnouncement::parse(&msg) {
                    Ok(Some(announcement)) => announcement,
                    _ => continue,
                };
                let peer_id = parts[2].to_string();
                let name = parts[3].to_string();
                let peer_port = announcement.port;
                let available_memory_mb: u64 = parts[5].parse().unwrap_or(0);
                if peer_id == my_id {
                    continue;
                }
                let std::net::IpAddr::V4(source_ip) = addr.ip() else {
                    continue;
                };
                if !discovery_packet_allowed(&snapshot, source_ip, ingress_index, &fixed_peer_ips) {
                    continue;
                }
                if !fixed_peer_resolver.identity_allowed(source_ip, &peer_id) {
                    eprintln!(
                        "[UDP][security] ignored fixed-address announcement from {source_ip}: device identity did not match"
                    );
                    continue;
                }
                let now = Instant::now();
                if !deduper.should_accept(&peer_id, source_ip, &msg, now) {
                    metrics.record_receive(true, false);
                    if let Some(report) = metrics.report_if_due(
                        now,
                        TOTAL_DISCOVERY_DATAGRAM_BUDGET,
                        &excluded_interfaces(&snapshot),
                    ) {
                        println!("[UDP][discovery.metrics] {report}");
                    }
                    continue;
                }
                metrics.record_receive(false, false);
                let observed_peer_addr = SocketAddr::new(addr.ip(), peer_port).to_string();
                let selected = select_peer_endpoint(
                    &peer_id,
                    addr.ip(),
                    peer_port,
                    &fixed_peer_resolver.verified_endpoints_by_device_id,
                );
                let using_verified_override = selected.source == PeerEndpointSource::VerifiedFixed
                    && selected.endpoint != observed_peer_addr;
                let endpoint_changed = using_verified_override
                    && peer_manager
                        .get_active_peers()
                        .into_iter()
                        .find(|peer| peer.id == peer_id)
                        .is_none_or(|peer| peer.addr != selected.endpoint);
                let peer_addr = selected.endpoint;

                let is_new_or_reconnected = peer_manager.add_or_update_with_details(
                    peer_id.clone(),
                    name.clone(),
                    peer_addr.clone(),
                    available_memory_mb,
                    announcement.hostname.clone(),
                    announcement.mac_address.clone(),
                    Some("lan".to_string()),
                    announcement.capabilities.clone(),
                    announcement.app_version.clone(),
                    announcement.has_authoritative_metadata(),
                );

                if using_verified_override && (is_new_or_reconnected || endpoint_changed) {
                    eprintln!(
                        "[UDP][endpoint] peer {peer_id}: observed {observed_peer_addr}, using verified {peer_addr}"
                    );
                }

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
                    announcement.app_version.as_deref(),
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
                    let target = SocketAddr::new(addr.ip(), peer_port);
                    if send_discovery_reply(
                        &snapshot,
                        &fixed_peer_ips,
                        source_ip,
                        ingress_index,
                        target,
                        reply.as_bytes(),
                    ) {
                        metrics.record_reply();
                    }
                }
                if let Some(report) = metrics.report_if_due(
                    Instant::now(),
                    TOTAL_DISCOVERY_DATAGRAM_BUDGET,
                    &excluded_interfaces(&snapshot),
                ) {
                    println!("[UDP][discovery.metrics] {report}");
                }
            }
        }
    }
}

/// 离线看门狗的扫描间隔。原先离线判定只在前端拉快照时顺带发生，
/// 没人拉就永远不翻转，于是既没有离线通知、也不会触发上线补发。
/// A0 兼容超时是 75 秒；2 秒扫描让实际翻转最多再增加约 2 秒。
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
    let snapshot = read_default_network_snapshot();
    let plan = discovery_policy::build_send_plan(&snapshot, port);
    let mut metrics = DiscoveryMetricsWindow::new(Instant::now());
    send_interface_announcements(&plan, &msg, &mut metrics);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_peer_source_requires_the_bound_device_identity() {
        let fixed_source = Ipv4Addr::new(192, 168, 10, 22);
        let ordinary_lan_source = Ipv4Addr::new(192, 168, 10, 111);
        let expected_ids =
            HashMap::from([(fixed_source, HashSet::from(["device-zhangsan".to_string()]))]);

        assert!(fixed_peer_identity_allowed(
            &expected_ids,
            fixed_source,
            "device-zhangsan"
        ));
        assert!(!fixed_peer_identity_allowed(
            &expected_ids,
            fixed_source,
            "device-lisi"
        ));
        assert!(fixed_peer_identity_allowed(
            &expected_ids,
            ordinary_lan_source,
            "device-lisi"
        ));
    }

    #[test]
    fn peer_endpoint_prefers_verified_endpoint_over_nat_source() {
        let verified_endpoints = HashMap::from([(
            "peer-20".to_string(),
            "192.168.20.105:8888".to_string(),
        )]);

        let selected = select_peer_endpoint(
            "peer-20",
            "192.168.10.120".parse().unwrap(),
            8888,
            &verified_endpoints,
        );

        assert_eq!(selected.endpoint, "192.168.20.105:8888");
        assert_eq!(selected.source, PeerEndpointSource::VerifiedFixed);
    }

    #[test]
    fn peer_endpoint_uses_observed_source_without_verified_endpoint() {
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
    fn peer_endpoint_does_not_reuse_another_devices_verified_endpoint() {
        let verified_endpoints = HashMap::from([(
            "peer-a".to_string(),
            "192.168.20.105:8888".to_string(),
        )]);

        let selected = select_peer_endpoint(
            "peer-b",
            "192.168.20.106".parse().unwrap(),
            8888,
            &verified_endpoints,
        );

        assert_eq!(selected.endpoint, "192.168.20.106:8888");
        assert_eq!(selected.source, PeerEndpointSource::ObservedUdp);
    }

    #[test]
    fn peer_endpoint_preserves_verified_port() {
        let verified_endpoints = HashMap::from([(
            "peer-20".to_string(),
            "192.168.20.105:18888".to_string(),
        )]);

        let selected = select_peer_endpoint(
            "peer-20",
            "192.168.10.120".parse().unwrap(),
            8888,
            &verified_endpoints,
        );

        assert_eq!(selected.endpoint, "192.168.20.105:18888");
    }

    #[test]
    fn peer_endpoint_falls_back_after_verified_record_is_removed() {
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

    #[test]
    fn windows_packet_info_reports_the_ingress_interface() {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestControlHeader {
            length: usize,
            level: i32,
            kind: i32,
        }

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct TestPacketInfo {
            address: u32,
            interface_index: u32,
        }

        #[repr(align(8))]
        struct AlignedControl([u8; 128]);

        let mut control = AlignedControl([0; 128]);
        let data_offset =
            (std::mem::size_of::<TestControlHeader>() + std::mem::align_of::<usize>() - 1)
                & !(std::mem::align_of::<usize>() - 1);
        let message_length = data_offset + std::mem::size_of::<TestPacketInfo>();
        unsafe {
            control
                .0
                .as_mut_ptr()
                .cast::<TestControlHeader>()
                .write_unaligned(TestControlHeader {
                    length: message_length,
                    level: 0,
                    kind: 19,
                });
            control
                .0
                .as_mut_ptr()
                .add(data_offset)
                .cast::<TestPacketInfo>()
                .write_unaligned(TestPacketInfo {
                    address: 0,
                    interface_index: 27,
                });
        }

        assert_eq!(
            parse_windows_pktinfo_control(&control.0[..message_length]),
            Some(27),
        );
        assert_eq!(
            parse_windows_pktinfo_control(
                &control.0[..std::mem::size_of::<TestControlHeader>() - 1]
            ),
            None,
        );

        control.0[12..16].copy_from_slice(&20_i32.to_ne_bytes());
        assert_eq!(
            parse_windows_pktinfo_control(&control.0[..message_length]),
            None,
        );
    }

    fn listener_test_snapshot() -> DiscoveryNetworkSnapshot {
        DiscoveryNetworkSnapshot {
            settings: DiscoverySettings::default(),
            interfaces: vec![
                discovery_policy::NetworkInterfaceView {
                    id: "if:name:en0".into(),
                    name: "en0".into(),
                    index: Some(14),
                    addresses: vec![discovery_policy::NetworkInterfaceAddress {
                        ipv4: "192.168.10.152".into(),
                        prefix_length: Some(23),
                    }],
                    category: discovery_policy::InterfaceCategory::PhysicalLan,
                    is_up: true,
                    default_enabled: true,
                    selected: true,
                    enabled: true,
                    exclusion_reason: None,
                },
                discovery_policy::NetworkInterfaceView {
                    id: "if:name:utun4".into(),
                    name: "utun4".into(),
                    index: Some(22),
                    addresses: vec![discovery_policy::NetworkInterfaceAddress {
                        ipv4: "198.18.0.1".into(),
                        prefix_length: Some(30),
                    }],
                    category: discovery_policy::InterfaceCategory::ProxyTun,
                    is_up: true,
                    default_enabled: false,
                    selected: false,
                    enabled: false,
                    exclusion_reason: Some("proxy_tun_default_excluded".into()),
                },
            ],
        }
    }

    #[test]
    fn discovery_cadence_bursts_then_moves_to_jittered_steady_state() {
        let mut cadence = DiscoveryCadence::default();
        let first_gap = cadence.delay_after_send(500);
        let second_gap = cadence.delay_after_send(500);
        let steady_gap = cadence.delay_after_send(500);

        assert_eq!(first_gap, Duration::from_millis(400));
        assert_eq!(first_gap + second_gap, Duration::from_millis(1500));
        assert_eq!(steady_gap, Duration::from_secs(30));
        assert_eq!(steady_discovery_delay(0), Duration::from_secs(24));
        assert_eq!(steady_discovery_delay(1000), Duration::from_secs(36));

        cadence.reset_burst();
        assert_eq!(cadence.delay_after_send(500), Duration::from_millis(400),);
    }

    #[test]
    fn discovery_reply_dedupe_only_suppresses_the_same_recent_frame() {
        let start = std::time::Instant::now();
        let mut deduper = ReplyDeduper::new(Duration::from_secs(2));
        let source = Ipv4Addr::new(192, 168, 10, 22);

        assert!(deduper.should_accept("peer-a", source, "frame-a", start));
        assert!(!deduper.should_accept(
            "peer-a",
            source,
            "frame-a",
            start + Duration::from_millis(500),
        ));
        assert!(deduper.should_accept(
            "peer-a",
            source,
            "frame-b",
            start + Duration::from_millis(500),
        ));
        assert!(deduper.should_accept(
            "peer-a",
            Ipv4Addr::new(10, 8, 0, 2),
            "frame-a",
            start + Duration::from_millis(500),
        ));
        assert!(deduper.should_accept(
            "peer-a",
            source,
            "frame-a",
            start + Duration::from_millis(2100),
        ));
    }

    #[test]
    fn listener_membership_ingress_and_reply_source_follow_enabled_interfaces() {
        let snapshot = listener_test_snapshot();
        let memberships = listener_multicast_memberships(&snapshot);
        assert_eq!(
            memberships,
            std::collections::BTreeSet::from([Ipv4Addr::new(192, 168, 10, 152)])
        );
        assert_eq!(enabled_local_ips(&snapshot), vec!["192.168.10.152"]);

        let fixed = std::collections::HashSet::from([Ipv4Addr::new(203, 0, 113, 8)]);
        let lan_peer = Ipv4Addr::new(192, 168, 11, 20);
        let tun_peer = Ipv4Addr::new(198, 18, 0, 2);
        let fixed_tun_peer = std::collections::HashSet::from([tun_peer]);

        assert!(discovery_packet_allowed(
            &snapshot,
            lan_peer,
            Some(14),
            &fixed
        ));
        assert!(!discovery_packet_allowed(
            &snapshot,
            tun_peer,
            Some(22),
            &fixed
        ));
        assert!(discovery_packet_allowed_with_unmapped_ingress(
            &snapshot,
            tun_peer,
            Some(22),
            &fixed_tun_peer,
            false,
        ));
        assert!(discovery_packet_allowed(&snapshot, lan_peer, None, &fixed));
        assert!(!discovery_packet_allowed(&snapshot, tun_peer, None, &fixed));
        assert!(discovery_packet_allowed(
            &snapshot,
            Ipv4Addr::new(203, 0, 113, 8),
            None,
            &fixed,
        ));
        assert!(!discovery_packet_allowed_with_unmapped_ingress(
            &snapshot,
            lan_peer,
            Some(999),
            &fixed,
            false,
        ));
        assert!(discovery_packet_allowed_with_unmapped_ingress(
            &snapshot,
            lan_peer,
            Some(999),
            &fixed,
            true,
        ));
        assert_eq!(
            reply_source_ip(&snapshot, lan_peer, Some(14)),
            Some(Ipv4Addr::new(192, 168, 10, 152)),
        );
        assert_eq!(reply_source_ip(&snapshot, tun_peer, Some(22)), None);
        assert_eq!(
            reply_source_ip_for_packet(&snapshot, tun_peer, Some(22), true),
            Some(Ipv4Addr::new(198, 18, 0, 1)),
        );
        assert_eq!(
            reply_source_ip_for_packet(&snapshot, tun_peer, Some(22), false),
            None,
        );
        assert_eq!(
            reply_source_ip_with_unmapped_ingress(&snapshot, lan_peer, Some(999), true),
            Some(Ipv4Addr::new(192, 168, 10, 152)),
        );
        assert_eq!(
            reply_source_ip_with_unmapped_ingress(&snapshot, tun_peer, Some(22), true),
            None,
        );

        let mut prefixless = snapshot.clone();
        prefixless.interfaces[0].addresses[0].prefix_length = None;
        assert_eq!(
            reply_source_ip_with_unmapped_ingress(&prefixless, lan_peer, None, true),
            Some(Ipv4Addr::new(192, 168, 10, 152)),
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn listener_reads_the_kernel_ingress_interface_index() {
        let listener = create_listener_socket("127.0.0.1:0").unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender
            .send_to(b"probe", listener.local_addr().unwrap())
            .unwrap();

        let mut buffer = [0u8; 16];
        let packet = bounded_resolution(
            Duration::from_secs(1),
            recv_discovery_packet(&listener, &mut buffer),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(&buffer[..packet.size], b"probe");
        assert_eq!(
            packet.source.ip(),
            "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
        );
        assert!(packet.ingress_index.is_some());
    }

    #[test]
    fn discovery_fixed_peer_failures_back_off_without_blocking_other_peers() {
        let start = std::time::Instant::now();
        let mut failing = FixedPeerRetryState::new(start);
        let untouched = FixedPeerRetryState::new(start);

        assert!(failing.can_attempt(start));
        failing.record_failure(start);
        assert!(!failing.can_attempt(start + Duration::from_secs(4)));
        assert!(failing.can_attempt(start + Duration::from_secs(5)));
        assert!(untouched.can_attempt(start + Duration::from_secs(1)));

        failing.record_failure(start + Duration::from_secs(5));
        assert!(!failing.can_attempt(start + Duration::from_secs(14)));
        assert!(failing.can_attempt(start + Duration::from_secs(15)));

        failing.record_success(start + Duration::from_secs(15));
        assert!(!failing.can_attempt(start + Duration::from_secs(44)));
        assert!(failing.can_attempt(start + Duration::from_secs(45)));

        failing.reset(start + Duration::from_secs(20));
        assert!(failing.can_attempt(start + Duration::from_secs(20)));
        assert_eq!(failing.consecutive_failures, 0);
    }

    #[test]
    fn fixed_peer_batches_rotate_without_exceeding_the_cycle_budget() {
        let now = Instant::now();
        let peers = (0..40)
            .map(|index| format!("peer-{index:02}.example"))
            .collect::<Vec<_>>();
        let mut retry_states = HashMap::new();
        let mut cursor = 0;

        let first = rotating_fixed_peer_endpoints(&peers, &mut retry_states, &mut cursor, now);
        let second = rotating_fixed_peer_endpoints(&peers, &mut retry_states, &mut cursor, now);

        assert_eq!(first.len(), MAX_FIXED_PEER_DATAGRAMS_PER_CYCLE);
        assert_eq!(second.len(), MAX_FIXED_PEER_DATAGRAMS_PER_CYCLE);
        assert_eq!(first[0], "peer-00.example");
        assert_eq!(second[0], "peer-16.example");
    }

    #[tokio::test]
    async fn listener_fixed_peer_resolution_keeps_last_good_address_on_failure() {
        let endpoint = "invalid\0host".to_string();
        let expected = Ipv4Addr::new(100, 64, 0, 8);
        let mut resolver = FixedPeerSourceResolver::default();
        resolver
            .resolved_by_endpoint
            .insert(endpoint.clone(), HashSet::from([expected]));

        let sources = resolver.refresh(&[endpoint], 8888).await;

        assert_eq!(sources, HashSet::from([expected]));
    }

    #[test]
    fn listener_fixed_peer_resolution_drops_stale_addresses_after_network_change() {
        let now = Instant::now();
        let endpoint = "mesh.example".to_string();
        let mut resolver = FixedPeerSourceResolver::default();
        resolver.resolved_by_endpoint.insert(
            endpoint.clone(),
            HashSet::from([Ipv4Addr::new(100, 64, 0, 8)]),
        );
        resolver.retry_states.insert(
            endpoint,
            FixedPeerRetryState {
                consecutive_failures: 3,
                next_attempt: now + Duration::from_secs(60),
            },
        );

        resolver.reset_after_network_change(now);

        assert!(resolver.resolved_by_endpoint.is_empty());
        assert!(resolver
            .retry_states
            .values()
            .all(|state| state.can_attempt(now)));
    }

    #[tokio::test]
    async fn fixed_peer_resolution_has_a_bounded_timeout() {
        assert_eq!(
            bounded_resolution(Duration::from_millis(20), async { 7 })
                .await
                .unwrap(),
            7,
        );
        let error = bounded_resolution(Duration::from_millis(1), async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            9
        })
        .await
        .unwrap_err();
        assert_eq!(error, "resolution timed out");
    }

    #[test]
    fn discovery_runtime_wake_delay_only_tracks_discovery_and_policy_deadlines() {
        let start = Instant::now();
        assert_eq!(
            discovery_runtime_wake_delay(
                start,
                start + Duration::from_secs(30),
                start + INTERFACE_POLL_INTERVAL,
            ),
            INTERFACE_POLL_INTERVAL,
        );
        assert_eq!(
            discovery_runtime_wake_delay(
                start,
                start + Duration::from_secs(3),
                start + INTERFACE_POLL_INTERVAL,
            ),
            Duration::from_secs(3),
        );
    }

    #[test]
    fn discovery_metrics_report_interface_targets_and_receive_deduplication() {
        let start = std::time::Instant::now();
        let mut metrics = DiscoveryMetricsWindow::new(start);
        metrics.record_send("if:7", "broadcast", true);
        metrics.record_send("if:7", "multicast", false);
        metrics.record_receive(false, true);
        metrics.record_receive(true, false);

        assert!(metrics
            .report_if_due(start + Duration::from_secs(59), 48, &[])
            .is_none());
        let report = metrics
            .report_if_due(
                start + Duration::from_secs(60),
                48,
                &[(
                    "Meta Tunnel".to_string(),
                    "proxy_tun_default_excluded".to_string(),
                )],
            )
            .unwrap();

        assert_eq!(report["send_budget"], 48);
        assert_eq!(report["send"]["if:7"]["broadcast"]["attempts"], 1);
        assert_eq!(report["send"]["if:7"]["broadcast"]["success"], 1);
        assert_eq!(report["send"]["if:7"]["multicast"]["failure"], 1);
        assert_eq!(report["receive"]["announcements"], 2);
        assert_eq!(report["receive"]["deduplicated"], 1);
        assert_eq!(report["receive"]["replies"], 1);
        assert_eq!(report["excluded_interfaces"][0]["name"], "Meta Tunnel");
    }

    #[test]
    fn snapshot_read_failure_keeps_last_good_policy_or_fails_closed() {
        let previous = DiscoveryNetworkSnapshot {
            settings: DiscoverySettings {
                local_discovery: false,
                vpn_discovery: true,
                interface_overrides: std::collections::BTreeMap::new(),
            },
            interfaces: Vec::new(),
        };

        assert_eq!(
            snapshot_after_read(Some(&previous), Err("database unavailable".into())),
            previous,
        );

        let initial = snapshot_after_read(None, Err("database unavailable".into()));
        assert!(!initial.settings.local_discovery);
        assert!(!initial.settings.vpn_discovery);
        assert!(initial.interfaces.is_empty());
    }

    #[test]
    fn offline_is_noticed_within_a_couple_seconds_of_the_timeout() {
        // 实际感知延迟 = 离线超时 + 扫描间隔；扫描粒度必须显著小于兼容超时。
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
        assert!(DISCOVERY_CAPABILITIES.contains(&"parallel_file_v3:16"));
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
    fn local_ip_address_never_returns_loopback_or_unspecified() {
        if let Some(address) = local_ip_address() {
            let address: std::net::IpAddr = address.parse().unwrap();
            assert!(!address.is_loopback());
            assert!(!address.is_unspecified());
        }
    }

    #[test]
    fn all_off_snapshot_has_no_display_or_cached_ip_fallback() {
        let mut snapshot = listener_test_snapshot();
        for interface in &mut snapshot.interfaces {
            interface.enabled = false;
        }

        assert_eq!(local_ip_for_snapshot(&snapshot), None);
        assert_eq!(cached_ip_list(&Some(Vec::new())), Some(Vec::new()));
    }
}
