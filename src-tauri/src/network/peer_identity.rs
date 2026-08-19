use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

const PEER_IDENTITY_CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
const PEER_IDENTITY_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerIdentity {
    pub device_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerIdentityTestResult {
    pub endpoint: String,
    pub address: String,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_device_id: Option<String>,
    pub identity_matches: bool,
    pub identity: PeerIdentity,
}

fn validate_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
}

pub fn normalize_peer_endpoint(input: &str, default_port: u16) -> Result<String, String> {
    if default_port == 0 {
        return Err("服务端口无效".to_string());
    }
    let endpoint = input.trim();
    if endpoint.is_empty() {
        return Err("请输入设备 IP 或主机名".to_string());
    }
    if endpoint.len() > 320
        || endpoint.contains("//")
        || endpoint
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '/' | '?' | '#'))
    {
        return Err("地址格式无效，请输入 IP、主机名或带端口的地址".to_string());
    }
    if let Ok(address) = endpoint.parse::<SocketAddr>() {
        if address.port() == 0 {
            return Err("服务端口无效".to_string());
        }
        return Ok(address.to_string());
    }
    if let Ok(address) = endpoint.parse::<IpAddr>() {
        return Ok(SocketAddr::new(address, default_port).to_string());
    }

    if let Some((host, port)) = endpoint.rsplit_once(':') {
        if host.contains(':') || !validate_hostname(host) {
            return Err("主机名格式无效".to_string());
        }
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| "服务端口无效".to_string())?;
        return Ok(format!("{host}:{port}"));
    }
    if validate_hostname(endpoint) {
        Ok(format!("{endpoint}:{default_port}"))
    } else {
        Err("主机名格式无效".to_string())
    }
}

fn validate_identity(identity: PeerIdentity) -> Result<PeerIdentity, String> {
    if identity.device_id.trim().is_empty() || identity.device_id.len() > 128 {
        return Err("对方返回的设备 ID 无效".to_string());
    }
    if identity.name.trim().is_empty() || identity.name.chars().count() > 128 {
        return Err("对方返回的设备名称无效".to_string());
    }
    Ok(identity)
}

pub async fn probe_peer_identity(
    endpoint: &str,
    default_port: u16,
    expected_device_id: Option<&str>,
) -> Result<PeerIdentityTestResult, String> {
    let endpoint = normalize_peer_endpoint(endpoint, default_port)?;
    let client = reqwest::Client::builder()
        .connect_timeout(PEER_IDENTITY_CONNECT_TIMEOUT)
        .timeout(PEER_IDENTITY_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("无法创建连接测试: {error}"))?;
    let url = format!("http://{endpoint}/api/peer_identity");
    let started = Instant::now();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("无法连接到该地址: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("对方拒绝身份测试 ({})", response.status()));
    }
    let identity = response
        .json::<PeerIdentity>()
        .await
        .map_err(|error| format!("对方返回的身份信息无效: {error}"))?;
    let identity = validate_identity(identity)?;
    let expected_device_id = expected_device_id
        .map(str::trim)
        .filter(|device_id| !device_id.is_empty())
        .map(str::to_string);
    let identity_matches = expected_device_id
        .as_deref()
        .is_none_or(|expected| expected == identity.device_id);

    Ok(PeerIdentityTestResult {
        address: endpoint.clone(),
        endpoint,
        latency_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        expected_device_id,
        identity_matches,
        identity,
    })
}

pub async fn require_peer_identity(
    endpoint: &str,
    expected_device_id: &str,
) -> Result<PeerIdentity, String> {
    let result = probe_peer_identity(endpoint, 8888, Some(expected_device_id)).await?;
    if !result.identity_matches {
        return Err(format!(
            "设备身份不匹配：期望 {}，实际 {}，未发送任何内容",
            expected_device_id, result.identity.device_id
        ));
    }
    Ok(result.identity)
}

fn default_service_port(pool: &Pool<Sqlite>) -> impl std::future::Future<Output = u16> + '_ {
    async move {
        crate::db::get_port(pool)
            .await
            .or_else(crate::config_file::get_port_from_config)
            .unwrap_or(8888)
    }
}

pub async fn test_custom_peer(
    pool: &Pool<Sqlite>,
    endpoint: &str,
    expected_device_id: Option<&str>,
) -> Result<PeerIdentityTestResult, String> {
    probe_peer_identity(
        endpoint,
        default_service_port(pool).await,
        expected_device_id,
    )
    .await
}

pub async fn verify_and_save_custom_peer(
    pool: &Pool<Sqlite>,
    endpoint: &str,
    expected_device_id: &str,
) -> Result<crate::db::CustomPeerRecord, String> {
    let result = test_custom_peer(pool, endpoint, Some(expected_device_id)).await?;
    if !result.identity_matches {
        return Err(format!(
            "设备身份已变化：期望 {}，实际 {}，未保存固定地址",
            expected_device_id, result.identity.device_id
        ));
    }
    let record = crate::db::CustomPeerRecord {
        endpoint: result.endpoint,
        device_id: Some(result.identity.device_id),
        name: Some(result.identity.name),
        hostname: result.identity.hostname,
        mac_address: result.identity.mac_address,
        app_version: result.identity.app_version,
        last_verified_at: Some(chrono::Utc::now().timestamp()),
    };
    crate::db::save_custom_peer_record(pool, &record).await?;
    crate::network::discovery_policy::notify_settings_changed();
    Ok(record)
}

pub async fn local_peer_identity(pool: &Pool<Sqlite>) -> Result<PeerIdentity, String> {
    let (hostname, mac_address) = super::discovery::local_device_metadata();
    Ok(PeerIdentity {
        device_id: crate::db::get_user_id(pool).await?,
        name: crate::db::get_username(pool).await?,
        hostname,
        mac_address,
        app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};

    async fn spawn_identity_server(
        identity: PeerIdentity,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/api/peer_identity",
            get(move || {
                let identity = identity.clone();
                async move { Json(identity) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, task)
    }

    #[tokio::test]
    async fn peer_identity_probe_reports_match_and_blocks_identity_changes() {
        let identity = PeerIdentity {
            device_id: "device-zhangsan".into(),
            name: "张三".into(),
            hostname: Some("zhangsan-pc".into()),
            mac_address: Some("AA:BB:CC:DD:EE:FF".into()),
            app_version: Some("0.1.5".into()),
        };
        let (address, server) = spawn_identity_server(identity.clone()).await;

        let matched = probe_peer_identity(&address, 8888, Some("device-zhangsan"))
            .await
            .unwrap();
        assert_eq!(matched.endpoint, address);
        assert_eq!(matched.identity, identity);
        assert!(matched.identity_matches);

        let changed = probe_peer_identity(&address, 8888, Some("different-device"))
            .await
            .unwrap();
        assert!(!changed.identity_matches);
        assert_eq!(
            changed.expected_device_id.as_deref(),
            Some("different-device")
        );

        let blocked = require_peer_identity(&address, "different-device").await;
        assert!(blocked.is_err());

        server.abort();
    }

    #[test]
    fn custom_peer_endpoint_normalization_is_strict_and_adds_the_service_port() {
        assert_eq!(
            normalize_peer_endpoint(" 192.168.10.22 ", 8888).unwrap(),
            "192.168.10.22:8888",
        );
        assert_eq!(
            normalize_peer_endpoint("zhangsan.local:9000", 8888).unwrap(),
            "zhangsan.local:9000",
        );
        assert!(normalize_peer_endpoint("http://192.168.10.22", 8888).is_err());
        assert!(normalize_peer_endpoint("192.168.10.22/path", 8888).is_err());
        assert!(normalize_peer_endpoint("", 8888).is_err());
    }
}
