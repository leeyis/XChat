//! Bounded diagnostics export only measured, allow-listed fields.
use crate::{db, network::{discovery_policy, peer_identity, runtime::NetworkHealth}, peers::PeerManager};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticRequest { pub peer_id: Option<String>, #[serde(default)] pub include_addresses: bool }

#[derive(Serialize)]
pub struct Check { pub key: &'static str, pub label: &'static str, pub state: &'static str, pub detail: String }
#[derive(Serialize)]
pub struct Report {
    pub version: u8, pub checked_at: i64, pub addresses_included: bool,
    pub peer_id: Option<String>, pub checks: Vec<Check>, pub interfaces: Vec<serde_json::Value>,
    pub endpoint: Option<String>, pub latency_ms: Option<u64>, pub free_bytes: Option<u64>,
}

pub(crate) async fn free_space(path: std::path::PathBuf) -> Result<Option<u64>, String> {
    tokio::task::spawn_blocking(move || {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let normalize = |path: &std::path::Path| {
            #[cfg(windows)]
            { std::path::PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase()) }
            #[cfg(not(windows))]
            { path.to_path_buf() }
        };
        let path = normalize(&path);
        // Match the most specific mount point, including nested volumes.
        disks.list().iter().filter(|disk| path.starts_with(normalize(disk.mount_point())))
            .max_by_key(|disk| disk.mount_point().components().count()).map(|disk| disk.available_space())
    }).await.map_err(|e| e.to_string())
}

pub async fn run(pool: &Pool<Sqlite>, peers: &PeerManager, health: Option<NetworkHealth>, request: DiagnosticRequest) -> Result<Report, String> {
    let settings = discovery_policy::load_settings(pool).await?;
    let snapshot = tokio::task::spawn_blocking(move || discovery_policy::system_network_snapshot(settings)).await.map_err(|e| e.to_string())?;
    let interfaces = snapshot.interfaces.iter().map(|item| serde_json::json!({
        "category": item.category, "enabled": item.enabled, "is_up": item.is_up,
        "name": request.include_addresses.then(|| item.name.clone()),
        "addresses": request.include_addresses.then(|| item.addresses.clone()),
    })).collect();
    let enabled = snapshot.interfaces.iter().filter(|i| i.enabled).count();
    let discovery_enabled = snapshot.settings.local_discovery || snapshot.settings.vpn_discovery;
    let mut report = Report { version: 1, checked_at: chrono::Utc::now().timestamp(),
        addresses_included: request.include_addresses, peer_id: request.peer_id.clone(), checks: vec![],
        interfaces, endpoint: None, latency_ms: None, free_bytes: None };
    report.checks.push(Check { key: "discovery", label: "局域网发现", state: if enabled > 0 { "ok" } else { "warning" },
        detail: if !discovery_enabled { "自动发现已关闭；仍可使用已验证的固定地址".into() }
        else if enabled == 0 { "没有可用的发现网卡，请检查网络设置".into() }
        else { format!("{enabled} 个网卡参与发现；发现状态不等同于消息连接状态") } });
    if let Some(health) = health {
        report.checks.push(Check { key: "service", label: "本机服务", state: if health.http_ready && health.database_ready { "ok" } else { "error" },
            detail: format!("消息服务{} · 数据库{} · 网络监督器{}", if health.http_ready { "就绪" } else { "未就绪" },
                if health.database_ready { "就绪" } else { "未就绪" }, health.state) });
    }
    if let Some(id) = request.peer_id.as_deref() {
        let peer = peers.get_all_peers().into_iter().find(|p| p.id == id).ok_or("所选设备已不存在")?;
        let address = peers.connection_snapshot(id).filter(|s| s.is_connected()).map(|s| s.address).unwrap_or(peer.addr);
        if request.include_addresses { report.endpoint = Some(address.clone()); }
        match peer_identity::probe_peer_identity(&address, 8888, Some(id)).await {
            Ok(result) => {
                report.latency_ms = Some(result.latency_ms);
                report.checks.push(Check { key: "connection", label: "消息服务连通", state: "ok", detail: format!("服务响应耗时 {} ms", result.latency_ms) });
                report.checks.push(Check { key: "identity", label: "设备身份", state: if result.identity_matches { "ok" } else { "error" },
                    detail: if result.identity_matches { "设备身份与联系人一致".into() } else {
                        crate::network::peer_connection::invalidate_peer_connection(peers, id, "设备身份不匹配");
                        "该地址对应其他设备，已阻止发送；请核对固定地址".into()
                    } });
            }
            Err(_) => {
                report.checks.push(Check { key: "connection", label: "消息服务连通", state: "error", detail: "连接测试失败，请检查对方是否在线、服务端口与防火墙设置".into() });
                report.checks.push(Check { key: "identity", label: "设备身份", state: "unknown", detail: "未获得身份响应，尚不能确认设备身份".into() });
            }
        }
    }
    let database_ok = sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(pool).await.is_ok();
    report.checks.push(Check { key: "database", label: "本地数据库", state: if database_ok { "ok" } else { "error" }, detail: if database_ok { "数据库查询正常".into() } else { "数据库暂不可用".into() } });
    let download = std::path::PathBuf::from(db::get_download_path(pool).await?);
    report.free_bytes = free_space(download.clone()).await?;
    let exists = tokio::fs::metadata(download).await.is_ok_and(|m| m.is_dir());
    report.checks.push(Check { key: "storage", label: "附件存储", state: if exists && report.free_bytes.is_some_and(|n| n > 128 * 1024 * 1024) { "ok" } else { "warning" },
        detail: if !exists { "下载目录尚未创建或暂不可访问".into() } else if report.free_bytes.is_none() { "目录可访问，未获取到磁盘剩余空间".into() } else { "已检查下载目录与可用空间".into() } });
    Ok(report)
}
