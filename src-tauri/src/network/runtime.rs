//! One supervised network lifetime shared by desktop, mobile and headless entry points.
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use sqlx::{Pool, Sqlite};
use tokio::{
    sync::{oneshot, watch},
    task::{JoinHandle, JoinSet},
};

use crate::peers::PeerManager;

#[derive(Clone)]
pub struct RuntimeConfig {
    pub port: u16,
    pub user_id: String,
    pub username: String,
    pub pool: Pool<Sqlite>,
    pub peer_manager: Arc<PeerManager>,
    #[cfg(feature = "desktop")]
    pub app_handle: Option<tauri::AppHandle>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkHealth {
    pub port: u16,
    pub state: String,
    pub generation: u64,
    pub http_ready: bool,
    pub discovery_ready: bool,
    pub announcing: bool,
    pub watchdog_running: bool,
    pub database_ready: bool,
    pub eligible_discovery_interfaces: usize,
    pub last_error: Option<String>,
    pub retry_in_seconds: u64,
}

impl NetworkHealth {
    pub(crate) fn starting(port: u16) -> Self {
        Self {
            port,
            state: "starting".into(),
            generation: 0,
            http_ready: false,
            discovery_ready: false,
            announcing: false,
            watchdog_running: false,
            database_ready: false,
            eligible_discovery_interfaces: 0,
            last_error: None,
            retry_in_seconds: 0,
        }
    }

    fn stop_services(&mut self) {
        self.http_ready = false;
        self.discovery_ready = false;
        self.announcing = false;
        self.watchdog_running = false;
    }
}

pub struct NetworkRuntime {
    shutdown: watch::Sender<bool>,
    health: watch::Receiver<NetworkHealth>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl NetworkRuntime {
    pub fn start(config: RuntimeConfig) -> Self {
        let (shutdown, stop) = watch::channel(false);
        let (health, receiver) = watch::channel(NetworkHealth::starting(config.port));
        let task = tokio::spawn(supervise(config, health, stop));
        Self {
            shutdown,
            health: receiver,
            task: Mutex::new(Some(task)),
        }
    }

    pub fn health(&self) -> NetworkHealth {
        self.health.borrow().clone()
    }

    pub async fn wait_ready(&self, timeout: Duration) -> Result<NetworkHealth, String> {
        let mut health = self.health.clone();
        tokio::time::timeout(timeout, async {
            loop {
                let snapshot = health.borrow().clone();
                if snapshot.state == "ready" {
                    return Ok(snapshot);
                }
                if snapshot.state == "stopped" {
                    return Err(snapshot
                        .last_error
                        .unwrap_or_else(|| "network runtime stopped".into()));
                }
                health
                    .changed()
                    .await
                    .map_err(|_| "network supervisor exited".to_string())?;
            }
        })
        .await
        .map_err(|_| format!("network readiness timeout: {:?}", self.health()))?
    }

    pub fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub async fn shutdown(&self) {
        self.request_shutdown();
        let task = self.task.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(mut task) = task {
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

impl Drop for NetworkRuntime {
    fn drop(&mut self) {
        self.request_shutdown();
        if let Some(task) = self.task.lock().unwrap_or_else(|p| p.into_inner()).take() {
            task.abort();
        }
    }
}

type Services = JoinSet<(&'static str, Result<(), String>)>;

async fn stopped(stop: &mut watch::Receiver<bool>) {
    while !*stop.borrow_and_update() {
        if stop.changed().await.is_err() {
            break;
        }
    }
}

fn service_exit(
    result: Option<Result<(&str, Result<(), String>), tokio::task::JoinError>>,
) -> String {
    match result {
        Some(Ok((name, Ok(())))) => format!("{name}: unexpected service exit"),
        Some(Ok((name, Err(error)))) => format!("{name}: {error}"),
        Some(Err(error)) => format!("network service panic or abort: {error}"),
        None => "no network services remain".into(),
    }
}

async fn wait_for_exit(
    services: &mut Services,
    stop: &mut watch::Receiver<bool>,
) -> Option<String> {
    tokio::select! {
        biased;
        _ = stopped(stop) => None,
        result = services.join_next() => Some(service_exit(result)),
    }
}

fn retry_delay(failures: usize) -> Duration {
    Duration::from_secs([1, 2, 5, 10, 30][failures.saturating_sub(1).min(4)])
}

async fn run_generation(
    config: &RuntimeConfig,
    health: &watch::Sender<NetworkHealth>,
    services: &mut Services,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    super::conversation_file::recover_abandoned_uploads(&config.pool)
        .await
        .map_err(|error| format!("database recovery: {error}"))?;
    health.send_modify(|h| h.database_ready = true);
    let tcp = tokio::net::TcpListener::bind(("0.0.0.0", config.port))
        .await
        .map_err(|e| format!("http bind port={}: {e}", config.port))?;
    let udp = super::discovery::create_listener_socket(&format!("0.0.0.0:{}", config.port))
        .map_err(|e| format!("discovery bind port={}: {e}", config.port))?;
    let (http_ready, http_wait) = oneshot::channel();
    let (udp_ready, udp_wait) = oneshot::channel();
    let http_config = config.clone();
    let http_health = health.subscribe();
    services.spawn(async move {
        (
            "http",
            crate::web_server::serve_listener(
                tcp,
                http_config.port,
                http_config.pool,
                http_config.peer_manager,
                http_health,
                http_ready,
                #[cfg(feature = "desktop")]
                http_config.app_handle,
            )
            .await,
        )
    });
    let udp_config = config.clone();
    services.spawn(async move {
        (
            "discovery",
            super::discovery::listen_on(
                udp,
                udp_config.port,
                udp_config.user_id,
                udp_config.username,
                #[cfg(feature = "desktop")]
                udp_config.app_handle,
                udp_config.peer_manager,
                udp_config.pool,
                udp_ready,
            )
            .await,
        )
    });
    let readiness = async {
        http_wait
            .await
            .map_err(|_| "http readiness channel closed".to_string())?;
        udp_wait
            .await
            .map_err(|_| "discovery readiness channel closed".to_string())
    };
    tokio::select! {
        _ = stopped(stop) => return Ok(()),
        result = services.join_next() => return Err(service_exit(result)),
        result = tokio::time::timeout(Duration::from_secs(15), readiness) => {
            result.map_err(|_| "listener readiness timeout".to_string())??;
        }
    }
    health.send_modify(|h| {
        h.http_ready = true;
        h.discovery_ready = true;
    });

    // No advertisements until both receiving paths are accepting work.
    let announce_config = config.clone();
    let announce_health = health.clone();
    let (announce_ready, announce_wait) = oneshot::channel();
    services.spawn(async move {
        (
            "announcer",
            super::discovery::start_announcing(
                announce_config.port,
                announce_config.user_id,
                announce_config.pool,
                announce_ready,
                announce_health,
            )
            .await,
        )
    });
    let watchdog_config = config.clone();
    services.spawn(async move {
        super::discovery::start_offline_watchdog(
            watchdog_config.peer_manager,
            watchdog_config.pool,
            #[cfg(feature = "desktop")]
            watchdog_config.app_handle,
        )
        .await;
        ("watchdog", Ok(()))
    });
    let database_pool = config.pool.clone();
    let database_health = health.clone();
    services.spawn(async move {
        (
            "database",
            monitor_database(&database_pool, &database_health).await,
        )
    });
    tokio::select! {
        _ = stopped(stop) => return Ok(()),
        result = services.join_next() => return Err(service_exit(result)),
        result = tokio::time::timeout(Duration::from_secs(15), announce_wait) => {
            result.map_err(|_| "announcer readiness timeout".to_string())?
                .map_err(|_| "announcer readiness channel closed".to_string())?;
        }
    }
    health.send_modify(|h| {
        h.state = "ready".into();
        h.announcing = true;
        h.watchdog_running = true;
        h.retry_in_seconds = 0;
    });
    eprintln!(
        "[NetworkRuntime] ready port={} generation={}",
        config.port,
        health.borrow().generation
    );
    match wait_for_exit(services, stop).await {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn supervise(
    config: RuntimeConfig,
    health: watch::Sender<NetworkHealth>,
    mut stop: watch::Receiver<bool>,
) {
    // TCP and UDP would select unrelated ephemeral ports for zero. Never advertise
    // an unusable port or report ready for this permanent configuration error.
    if config.port == 0 {
        let error = "network port must be between 1 and 65535".to_string();
        eprintln!("[NetworkRuntime] {error}");
        health.send_modify(|h| {
            h.state = "stopped".into();
            h.last_error = Some(error);
        });
        return;
    }
    let mut failures = 0;
    while !*stop.borrow() {
        health.send_modify(|h| {
            h.state = "starting".into();
            h.generation += 1;
            h.retry_in_seconds = 0;
            h.stop_services();
            h.database_ready = false;
        });
        let started = tokio::time::Instant::now();
        let mut services = Services::new();
        let mut generation_stop = stop.clone();
        let result = tokio::select! {
            _ = stopped(&mut stop) => Ok(()),
            result = run_generation(&config, &health, &mut services, &mut generation_stop) => result,
        };
        health.send_modify(|h| {
            h.state = "stopping".into();
            h.stop_services();
        });
        services.shutdown().await;
        if *stop.borrow() {
            break;
        }
        let error = result
            .err()
            .unwrap_or_else(|| "network generation unexpectedly stopped".into());
        if started.elapsed() > Duration::from_secs(60) {
            failures = 0;
        }
        failures += 1;
        let delay = retry_delay(failures);
        eprintln!(
            "[NetworkRuntime] failed port={} generation={} retry_in={}s error={error}",
            config.port,
            health.borrow().generation,
            delay.as_secs()
        );
        health.send_modify(|h| {
            h.state = "backoff".into();
            h.last_error = Some(error);
            h.retry_in_seconds = delay.as_secs();
        });
        tokio::select! { _ = stopped(&mut stop) => break, _ = tokio::time::sleep(delay) => {} }
    }
    health.send_modify(|h| {
        h.state = "stopped".into();
        h.retry_in_seconds = 0;
        h.stop_services();
        h.database_ready = false;
    });
}

async fn monitor_database(
    pool: &Pool<Sqlite>,
    health: &watch::Sender<NetworkHealth>,
) -> Result<(), String> {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query("SELECT value FROM settings WHERE key = 'user_id'").fetch_optional(pool),
        )
        .await;
        let error = match result {
            Ok(Ok(_)) => continue,
            Ok(Err(error)) => format!("database health failed: {error}"),
            Err(_) => "database health timeout".to_string(),
        };
        health.send_modify(|h| h.database_ready = false);
        return Err(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn qa_zero_port_stops_before_starting_services() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_lazy("sqlite::memory:")
            .unwrap();
        let runtime = NetworkRuntime::start(RuntimeConfig {
            port: 0,
            user_id: "self".into(),
            username: "test".into(),
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            #[cfg(feature = "desktop")]
            app_handle: None,
        });
        let error = runtime
            .wait_ready(Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(error.contains("port must be between"), "{error}");
        let health = runtime.health();
        assert_eq!(health.state, "stopped");
        assert_eq!(health.generation, 0);
        assert!(!health.http_ready && !health.discovery_ready && !health.database_ready);
        runtime.shutdown().await;
        pool.close().await;
    }

    #[tokio::test]
    async fn occupied_port_recovers_readiness_and_shutdown_releases_listeners() {
        // TCP ephemeral allocation does not exclude Windows' UDP reserved ranges.
        // Reserve both protocols so this test isolates the deliberate TCP conflict.
        let mut reservation = None;
        for _ in 0..64 {
            let tcp = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
            let address = tcp.local_addr().unwrap();
            match tokio::net::UdpSocket::bind(address).await {
                Ok(udp) => {
                    reservation = Some((tcp, udp));
                    break;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
                    ) => continue,
                Err(error) => panic!("cannot reserve test UDP listener: {error}"),
            }
        }
        let (occupied, udp_reservation) =
            reservation.expect("no port available to both TCP and UDP");
        let port = occupied.local_addr().unwrap().port();
        let app_dir = std::env::temp_dir().join(format!("xchat-runtime-{}", uuid::Uuid::new_v4()));
        let pool = crate::db::init_db_standalone(Some(app_dir.clone()))
            .await
            .unwrap();
        let downloads = app_dir.join("downloads");
        tokio::fs::create_dir_all(&downloads).await.unwrap();
        crate::db::update_download_path(&pool, downloads.to_string_lossy().into_owned())
            .await
            .unwrap();
        crate::db::set_setting(
            &pool,
            "network.discovery.settings.v1",
            r#"{"local_discovery":false,"vpn_discovery":false,"interface_overrides":{}}"#,
        )
        .await
        .unwrap();
        let config = RuntimeConfig {
            port,
            user_id: crate::db::get_user_id(&pool).await.unwrap(),
            username: "runtime test".into(),
            pool: pool.clone(),
            peer_manager: Arc::new(PeerManager::new()),
            #[cfg(feature = "desktop")]
            app_handle: None,
        };
        let runtime = NetworkRuntime::start(config);
        let mut health = runtime.health.clone();
        tokio::time::timeout(Duration::from_secs(3), async {
            while health.borrow().state != "backoff" {
                health.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let failed = runtime.health();
        assert!(!failed.http_ready && !failed.discovery_ready && !failed.announcing);
        assert!(failed.last_error.as_deref().unwrap().contains("http bind"));
        assert_eq!(failed.retry_in_seconds, 1);
        drop(udp_reservation);
        drop(occupied);
        let ready = runtime.wait_ready(Duration::from_secs(8)).await.unwrap();
        assert!(
            ready.http_ready && ready.discovery_ready && ready.announcing && ready.watchdog_running
        );
        assert!(ready.generation >= 2);
        let reported: serde_json::Value =
            reqwest::get(format!("http://127.0.0.1:{port}/api/health"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(reported["state"], "ready");
        assert_eq!(reported["generation"], ready.generation);
        assert_eq!(reported["database_ready"], true);
        let user_id = crate::db::get_user_id(&pool).await.unwrap();
        let (mut socket, _) = tokio_tungstenite::connect_async(format!(
            "ws://127.0.0.1:{port}/ws?target_id={user_id}"
        ))
        .await
        .unwrap();
        // An oversized datagram is a bad packet, not a failed service. In particular,
        // Windows reports WSAEMSGSIZE instead of returning a truncated datagram.
        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sender
            .send_to(&[b'x'; 2048], ("127.0.0.1", port))
            .await
            .unwrap();
        use futures_util::StreamExt;
        // Other concurrent tests may publish normal workspace broadcasts. Only
        // connection closure is a failure; keep consuming unrelated data frames.
        assert!(
            tokio::time::timeout(Duration::from_millis(300), async {
                while let Some(frame) = socket.next().await {
                    if matches!(
                        frame,
                        Err(_) | Ok(tokio_tungstenite::tungstenite::Message::Close(_))
                    ) {
                        return;
                    }
                }
            })
            .await
            .is_err(),
            "a bad UDP packet must not close an established WebSocket"
        );
        assert_eq!(runtime.health().state, "ready");
        assert_eq!(runtime.health().generation, ready.generation);
        runtime.shutdown().await;
        let closed = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap();
        assert!(matches!(
            closed,
            None | Some(Err(_)) | Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)))
        ));
        assert_eq!(runtime.health().state, "stopped");
        assert!(!runtime.health().announcing);
        let _tcp = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let _udp =
            super::super::discovery::create_listener_socket(&format!("0.0.0.0:{port}")).unwrap();
        pool.close().await;
        crate::db::remove_test_database(&pool, &app_dir).await;
    }

    #[tokio::test]
    async fn supervisor_observes_panics_and_stop_interrupts_pending_services() {
        let (shutdown, mut stop) = watch::channel(false);
        let mut services = Services::new();
        services.spawn(async {
            panic!("injected service panic");
            #[allow(unreachable_code)]
            ("broken", Ok(()))
        });
        let error = wait_for_exit(&mut services, &mut stop).await.unwrap();
        assert!(error.contains("panic"));
        services.spawn(async {
            std::future::pending::<()>().await;
            ("stalled", Ok(()))
        });
        shutdown.send(true).unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_exit(&mut services, &mut stop)
        )
        .await
        .unwrap()
        .is_none());
        services.shutdown().await;
        assert!(services.is_empty());
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(100), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn database_failure_is_reported_instead_of_remaining_ready() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        let (health, _) = watch::channel(NetworkHealth::starting(18888));
        health.send_modify(|h| h.database_ready = true);
        pool.close().await;
        let error = tokio::time::timeout(Duration::from_secs(7), monitor_database(&pool, &health))
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.contains("database health failed"));
        assert!(!health.borrow().database_ready);
    }
}
