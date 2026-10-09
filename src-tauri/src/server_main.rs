use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tokio;

use lanchat::peers::PeerManager;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: Option<u16>, // 改为 Option，优先用 DB 中的值

    #[arg(long)]
    db_path: Option<String>, // 可选的数据库路径
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();

    // Step 2: 若 --db-path 没传，读 config.json
    let db_dir = if let Some(ref p) = args.db_path {
        // CLI 参数优先级最高
        Some(std::path::PathBuf::from(p))
    } else {
        // 读配置文件的 db_path
        let cfg = lanchat::config_file::read_config();
        cfg.db_path.map(|p| lanchat::config_file::resolve_db_dir(&p))
    };

    // Step 3: 打开数据库
    println!("[Server Main] 正在初始化数据库...");
    let pool = lanchat::db::init_db_standalone(db_dir)
        .await
        .expect("数据库初始化失败");

    // 从数据库读取用户名和 ID
    let my_name = lanchat::db::get_username(&pool)
        .await
        .unwrap_or_else(|_| "Web-User".to_string());

    let my_id = lanchat::db::get_user_id(&pool)
        .await
        .expect("无法获取或生成用户 ID");

    println!("[Server Main] 我的用户名: {}", my_name);
    println!("[Server Main] 我的 ID: {}", my_id);

    // Step 4: 若 --port 没传，读配置文件取 port
    let port: u16 = args.port.unwrap_or_else(|| lanchat::config_file::get_port_from_config().unwrap_or(8888));

    // 创建全局用户管理器
    let peer_manager = Arc::new(PeerManager::new());

    // 从数据库加载历史用户
    if let Err(e) = peer_manager.load_from_db(&pool).await {
        eprintln!("[Server Main] 加载历史用户失败: {}", e);
    }

    let runtime = lanchat::network::runtime::NetworkRuntime::start(
        lanchat::network::runtime::RuntimeConfig {
            port, user_id: my_id, username: my_name,
            pool: pool.clone(), peer_manager,
            #[cfg(feature = "desktop")]
            app_handle: None,
        },
    );
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    let stopped_during_startup = tokio::select! {
        signal = &mut shutdown => {
            if let Err(error) = signal {
                eprintln!("[Server Main] shutdown signal failed: {error}");
            }
            true
        }
        ready = runtime.wait_ready(Duration::from_secs(20)) => {
            match ready {
                Ok(_) => println!("[Server Main] ready http://localhost:{port} health=/api/health"),
                Err(error) if runtime.health().state == "stopped" => {
                    eprintln!("[Server Main] {error}");
                }
                Err(error) => eprintln!("[Server Main] {error}; supervisor will keep retrying"),
            }
            runtime.health().state == "stopped"
        }
    };
    if !stopped_during_startup {
        if let Err(error) = shutdown.await {
            eprintln!("[Server Main] shutdown signal failed: {error}");
        }
    }
    let startup_failed = runtime.health().state == "stopped";
    runtime.shutdown().await;
    pool.close().await;
    if startup_failed {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
