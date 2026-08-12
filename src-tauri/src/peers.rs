// 在线用户管理模块
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

const PEER_OFFLINE_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub id: String, // UUID
    pub name: String,
    pub addr: String,
    pub last_seen: u64,           // Unix 时间戳
    pub is_offline: bool,         // 是否离线
    pub available_memory_mb: u64, // 首次权威发现时的可用内存快照（MB）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_source: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

// 全局在线用户列表
pub struct PeerManager {
    peers: Arc<RwLock<HashMap<String, Peer>>>, // key 是 UUID
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn remove_peer(&self, id: &str) {
        let mut peers = self.peers.write().unwrap();
        if peers.remove(id).is_some() {
            println!("[PeerManager] 已从内存中彻底移除用户: {}", id);
        }
    }

    // 从数据库加载历史用户
    pub async fn load_from_db(&self, pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<(), String> {
        println!("[PeerManager] 从数据库加载历史用户...");

        let users = crate::db::list_users_with_metadata(pool).await?;

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
            };
            peers.insert(user.id, peer);
        }

        println!("[PeerManager] 已加载 {} 个历史用户", peers.len());
        Ok(())
    }

    // 添加或更新用户
    pub fn add_or_update(&self, id: String, name: String, addr: String) -> bool {
        self.add_or_update_with_memory(id, name, addr, 0)
    }

    // 添加或更新用户（包含内存信息）
    // 返回 true 表示是新用户或重新上线，false 表示只是更新
    pub fn add_or_update_with_memory(
        &self,
        id: String,
        name: String,
        addr: String,
        available_memory_mb: u64,
    ) -> bool {
        self.add_or_update_with_details(
            id,
            name,
            addr,
            available_memory_mb,
            None,
            None,
            Some("lan".to_string()),
            Vec::new(),
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_or_update_with_details(
        &self,
        id: String,
        name: String,
        addr: String,
        available_memory_mb: u64,
        hostname: Option<String>,
        mac_address: Option<String>,
        discovery_source: Option<String>,
        capabilities: Vec<String>,
        authoritative: bool,
    ) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut peers = self.peers.write().unwrap();

        if let Some(peer) = peers.get_mut(&id) {
            // 已存在,更新信息
            let was_offline = peer.is_offline;
            peer.name = name;
            peer.addr = addr;
            peer.last_seen = now;
            peer.is_offline = false;
            if authoritative && peer.available_memory_mb == 0 && available_memory_mb > 0 {
                peer.available_memory_mb = available_memory_mb;
            }
            if authoritative {
                if hostname.is_some() {
                    peer.hostname = hostname;
                }
                if mac_address.is_some() {
                    peer.mac_address = mac_address;
                }
                if discovery_source.is_some() {
                    peer.discovery_source = discovery_source;
                }
                peer.capabilities = capabilities;
            }

            // 只在用户重新上线时打印日志
            if was_offline {
                println!(
                    "[PeerManager] 用户重新上线: {} ({}) - 可用内存: {} MB",
                    peer.name, peer.id, peer.available_memory_mb
                );
                return true; // 重新上线，返回 true
            }
            return false; // 只是更新，返回 false
        } else {
            // 新用户
            let peer = Peer {
                id: id.clone(),
                name: name.clone(),
                addr,
                last_seen: now,
                is_offline: false,
                available_memory_mb: if authoritative {
                    available_memory_mb
                } else {
                    0
                },
                hostname: if authoritative { hostname } else { None },
                mac_address: if authoritative { mac_address } else { None },
                remark: None,
                discovery_source: if authoritative {
                    discovery_source
                } else {
                    None
                },
                capabilities: if authoritative {
                    capabilities
                } else {
                    Vec::new()
                },
            };
            println!(
                "[PeerManager] 添加新用户: {} ({}) - 可用内存: {} MB",
                name, id, available_memory_mb
            );
            peers.insert(id, peer);
            return true; // 新用户，返回 true
        }
    }

    pub fn force_mark_offline(&self, id: &str) {
        let mut peers = self.peers.write().unwrap();
        if let Some(peer) = peers.get_mut(id) {
            if !peer.is_offline {
                println!(
                    "[PeerManager] 探测发现用户确已离线: {} ({})",
                    peer.name, peer.id
                );
                peer.is_offline = true;
            }
        }
    }

    // 标记所有用户为"待确认"状态,然后检查哪些用户离线
    pub fn mark_stale_as_offline(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.mark_stale_as_offline_at(now);
    }

    fn mark_stale_as_offline_at(&self, now: u64) {
        let mut peers = self.peers.write().unwrap();

        // 局域网广播会受休眠、调度和 Wi-Fi 抖动影响；给 2 秒心跳留出足够余量。
        for peer in peers.values_mut() {
            let time_since_seen = now.saturating_sub(peer.last_seen);
            if time_since_seen > PEER_OFFLINE_TIMEOUT_SECS && !peer.is_offline {
                println!(
                    "[PeerManager] 用户离线: {} ({}) - {}秒未见",
                    peer.name, peer.id, time_since_seen
                );
                peer.is_offline = true;
            }
        }
    }

    // 获取所有用户（包括离线的）
    pub fn get_all_peers(&self) -> Vec<Peer> {
        // 先标记离线用户
        self.mark_stale_as_offline();

        let peers = self.peers.read().unwrap();
        peers.values().cloned().collect()
    }

    // 获取所有在线用户（过滤掉离线的）
    pub fn get_active_peers(&self) -> Vec<Peer> {
        let peers = self.peers.read().unwrap();
        peers.values().filter(|p| !p.is_offline).cloned().collect()
    }
}

impl Default for PeerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_do_not_replace_authoritative_discovery_metadata() {
        let manager = PeerManager::new();
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            0,
            Some("reply-host".into()),
            Some("ac:de:48:00:11:22".into()),
            Some("lan".into()),
            vec!["reply-capability".into()],
            false,
        );
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            2356,
            Some("alice-mac".into()),
            Some("82:ae:17:28:c4:04".into()),
            Some("lan".into()),
            vec!["groups_v1".into()],
            true,
        );
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            0,
            Some("reply-host".into()),
            Some("ac:de:48:00:11:22".into()),
            Some("lan".into()),
            vec!["reply-capability".into()],
            false,
        );
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            2367,
            Some("alice-mac".into()),
            Some("82:ae:17:28:c4:04".into()),
            Some("lan".into()),
            vec!["groups_v1".into()],
            true,
        );
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            0,
            None,
            None,
            None,
            Vec::new(),
            false,
        );

        let peer = manager.get_all_peers().pop().unwrap();
        assert_eq!(peer.available_memory_mb, 2356);
        assert_eq!(peer.hostname.as_deref(), Some("alice-mac"));
        assert_eq!(peer.mac_address.as_deref(), Some("82:ae:17:28:c4:04"));
        assert_eq!(peer.capabilities, vec!["groups_v1"]);
    }

    #[test]
    fn authoritative_empty_capabilities_clear_stale_parallel_v2() {
        let manager = PeerManager::new();
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            2356,
            Some("alice-mac".into()),
            Some("82:ae:17:28:c4:04".into()),
            Some("lan".into()),
            vec!["parallel_file_v2".into()],
            true,
        );
        manager.add_or_update_with_details(
            "peer-1".into(),
            "Alice".into(),
            "127.0.0.1:8888".into(),
            2356,
            Some("alice-mac".into()),
            Some("82:ae:17:28:c4:04".into()),
            Some("lan".into()),
            Vec::new(),
            true,
        );

        let peer = manager.get_all_peers().pop().unwrap();
        assert!(peer.capabilities.is_empty());
    }

    #[test]
    fn heartbeat_jitter_does_not_flap_peer_online_state() {
        let manager = PeerManager::new();
        manager.add_or_update("peer-1".into(), "Alice".into(), "127.0.0.1:8888".into());
        {
            let mut peers = manager.peers.write().unwrap();
            peers.get_mut("peer-1").unwrap().last_seen = 1_000;
        }

        manager.mark_stale_as_offline_at(1_017);
        assert!(!manager.peers.read().unwrap()["peer-1"].is_offline);

        manager.mark_stale_as_offline_at(1_030);
        assert!(!manager.peers.read().unwrap()["peer-1"].is_offline);

        manager.mark_stale_as_offline_at(999);
        assert!(!manager.peers.read().unwrap()["peer-1"].is_offline);

        manager.mark_stale_as_offline_at(1_031);
        assert!(manager.peers.read().unwrap()["peer-1"].is_offline);
    }
}
