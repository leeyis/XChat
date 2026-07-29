use crate::utils::{is_legacy_generated_name, machine_name};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, Pool, Sqlite};
use std::path::PathBuf;

#[cfg(feature = "desktop")]
use tauri::AppHandle;
#[cfg(feature = "desktop")]
use tauri::Manager;

pub struct DbState {
    pub pool: Pool<Sqlite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConversationRecord {
    pub id: String,
    pub kind: String,
    pub peer_id: Option<String>,
    pub title: Option<String>,
    pub created_by: Option<String>,
    pub pinned: bool,
    pub forced_unread: bool,
    pub draft: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConversationMemberRecord {
    pub conversation_id: String,
    pub peer_id: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewConversationMember {
    pub peer_id: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageRecord {
    pub id: i64,
    pub sender_id: String,
    pub receiver_id: Option<String>,
    pub content: String,
    pub msg_type: String,
    pub timestamp: i64,
    pub file_path: Option<String>,
    pub file_status: Option<String>,
    pub file_size: Option<i64>,
    pub sender_msg_id: Option<String>,
    pub status: Option<String>,
    pub conversation_id: Option<String>,
    pub client_message_id: Option<String>,
}

pub type FileMessageRecord = MessageRecord;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageReceiptRecord {
    pub message_client_id: String,
    pub reader_id: String,
    pub delivered_at: Option<i64>,
    pub read_at: Option<i64>,
    pub updated_at: i64,
    pub delivery_ack_sent_at: Option<i64>,
    pub read_ack_sent_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct PendingReceiptRecord {
    pub message_client_id: String,
    pub conversation_id: String,
    pub reader_id: String,
    pub delivered_at: Option<i64>,
    pub read_at: Option<i64>,
    pub delivery_ack_sent_at: Option<i64>,
    pub read_ack_sent_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct TransferRecord {
    pub id: String,
    pub message_id: Option<i64>,
    pub conversation_id: String,
    pub peer_id: String,
    pub direction: String,
    pub status: String,
    pub bytes_total: i64,
    pub bytes_transferred: i64,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserRecord {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub last_seen: i64,
    pub is_offline: bool,
    pub available_memory_mb: i64,
    pub hostname: Option<String>,
    pub mac_address: Option<String>,
    pub remark: Option<String>,
    pub discovery_source: Option<String>,
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn stable_direct_conversation_id(left_id: &str, right_id: &str) -> String {
    let (first, second) = if left_id <= right_id {
        (left_id, right_id)
    } else {
        (right_id, left_id)
    };
    format!("direct:{}:{}", first, second)
}

pub async fn get_username(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<String, String> {
    let res: (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'username'")
        .fetch_one(pool)
        .await
        .map_err(|e| {
            eprintln!("[DB] read username failed: {}", e);
            e.to_string()
        })?;
    Ok(res.0)
}

pub async fn get_user_id(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<String, String> {
    let res: Result<(String,), _> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'user_id'")
            .fetch_one(pool)
            .await;

    match res {
        Ok((id,)) => Ok(id),
        Err(_) => {
            // 如果没有 user_id,生成一个并保存
            let user_id = uuid::Uuid::new_v4().to_string();
            println!("[DB] 生成并保存新的用户 ID: {}", user_id);

            sqlx::query("INSERT INTO settings (key, value) VALUES ('user_id', ?)")
                .bind(&user_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;

            Ok(user_id)
        }
    }
}

pub async fn update_username(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    new_name: String,
) -> Result<(), String> {
    println!("[DB] updating username to: {}", new_name);

    // 验证用户名不为空
    if new_name.trim().is_empty() {
        return Err("username cannot be empty".to_string());
    }

    // 验证用户名长度
    if new_name.len() > 50 {
        return Err("username too long (max 50 chars)".to_string());
    }

    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('username', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(new_name.trim())
    .execute(&mut *transaction)
    .await
    .map_err(|e| {
        println!("[DB] update failed: {}", e);
        e.to_string()
    })?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('username_source', 'custom')
         ON CONFLICT(key) DO UPDATE SET value = 'custom'",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;
    transaction.commit().await.map_err(|e| e.to_string())?;

    println!("[DB] username updated successfully");
    Ok(())
}

// 获取下载路径
pub async fn get_download_path(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<String, String> {
    let res: Result<(String,), _> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'download_path'")
            .fetch_one(pool)
            .await;

    match res {
        Ok((path,)) => Ok(path),
        Err(_) => {
            // 如果没有设置，返回默认路径
            if cfg!(target_os = "android") {
                Ok("/storage/emulated/0/Download/Xchat".to_string())
            } else {
                let home_dir = dirs::home_dir().ok_or("cannot get home directory")?;
                let default_path = home_dir.join("Downloads").join("Xchat");
                Ok(default_path.to_string_lossy().to_string())
            }
        }
    }
}

// 更新下载路径
pub async fn update_download_path(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    new_path: String,
) -> Result<(), String> {
    println!("[DB] updating download path to: {}", new_path);

    // 验证路径不为空
    if new_path.trim().is_empty() {
        return Err("path cannot be empty".to_string());
    }

    // 尝试创建目录
    if let Err(e) = std::fs::create_dir_all(&new_path) {
        return Err(format!("cannot create directory: {}", e));
    }

    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('download_path', ?)")
        .bind(new_path.trim())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    println!("[DB] download path updated successfully");
    Ok(())
}

// 为 Tauri 桌面端初始化数据库
#[cfg(feature = "desktop")]
pub async fn init_db(app_handle: &AppHandle) -> Result<Pool<Sqlite>, sqlx::Error> {
    let app_dir = app_handle.path().app_data_dir().expect("读取路径失败");
    init_db_with_path(app_dir).await
}

// 为 Web 端初始化数据库（自动匹配平台标准路径）
pub async fn init_db_standalone(custom_path: Option<PathBuf>) -> Result<Pool<Sqlite>, sqlx::Error> {
    let app_dir = if let Some(path) = custom_path {
        path
    } else {
        // Windows: C:\Users\用户名\AppData\Roaming\com.xchat.app
        // Linux: /home/用户名/.local/share/com.xchat.app
        // macOS: /Users/用户名/Library/Application Support/com.xchat.app
        dirs::data_dir()
            .map(|p| p.join("com.xchat.app"))
            .unwrap_or_else(|| {
                // 如果实在拿不到系统路径（极罕见），回退到当前目录下的 data 文件夹
                eprintln!("[DB] 无法获取系统数据目录，回退到本地路径");
                PathBuf::from(".").join("data")
            })
    };

    init_db_with_path(app_dir).await
}

// 通用的数据库初始化逻辑
async fn init_db_with_path(app_dir: PathBuf) -> Result<Pool<Sqlite>, sqlx::Error> {
    let machine_name = machine_name();
    init_db_with_path_and_machine_name(app_dir, &machine_name).await
}

async fn init_db_with_path_and_machine_name(
    app_dir: PathBuf,
    machine_name: &str,
) -> Result<Pool<Sqlite>, sqlx::Error> {
    println!("[DB] 数据库路径: {:?}", app_dir);

    // 确保目录一定存在
    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir).unwrap();
    }

    let db_path = app_dir.join("xchat.db");
    let db_url = format!("sqlite:{}", db_path.to_str().unwrap());

    // 检查文件是否存在，如果不存在，手动创建空文件
    if !db_path.exists() {
        std::fs::File::create(&db_path).unwrap();
    }

    let pool = SqlitePool::connect(&db_url).await?;

    // 创建表结构
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sender_id TEXT,
            receiver_id TEXT,
            content TEXT,
            msg_type TEXT,
            timestamp INTEGER,
            file_path TEXT,
            file_status TEXT,
            file_size INTEGER,
            status TEXT DEFAULT 'sent',
            sender_msg_id TEXT,
            conversation_id TEXT,
            client_message_id TEXT
        )",
    )
    .execute(&pool)
    .await?;

    // 数据库迁移：为现有的messages表添加receiver_id字段（如果不存在）
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN receiver_id TEXT")
        .execute(&pool)
        .await; // 忽略错误，因为字段可能已经存在

    // 数据库迁移：为现有的messages表添加file_size字段（如果不存在）
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN file_size INTEGER")
        .execute(&pool)
        .await;

    // 数据库迁移：为现有的messages表添加status字段（如果不存在）
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN status TEXT DEFAULT 'sent'")
        .execute(&pool)
        .await;

    // 数据库迁移：为现有的messages表添加sender_msg_id字段（对方的消息ID，用于手动下载回执）
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN sender_msg_id TEXT")
        .execute(&pool)
        .await;

    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN conversation_id TEXT")
        .execute(&pool)
        .await;

    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN client_message_id TEXT")
        .execute(&pool)
        .await;

    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_client_message_id
         ON messages(client_message_id)",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_messages_conversation_timestamp
         ON messages(conversation_id, timestamp DESC)",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT
        )",
    )
    .execute(&pool)
    .await?;

    // 创建 users 表存储历史用户
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            name TEXT,
            addr TEXT,
            last_seen INTEGER,
            is_offline INTEGER DEFAULT 0,
            available_memory_mb INTEGER DEFAULT 0,
            hostname TEXT,
            mac_address TEXT,
            remark TEXT,
            discovery_source TEXT
        )",
    )
    .execute(&pool)
    .await?;

    let _ = sqlx::query("ALTER TABLE users ADD COLUMN hostname TEXT")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN mac_address TEXT")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN remark TEXT")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN discovery_source TEXT")
        .execute(&pool)
        .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            peer_id TEXT,
            title TEXT,
            created_by TEXT,
            pinned INTEGER NOT NULL DEFAULT 0,
            forced_unread INTEGER NOT NULL DEFAULT 0,
            draft TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            version INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await?;

    let _ = sqlx::query("ALTER TABLE conversations ADD COLUMN version INTEGER NOT NULL DEFAULT 0")
        .execute(&pool)
        .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS conversation_members (
            conversation_id TEXT NOT NULL,
            peer_id TEXT NOT NULL,
            display_name TEXT NOT NULL,
            role TEXT NOT NULL,
            joined_at INTEGER NOT NULL,
            PRIMARY KEY (conversation_id, peer_id)
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS message_receipts (
            message_client_id TEXT NOT NULL,
            reader_id TEXT NOT NULL,
            delivered_at INTEGER,
            read_at INTEGER,
            updated_at INTEGER NOT NULL,
            delivery_ack_sent_at INTEGER,
            read_ack_sent_at INTEGER,
            PRIMARY KEY (message_client_id, reader_id)
        )",
    )
    .execute(&pool)
    .await?;

    let _ = sqlx::query("ALTER TABLE message_receipts ADD COLUMN delivery_ack_sent_at INTEGER")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE message_receipts ADD COLUMN read_ack_sent_at INTEGER")
        .execute(&pool)
        .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS transfers (
            id TEXT PRIMARY KEY,
            message_id INTEGER,
            conversation_id TEXT NOT NULL,
            peer_id TEXT NOT NULL,
            direction TEXT NOT NULL,
            status TEXT NOT NULL,
            bytes_total INTEGER NOT NULL,
            bytes_transferred INTEGER NOT NULL,
            error TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_transfers_updated_at
         ON transfers(updated_at DESC)",
    )
    .execute(&pool)
    .await?;

    // 创建 persisted_uris 表追踪已持久化的 content URI 权限
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS persisted_uris (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uri TEXT NOT NULL,
            msg_id INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 初始化配置；旧版自动生成名只迁移一次，用户自定义名称不覆盖。
    let username =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'username'")
            .fetch_optional(&pool)
            .await?;
    let username_source =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'username_source'")
            .fetch_optional(&pool)
            .await?;

    if username.is_none() {
        println!("[DB] 使用本机名称: {}", machine_name);

        // 生成唯一的 UUID
        let user_id = uuid::Uuid::new_v4().to_string();
        println!("[DB] 生成用户 ID: {}", user_id);

        sqlx::query("INSERT INTO settings (key, value) VALUES ('username', ?)")
            .bind(machine_name)
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('username_source', 'machine')")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES ('user_id', ?)")
            .bind(user_id)
            .execute(&pool)
            .await?;

        // 初始保存路径 - 统一使用 ~/Downloads/Xchat
        let download_dir = if cfg!(target_os = "android") {
            "/storage/emulated/0/Download/Xchat".to_string()
        } else {
            let home_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
            home_dir
                .join("Downloads")
                .join("Xchat")
                .to_string_lossy()
                .to_string()
        };

        println!("[DB] 设置默认下载路径: {}", download_dir);

        sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES ('download_path', ?)")
            .bind(download_dir)
            .execute(&pool)
            .await?;
    } else if username_source.is_none() {
        let username = username.as_deref().unwrap();
        let (value, source) = if is_legacy_generated_name(&username) {
            (machine_name, "machine")
        } else {
            (username, "custom")
        };
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('username', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(value)
        .execute(&pool)
        .await?;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('username_source', ?)")
            .bind(source)
            .execute(&pool)
            .await?;
    } else if username_source.as_deref() == Some("machine")
        && username.as_deref() != Some(machine_name)
    {
        sqlx::query("UPDATE settings SET value = ? WHERE key = 'username'")
            .bind(machine_name)
            .execute(&pool)
            .await?;
    }

    Ok(pool)
}

// ==================== 文件相关的数据库函数 ====================

/// 保存文件消息到数据库
pub async fn save_file_message(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    peer_id: String,
    file_name: String,
    file_size: usize,
    file_path: String,
    file_status: String,
    overall_status: String,
) -> Result<i64, String> {
    println!(
        "[DB] 保存文件消息: 文件={}, 大小={}, 状态={}",
        file_name, file_size, file_status
    );
    // 检查是否存在
    let existing = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM messages WHERE receiver_id = ? AND content = ? AND msg_type = 'file' AND file_status = 'uploading' ORDER BY id DESC LIMIT 1"
    )
    .bind(&peer_id).bind(&file_name).fetch_optional(pool).await.map_err(|e| e.to_string())?;

    if let Some((msg_id,)) = existing {
        sqlx::query("UPDATE messages SET file_path = ?, file_status = ?, status = ? WHERE id = ?") // <--- 【修改 2】更新 status
            .bind(&file_path)
            .bind(&file_status)
            .bind(&overall_status)
            .bind(msg_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(msg_id)
    } else {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // <--- 【修改 3】把 status 写入 INSERT
        let result = sqlx::query(
            "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp, file_path, file_status, file_size, sender_msg_id, status) VALUES ('me', ?, ?, 'file', ?, ?, ?, ?, '0', ?)"
        )
        .bind(&peer_id).bind(&file_name).bind(timestamp).bind(&file_path)
        .bind(&file_status).bind(file_size as i64).bind(&overall_status)
        .execute(pool).await.map_err(|e| e.to_string())?;

        let new_id = result.last_insert_rowid();
        // 更新 sender_msg_id 为实际 msg_id
        let _ = sqlx::query("UPDATE messages SET sender_msg_id = ? WHERE id = ?")
            .bind(new_id.to_string())
            .bind(new_id)
            .execute(pool)
            .await;
        println!(
            "[DB] 文件消息保存完成, id={}, sender_msg_id={}",
            new_id, new_id
        );
        Ok(new_id)
    }
}

/// 获取下载中的文件（根据发送者ID）
pub async fn get_downloading_file(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: &str,
) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT content FROM messages WHERE sender_id = ? AND msg_type = 'file' AND file_status = 'downloading' ORDER BY id DESC LIMIT 1")
        .bind(sender_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询文件失败: {}", e))?;

    if let Some(row) = row {
        use sqlx::Row;
        let file_name: String = row.get("content");
        Ok(Some(file_name))
    } else {
        Ok(None)
    }
}

/// 按 sender_msg_id 查询当前正在下载的文件名（多文件并发隔离）
pub async fn get_downloading_file_by_sender_msg_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_msg_id: &str,
) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT content FROM messages WHERE sender_msg_id = ? AND msg_type = 'file' AND file_status = 'downloading' ORDER BY id DESC LIMIT 1")
        .bind(sender_msg_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询文件失败: {}", e))?;

    if let Some(row) = row {
        use sqlx::Row;
        let file_name: String = row.get("content");
        Ok(Some(file_name))
    } else {
        Ok(None)
    }
}

/// 更新文件状态（从 downloading 到 accepted）
pub async fn update_file_status(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    file_name: &str,
    new_status: &str,
) -> Result<(), String> {
    println!("[DB] 更新文件状态: {} -> {}", file_name, new_status);

    sqlx::query(
        "UPDATE messages SET file_status = ? WHERE content = ? AND msg_type = 'file' AND file_status = 'downloading'"
    )
    .bind(new_status)
    .bind(file_name)
    .execute(pool)
    .await
    .map_err(|e| format!("更新文件状态失败: {}", e))?;

    println!("[DB] 文件状态已更新");
    Ok(())
}

/// 创建文件接收记录（Web端发送文件时）
pub async fn create_upload_record(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    receiver_id: String,
    file_name: String,
    file_size: u64,
    timestamp: i64,
    file_status: String,
    overall_status: String,
) -> Result<i64, String> {
    println!(
        "[DB] 创建上传记录: 接收者={}, 文件={}, 状态={}",
        receiver_id, file_name, overall_status
    );

    let new_id: i64 = {
        let result = sqlx::query(
            "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp, file_path, file_status, file_size, sender_msg_id, status) 
             VALUES ('me', ?, ?, 'file', ?, '', ?, ?, '0', ?)"
        )
        .bind(&receiver_id)
        .bind(&file_name)
        .bind(timestamp)
        .bind(&file_status)
        .bind(file_size as i64)
        .bind(&overall_status)
        .execute(pool)
        .await
        .map_err(|e| format!("创建记录失败: {}", e))?;
        result.last_insert_rowid()
    };
    // 更新 sender_msg_id 为实际 msg_id
    let _ = sqlx::query("UPDATE messages SET sender_msg_id = ? WHERE id = ?")
        .bind(new_id.to_string())
        .bind(new_id)
        .execute(pool)
        .await;
    println!(
        "[DB] 上传记录创建完成, id={}, sender_msg_id={}",
        new_id, new_id
    );
    Ok(new_id)
}

/// 更新上传状态
pub async fn update_upload_status(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    file_name: String,
    status: String,
) -> Result<(), String> {
    println!(
        "[DB] Web前端确认发送完毕，强制同步更新状态: {} -> {}",
        file_name, status
    );
    // 只要前端说传完了，不管后端之前以为它是 pending 还是 uploading，统统强制标为已发送！
    sqlx::query(
        "UPDATE messages SET file_status = ?, status = 'sent' WHERE sender_id = 'me' AND content = ?"
    )
    .bind(&status)
    .bind(&file_name)
    .execute(pool)
    .await
    .map_err(|e| format!("更新状态失败: {}", e))?;

    println!("[DB] 上传状态已强制更新为 sent，移出补发队列");
    Ok(())
}

/// 删除上传记录（上传失败时）
pub async fn delete_upload_record(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    file_name: String,
    timestamp: i64,
) -> Result<(), String> {
    println!("[DB] 删除上传记录: {}", file_name);

    sqlx::query(
        "DELETE FROM messages WHERE sender_id = 'me' AND content = ? AND timestamp = ? AND file_status = 'uploading'"
    )
    .bind(&file_name)
    .bind(timestamp)
    .execute(pool)
    .await
    .map_err(|e| format!("删除记录失败: {}", e))?;

    println!("[DB] 上传记录已删除");
    Ok(())
}

/// 更新文件状态（通过消息ID）
pub async fn update_file_status_by_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
    new_status: &str,
) -> Result<(), String> {
    println!("[DB] 更新文件状态（ID: {}）: -> {}", msg_id, new_status);

    sqlx::query("UPDATE messages SET file_status = ? WHERE id = ?")
        .bind(new_status)
        .bind(msg_id)
        .execute(pool)
        .await
        .map_err(|e| format!("更新文件状态失败: {}", e))?;

    println!("[DB] 文件状态已更新");
    Ok(())
}

/// 删除消息（通过消息ID）
pub async fn delete_message_by_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
) -> Result<(), String> {
    println!("[DB] 删除消息: ID {}", msg_id);

    sqlx::query("DELETE FROM messages WHERE id = ?")
        .bind(msg_id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除消息失败: {}", e))?;

    println!("[DB] 消息已删除");
    Ok(())
}

/// 保存文本消息
pub async fn save_text_message(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    receiver_id: String,
    content: String,
) -> Result<(), String> {
    println!(
        "[DB] 保存文本消息: 接收者={}, 内容长度={}",
        receiver_id,
        content.len()
    );

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    sqlx::query(
        "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp) VALUES ('me', ?, ?, 'text', ?)"
    )
    .bind(&receiver_id)
    .bind(&content)
    .bind(timestamp)
    .execute(pool)
    .await
    .map_err(|e| format!("保存消息失败: {}", e))?;

    println!("[DB] 文本消息已保存");
    Ok(())
}

/// 保存当前主题
pub async fn save_current_theme(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    theme_name: String,
) -> Result<(), String> {
    println!("[DB] 保存当前主题: {}", theme_name);

    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('current_theme', ?)")
        .bind(&theme_name)
        .execute(pool)
        .await
        .map_err(|e| format!("保存主题失败: {}", e))?;

    println!("[DB] 主题已保存");
    Ok(())
}

/// 获取当前主题
pub async fn get_current_theme(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<Option<String>, String> {
    let result =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'current_theme'")
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("查询主题失败: {}", e))?;

    Ok(result)
}

/// 保存接收到的文本消息（来自其他对等体）
pub async fn save_received_text_message(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: String,
    content: String,
    msg_type: String,
    timestamp: i64,
) -> Result<i64, String> {
    println!(
        "[DB] 保存接收到的文本消息: 发送者={}, 内容长度={}",
        sender_id,
        content.len()
    );

    let result = sqlx::query(
        "INSERT INTO messages (sender_id, content, msg_type, timestamp) VALUES (?, ?, ?, ?)",
    )
    .bind(&sender_id)
    .bind(&content)
    .bind(&msg_type)
    .bind(timestamp)
    .execute(pool)
    .await
    .map_err(|e| format!("保存消息失败: {}", e))?;

    let msg_id = result.last_insert_rowid();
    println!("[DB] 接收到的消息已保存, ID: {}", msg_id);
    Ok(msg_id)
}

/// 更新文件状态（通过文件路径）
pub async fn update_file_status_by_path(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    old_path: &str,
    new_path: &str,
    new_status: &str,
) -> Result<(), String> {
    println!(
        "[DB] 更新文件状态（路径）: {} -> {}, 状态: {}",
        old_path, new_path, new_status
    );

    sqlx::query("UPDATE messages SET file_status = ?, file_path = ? WHERE file_path = ?")
        .bind(new_status)
        .bind(new_path)
        .bind(old_path)
        .execute(pool)
        .await
        .map_err(|e| format!("更新文件状态失败: {}", e))?;

    println!("[DB] 文件状态已更新");
    Ok(())
}

/// 获取所有文件消息（用于调试）
pub async fn get_all_file_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    limit: i64,
) -> Result<Vec<(i64, String, String, String, String)>, String> {
    println!("[DB] 获取所有文件消息（限制: {}）", limit);

    let rows = sqlx::query_as::<_, (i64, String, String, String, String)>(
        "SELECT id, sender_id, content, file_path, file_status FROM messages WHERE msg_type = 'file' ORDER BY id DESC LIMIT ?"
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询文件消息失败: {}", e))?;

    println!("[DB] 找到 {} 条文件消息", rows.len());
    Ok(rows)
}

/// 查询待接收的文件（通过文件路径模糊匹配）
pub async fn get_pending_file_by_path(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    path_pattern: &str,
) -> Result<Option<(String, String)>, String> {
    println!("[DB] 查询待接收文件: 路径模式={}", path_pattern);

    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT file_path, content FROM messages WHERE file_path LIKE ? AND file_status = 'pending'"
    )
    .bind(path_pattern)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询文件失败: {}", e))?;

    if let Some((path, name)) = &row {
        println!("[DB] 找到待接收文件: {} ({})", name, path);
    } else {
        println!("[DB] 未找到待接收文件");
    }

    Ok(row)
}

/// 创建接收文件记录（接收来自其他对等体的文件）
pub async fn create_received_file_record(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: String,
    file_name: String,
    file_path: String,
    file_size: u64,
    timestamp: i64,
    sender_msg_id: &str, // 发送端的 DB row ID，用于进度 DOM 查找
) -> Result<i64, String> {
    // 获取当前用户ID作为接收者
    let my_id = get_user_id(pool).await?;

    let result = sqlx::query(
        "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp, file_path, file_status, file_size, sender_msg_id) VALUES (?, ?, ?, 'file', ?, ?, 'downloading', ?, ?)"
    )
    .bind(&sender_id)
    .bind(&my_id)
    .bind(&file_name)
    .bind(timestamp)
    .bind(&file_path)
    .bind(file_size as i64)
    .bind(sender_msg_id)
    .execute(pool)
    .await
    .map_err(|e| format!("创建记录失败: {}", e))?;

    let msg_id = result.last_insert_rowid();
    println!(
        "[DB] ✓ 接收文件记录已创建，ID: {}, 文件: {}, 状态: downloading, sender_msg_id: {}",
        msg_id, file_name, sender_msg_id
    );
    Ok(msg_id)
}

/// 批量删除消息（通过消息ID列表）
pub async fn delete_messages_by_ids(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_ids: Vec<i64>,
) -> Result<(), String> {
    if msg_ids.is_empty() {
        return Ok(());
    }

    println!("[DB] 批量删除消息: {} 条", msg_ids.len());

    // 构建 IN 子句的占位符
    let placeholders = msg_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let query_str = format!("DELETE FROM messages WHERE id IN ({})", placeholders);

    let mut query = sqlx::query(&query_str);
    for id in msg_ids {
        query = query.bind(id);
    }

    query
        .execute(pool)
        .await
        .map_err(|e| format!("批量删除消息失败: {}", e))?;

    println!("[DB] 消息已批量删除");
    Ok(())
}

// ==================== 历史用户管理函数 ====================

/// 保存或更新用户到历史用户表
pub async fn save_or_update_user(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    id: String,
    name: String,
    addr: String,
    is_offline: bool,
    available_memory_mb: u64,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    sqlx::query(
        "INSERT INTO users (id, name, addr, last_seen, is_offline, available_memory_mb)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            addr = excluded.addr,
            last_seen = excluded.last_seen,
            is_offline = excluded.is_offline,
            available_memory_mb = CASE
                WHEN users.available_memory_mb <= 0
                     AND excluded.available_memory_mb > 0
                THEN excluded.available_memory_mb
                ELSE users.available_memory_mb
            END",
    )
    .bind(&id)
    .bind(&name)
    .bind(&addr)
    .bind(now)
    .bind(if is_offline { 1 } else { 0 })
    .bind(available_memory_mb as i64)
    .execute(pool)
    .await
    .map_err(|e| format!("保存用户失败: {}", e))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn save_or_update_discovered_user(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    id: &str,
    name: &str,
    addr: &str,
    available_memory_mb: u64,
    hostname: Option<&str>,
    mac_address: Option<&str>,
    discovery_source: Option<&str>,
    authoritative: bool,
) -> Result<(), String> {
    save_or_update_user(
        pool,
        id.to_string(),
        name.to_string(),
        addr.to_string(),
        false,
        if authoritative {
            available_memory_mb
        } else {
            0
        },
    )
    .await?;
    if authoritative {
        update_user_metadata(pool, id, hostname, mac_address, discovery_source).await?;
    }
    Ok(())
}

/// 获取所有历史用户（包括离线的）
pub async fn get_all_users(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Vec<(String, String, String, i64, bool, u64)>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64)>(
        "SELECT id, name, addr, last_seen, is_offline, available_memory_mb FROM users ORDER BY last_seen DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(id, name, addr, last_seen, is_offline, mem)| {
            (id, name, addr, last_seen, is_offline != 0, mem as u64)
        })
        .collect())
}

/// 获取所有挂起的消息（发送给离线用户的消息）
pub async fn get_pending_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    receiver_id: &str,
) -> Result<Vec<(i64, String, String, i64, Option<String>, Option<i64>)>, String> {
    let rows = sqlx::query_as::<_, (i64, String, String, i64, Option<String>, Option<i64>)>(
        "SELECT id, content, msg_type, timestamp, file_path, file_size FROM messages WHERE receiver_id = ? AND status = 'pending' ORDER BY timestamp ASC"
    )
    .bind(receiver_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询挂起消息失败: {}", e))?;

    Ok(rows)
}

/// 标记消息为已发送
pub async fn mark_message_as_sent(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
) -> Result<(), String> {
    sqlx::query("UPDATE messages SET status = 'sent' WHERE id = ?")
        .bind(msg_id)
        .execute(pool)
        .await
        .map_err(|e| format!("更新消息状态失败: {}", e))?;

    Ok(())
}

/// 标记多条消息为已发送
pub async fn mark_messages_as_sent(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_ids: Vec<i64>,
) -> Result<(), String> {
    if msg_ids.is_empty() {
        return Ok(());
    }

    let placeholders = msg_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let query_str = format!(
        "UPDATE messages SET status = 'sent' WHERE id IN ({})",
        placeholders
    );

    let mut query = sqlx::query(&query_str);
    for id in msg_ids {
        query = query.bind(id);
    }

    query
        .execute(pool)
        .await
        .map_err(|e| format!("批量更新消息状态失败: {}", e))?;

    Ok(())
}

/// 保存文本消息（支持挂起状态）
pub async fn save_text_message_with_status(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    receiver_id: String,
    content: String,
    status: String,
) -> Result<i64, String> {
    println!(
        "[DB] 保存文本消息: 接收者={}, 内容长度={}, 状态={}",
        receiver_id,
        content.len(),
        status
    );

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let result = sqlx::query(
        "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp, status) VALUES ('me', ?, ?, 'text', ?, ?)"
    )
    .bind(&receiver_id)
    .bind(&content)
    .bind(timestamp)
    .bind(&status)
    .execute(pool)
    .await
    .map_err(|e| format!("保存消息失败: {}", e))?;

    let msg_id = result.last_insert_rowid();
    println!("[DB] 文本消息已保存，ID: {}, 状态: {}", msg_id, status);
    Ok(msg_id)
}

/// 查询聊天历史（带偏移量，用于懒加载）
pub async fn get_chat_history_with_offset(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    peer_id: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<crate::models::Message>, String> {
    // 获取当前用户ID
    let my_id = get_user_id(pool).await?;

    // 【核心修复】：在两处 SELECT 中加上了 status 字段
    let messages = sqlx::query_as::<_, crate::models::Message>(
        "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path, file_status, file_size, sender_msg_id, status 
         FROM (
            SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path, file_status, file_size, sender_msg_id, status 
            FROM messages 
            WHERE 
                (sender_id = ? AND receiver_id = ?) OR 
                (sender_id = ? AND (receiver_id = ? OR receiver_id IS NULL)) OR
                (sender_id = 'me' AND receiver_id = ?)
            ORDER BY timestamp DESC 
            LIMIT ? OFFSET ?
         ) 
         ORDER BY timestamp ASC",
    )
    .bind(&my_id)
    .bind(peer_id)
    .bind(peer_id)
    .bind(&my_id)
    .bind(peer_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询历史失败: {}", e))?;

    Ok(messages)
}

/// 专门用于保存从网络接收到的文本消息
pub async fn save_network_message(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    from_id: &str,
    content: &str,
    msg_type: &str,
    timestamp: u64,
) -> Result<i64, String> {
    // 获取当前用户ID作为接收者
    let my_id = get_user_id(pool).await?;

    let result = sqlx::query(
        "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(from_id)
    .bind(&my_id)
    .bind(content)
    .bind(msg_type)
    .bind(timestamp as i64)
    .execute(pool)
    .await
    .map_err(|e| format!("保存网络消息失败: {}", e))?;

    println!("[DB] 接收自网络的消息已保存到数据库");

    // 返回生成的数据库行 ID
    Ok(result.last_insert_rowid())
}

/// 根据发送者和文件名获取最新的一条消息 ID
pub async fn get_latest_msg_id_by_file(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: &str,
    file_name: &str,
) -> Option<i64> {
    // 尝试最多 3 次查询，每次间隔 50ms，应对写入延迟
    for _ in 0..3 {
        let res = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM messages WHERE sender_id = ? AND content = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(sender_id)
        .bind(file_name)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

        if res.is_some() {
            return res;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    None
}

/// 查询消息的 sender_msg_id（用于 file_status_update 广播）
pub async fn get_sender_msg_id_by_file(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: &str,
    file_name: &str,
) -> Result<String, String> {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT sender_msg_id FROM messages WHERE sender_id = ? AND content = ? AND sender_msg_id IS NOT NULL AND sender_msg_id != '' ORDER BY id DESC LIMIT 1"
    )
    .bind(sender_id)
    .bind(file_name)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询 sender_msg_id 失败: {}", e))?;

    Ok(result.unwrap_or_default())
}

/// 清空与某个用户的聊天记录
pub async fn clear_chat_history(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    my_id: &str,
    peer_id: &str,
) -> Result<(), String> {
    println!("[DB] 清空与用户 {} 的聊天记录", peer_id);
    sqlx::query(
        "DELETE FROM messages WHERE
        (sender_id IN ('me', ?) AND receiver_id = ?) OR
        (sender_id = ? AND (receiver_id IN ('me', ?) OR receiver_id IS NULL))",
    )
    .bind(my_id)
    .bind(peer_id)
    .bind(peer_id)
    .bind(my_id)
    .execute(pool)
    .await
    .map_err(|e| format!("清空聊天记录失败: {}", e))?;
    Ok(())
}

pub async fn clear_conversation_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
) -> Result<(), String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|e| format!("开始清空会话失败: {e}"))?;
    sqlx::query(
        "DELETE FROM message_receipts
         WHERE message_client_id IN (
             SELECT client_message_id FROM messages
             WHERE conversation_id = ? AND client_message_id IS NOT NULL
         )",
    )
    .bind(conversation_id)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("清空会话回执失败: {e}"))?;
    sqlx::query("DELETE FROM messages WHERE conversation_id = ?")
        .bind(conversation_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| format!("清空会话消息失败: {e}"))?;
    transaction
        .commit()
        .await
        .map_err(|e| format!("提交清空会话失败: {e}"))
}

/// 删除用户及其所有聊天记录
pub async fn delete_user_and_history(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    my_id: &str,
    peer_id: &str,
) -> Result<(), String> {
    // 1. 删除消息
    clear_chat_history(pool, my_id, peer_id).await?;
    // 2. 删除只属于该设备的 direct 会话
    sqlx::query("DELETE FROM conversations WHERE kind = 'direct' AND peer_id = ?")
        .bind(peer_id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除私聊会话失败: {}", e))?;
    // 3. 从用户表删除
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(peer_id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除用户失败: {}", e))?;
    println!("[DB] 用户 {} 及其记录已彻底删除", peer_id);
    Ok(())
}

// ── 自定义 IP ──

const CUSTOM_PEER_KEY_PREFIX: &str = "custom_peer_";

/// 获取所有自定义 IP
pub async fn get_custom_peers(pool: &sqlx::Pool<sqlx::Sqlite>) -> Vec<String> {
    let pattern = format!("{}%", CUSTOM_PEER_KEY_PREFIX);
    match sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows.into_iter().map(|r| r.0).collect(),
        Err(e) => {
            eprintln!("[DB] 读取自定义 IP 失败: {}", e);
            vec![]
        }
    }
}

/// 添加自定义 IP
pub async fn add_custom_peer(pool: &sqlx::Pool<sqlx::Sqlite>, peer: &str) -> Result<(), String> {
    let key = format!("{}{}", CUSTOM_PEER_KEY_PREFIX, peer);
    sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES (?, ?)")
        .bind(&key)
        .bind(peer)
        .execute(pool)
        .await
        .map_err(|e| format!("添加自定义 IP 失败: {}", e))?;
    println!("[DB] 已添加自定义 IP: {}", peer);
    Ok(())
}

/// 删除自定义 IP
pub async fn remove_custom_peer(pool: &sqlx::Pool<sqlx::Sqlite>, peer: &str) -> Result<(), String> {
    let key = format!("{}{}", CUSTOM_PEER_KEY_PREFIX, peer);
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(&key)
        .execute(pool)
        .await
        .map_err(|e| format!("删除自定义 IP 失败: {}", e))?;
    println!("[DB] 已删除自定义 IP: {}", peer);
    Ok(())
}

// ── 通知开关 ──────────────────────────────────────────────

/// 获取通知开关状态（默认开启）
pub async fn get_notifications_enabled(pool: &sqlx::Pool<sqlx::Sqlite>) -> bool {
    let res = sqlx::query_as::<_, (String,)>(
        "SELECT value FROM settings WHERE key = 'notifications_enabled'",
    )
    .fetch_one(pool)
    .await;

    match res {
        Ok((val,)) => val == "true",
        Err(_) => true, // 默认开启
    }
}

/// 设置通知开关状态
pub async fn set_notifications_enabled(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    enabled: bool,
) -> Result<(), String> {
    let val = if enabled { "true" } else { "false" };
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('notifications_enabled', ?)")
        .bind(val)
        .execute(pool)
        .await
        .map_err(|e| format!("保存通知设置失败: {}", e))?;
    println!("[DB] 通知状态已设置为: {}", val);
    Ok(())
}

// ── 自动下载开关 ──────────────────────────────────────────────

/// 获取自动下载开关状态（默认开启）
pub async fn get_auto_download(pool: &sqlx::Pool<sqlx::Sqlite>) -> bool {
    let res =
        sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'auto_download'")
            .fetch_one(pool)
            .await;

    match res {
        Ok((val,)) => val == "true",
        Err(_) => true, // 默认开启
    }
}

/// 设置自动下载开关
pub async fn set_auto_download(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    enabled: bool,
) -> Result<(), String> {
    let val = if enabled { "true" } else { "false" };
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('auto_download', ?)")
        .bind(val)
        .execute(pool)
        .await
        .map_err(|e| format!("保存自动下载设置失败: {}", e))?;
    println!("[DB] 自动下载已设置为: {}", val);
    Ok(())
}

/// 获取端口（仅 Android 端使用，其他平台走 config.json）
pub async fn get_port(pool: &sqlx::Pool<sqlx::Sqlite>) -> Option<u16> {
    let res = sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'port'")
        .fetch_one(pool)
        .await;

    match res {
        Ok((val,)) => val.parse::<u16>().ok(),
        Err(_) => None,
    }
}

/// 设置端口（仅 Android 端使用）
pub async fn set_port(pool: &sqlx::Pool<sqlx::Sqlite>, port: u16) -> Result<(), String> {
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('port', ?)")
        .bind(port.to_string())
        .execute(pool)
        .await
        .map_err(|e| format!("保存端口设置失败: {}", e))?;
    println!("[DB] 端口已设置为: {}", port);
    Ok(())
}

// ── 手动下载（file_offer / file_request） ───────────────────────

/// 当收到 file_offer 且 auto_download=OFF 时，创建 "offered" 状态的记录
pub async fn create_offered_file_record(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: &str,
    file_name: &str,
    file_size: u64,
    sender_msg_id: &str,
    timestamp: i64,
) -> Result<i64, String> {
    let my_id = get_user_id(pool).await?;
    let result = sqlx::query(
        "INSERT INTO messages (sender_id, receiver_id, content, msg_type, timestamp, file_status, file_size, sender_msg_id) \
         VALUES (?, ?, ?, 'file', ?, 'offered', ?, ?)"
    )
    .bind(sender_id)
    .bind(&my_id)
    .bind(file_name)
    .bind(timestamp)
    .bind(file_size as i64)
    .bind(sender_msg_id)
    .execute(pool)
    .await
    .map_err(|e| format!("创建 offered 记录失败: {}", e))?;
    Ok(result.last_insert_rowid())
}

/// 当收到文件上传时，查找并更新已存在的 offered 记录为 downloading
/// 返回 (被更新的记录ID, sender_msg_id)（如果找到并更新），否则返回 None
pub async fn find_and_update_offered_record(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_id: &str,
    file_name: &str,
    file_path: &str,
) -> Result<Option<(i64, String)>, String> {
    // 查找 sender_id + file_name 匹配且 file_status = 'offered' 的记录，获取 id 和 sender_msg_id
    let result = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, sender_msg_id FROM messages WHERE sender_id = ? AND content = ? AND file_status = 'offered' LIMIT 1"
    )
    .bind(sender_id)
    .bind(file_name)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询 offered 记录失败: {}", e))?;

    if let Some((msg_id, sender_msg_id)) = result {
        sqlx::query("UPDATE messages SET file_status = 'downloading', file_path = ? WHERE id = ?")
            .bind(file_path)
            .bind(msg_id)
            .execute(pool)
            .await
            .map_err(|e| format!("更新 offered 为 downloading 失败: {}", e))?;
        println!(
            "[DB] 已更新 offered 记录(ID={}, sender_msg_id={}) 为 downloading，路径: {}",
            msg_id, sender_msg_id, file_path
        );
        Ok(Some((msg_id, sender_msg_id)))
    } else {
        Ok(None)
    }
}

/// 通过 sender_msg_id 更新 file_status
pub async fn update_file_status_by_sender_msg_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    sender_msg_id: &str,
    status: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE messages SET file_status = ? WHERE sender_msg_id = ?")
        .bind(status)
        .bind(sender_msg_id)
        .execute(pool)
        .await
        .map_err(|e| format!("更新文件状态失败: {}", e))?;
    Ok(())
}

/// 通过 sender_msg_id 查询发送端的文件记录（file_path 等）
pub async fn get_sender_file_by_msg_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
) -> Result<(String, String, i64), String> {
    sqlx::query_as::<_, (String, String, i64)>(
        "SELECT file_path, content, file_size FROM messages WHERE id = ? AND sender_id = 'me'",
    )
    .bind(msg_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("查询发送端文件记录失败: {}", e))
}

// ═══════════════════════════════════════════════════════════════
// persisted_uris 表 CRUD
// ═══════════════════════════════════════════════════════════════

/// 添加一条持久化 URI 追踪记录
pub async fn add_persisted_uri(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uri: &str,
    msg_id: i64,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    sqlx::query("INSERT OR IGNORE INTO persisted_uris (uri, msg_id, created_at) VALUES (?, ?, ?)")
        .bind(uri)
        .bind(msg_id)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|e| format!("添加持久化 URI 记录失败: {}", e))?;

    println!("[DB] 已添加持久化 URI: msg_id={}, uri={}", msg_id, uri);
    Ok(())
}

/// 删除一条持久化 URI 追踪记录
pub async fn remove_persisted_uri(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uri: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM persisted_uris WHERE uri = ?")
        .bind(uri)
        .execute(pool)
        .await
        .map_err(|e| format!("删除持久化 URI 记录失败: {}", e))?;
    Ok(())
}

/// 通过 msg_id 删除持久化 URI 追踪记录
pub async fn remove_persisted_uri_by_msg_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
) -> Result<(), String> {
    sqlx::query("DELETE FROM persisted_uris WHERE msg_id = ?")
        .bind(msg_id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除持久化 URI 通过 msg_id 失败: {}", e))?;
    Ok(())
}

/// 查询最早的持久化 URI（用于 FIFO 淘汰）
pub async fn get_oldest_persisted_uri(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Option<(i64, String, i64)>, String> {
    // 返回 (id, uri, msg_id)，按 created_at ASC
    let result = sqlx::query_as::<_, (i64, String, i64)>(
        "SELECT id, uri, msg_id FROM persisted_uris ORDER BY created_at ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询最旧持久化 URI 失败: {}", e))?;
    Ok(result)
}

/// 统计当前追踪的持久化 URI 数量
pub async fn count_persisted_uris(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<i64, String> {
    let result = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM persisted_uris")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("统计持久化 URI 数量失败: {}", e))?;
    Ok(result)
}

/// 通过 URI 查询对应的 msg_id
pub async fn get_msg_id_for_uri(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uri: &str,
) -> Result<Option<i64>, String> {
    let result = sqlx::query_scalar::<_, i64>("SELECT msg_id FROM persisted_uris WHERE uri = ?")
        .bind(uri)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("通过 URI 查询 msg_id 失败: {}", e))?;
    Ok(result)
}

/// 通过 msg_id 查询对应的 URI
pub async fn get_uri_by_msg_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
) -> Result<Option<String>, String> {
    let result = sqlx::query_scalar::<_, String>("SELECT uri FROM persisted_uris WHERE msg_id = ?")
        .bind(msg_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("通过 msg_id 查询 URI 失败: {}", e))?;
    Ok(result)
}

/// 更新消息的 file_path（用于持久化失败时降级为 FD 缓存标记）
pub async fn update_file_path_by_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    msg_id: i64,
    file_path: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE messages SET file_path = ? WHERE id = ?")
        .bind(file_path)
        .bind(msg_id)
        .execute(pool)
        .await
        .map_err(|e| format!("更新 file_path 失败: {}", e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// React 会话模型
// ═══════════════════════════════════════════════════════════════

pub async fn get_conversation(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
) -> Result<Option<ConversationRecord>, String> {
    sqlx::query_as::<_, ConversationRecord>(
        "SELECT id, kind, peer_id, title, created_by, pinned, forced_unread, draft,
                created_at, updated_at, version
         FROM conversations WHERE id = ?",
    )
    .bind(conversation_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询会话失败: {}", e))
}

pub async fn update_conversation_state(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
    pinned: Option<bool>,
    forced_unread: Option<bool>,
    draft: Option<&str>,
) -> Result<ConversationRecord, String> {
    if conversation_id.trim().is_empty() {
        return Err("conversation id is required".to_string());
    }
    let result = sqlx::query(
        "UPDATE conversations SET
            pinned = COALESCE(?, pinned),
            forced_unread = COALESCE(?, forced_unread),
            draft = COALESCE(?, draft)
         WHERE id = ?",
    )
    .bind(pinned)
    .bind(forced_unread)
    .bind(draft)
    .bind(conversation_id)
    .execute(pool)
    .await
    .map_err(|e| format!("更新会话状态失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err("conversation not found".to_string());
    }
    get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())
}

async fn ensure_direct_conversation_at(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    my_id: &str,
    peer_id: &str,
    created_at: i64,
    updated_at: i64,
) -> Result<ConversationRecord, String> {
    if peer_id.trim().is_empty() || peer_id == my_id {
        return Err("invalid direct conversation peer".to_string());
    }

    let conversation_id = stable_direct_conversation_id(my_id, peer_id);
    if let Some(existing) = get_conversation(pool, &conversation_id).await? {
        if existing.kind != "direct" || existing.peer_id.as_deref() != Some(peer_id) {
            return Err("direct conversation id conflicts with another conversation".to_string());
        }
    }
    let peer_name = sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(NULLIF(remark, ''), NULLIF(name, ''), id)
         FROM users WHERE id = ?",
    )
    .bind(peer_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询用户名称失败: {}", e))?
    .unwrap_or_else(|| peer_id.to_string());
    let my_name = get_username(pool)
        .await
        .unwrap_or_else(|_| my_id.to_string());

    sqlx::query(
        "INSERT INTO conversations
            (id, kind, peer_id, title, created_by, created_at, updated_at)
         VALUES (?, 'direct', ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            kind = 'direct',
            peer_id = excluded.peer_id,
            title = COALESCE(conversations.title, excluded.title)",
    )
    .bind(&conversation_id)
    .bind(peer_id)
    .bind(&peer_name)
    .bind(my_id)
    .bind(created_at)
    .bind(updated_at)
    .execute(pool)
    .await
    .map_err(|e| format!("保存直接会话失败: {}", e))?;

    for (member_id, display_name) in [(my_id, my_name.as_str()), (peer_id, peer_name.as_str())] {
        sqlx::query(
            "INSERT INTO conversation_members
                (conversation_id, peer_id, display_name, role, joined_at)
             VALUES (?, ?, ?, 'member', ?)
             ON CONFLICT(conversation_id, peer_id) DO UPDATE SET
                display_name = excluded.display_name",
        )
        .bind(&conversation_id)
        .bind(member_id)
        .bind(display_name)
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(|e| format!("保存直接会话成员失败: {}", e))?;
    }

    get_conversation(pool, &conversation_id)
        .await?
        .ok_or_else(|| "direct conversation was not saved".to_string())
}

pub async fn ensure_direct_conversation(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    peer_id: &str,
) -> Result<ConversationRecord, String> {
    let my_id = get_user_id(pool).await?;
    let now = unix_timestamp();
    ensure_direct_conversation_at(pool, &my_id, peer_id, now, now).await
}

pub async fn create_group_conversation(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: Option<&str>,
    title: &str,
    created_by: &str,
    members: &[NewConversationMember],
) -> Result<ConversationRecord, String> {
    let title = title.trim();
    if title.is_empty() || created_by.trim().is_empty() || members.is_empty() {
        return Err("group title, creator and members are required".to_string());
    }
    if members
        .iter()
        .any(|member| member.peer_id.trim().is_empty())
    {
        return Err("group member id cannot be empty".to_string());
    }

    let conversation_id = conversation_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    apply_group_sync(pool, &conversation_id, title, created_by, 1, members).await
}

pub async fn apply_group_sync(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
    title: &str,
    created_by: &str,
    version: i64,
    members: &[NewConversationMember],
) -> Result<ConversationRecord, String> {
    let conversation_id = conversation_id.trim();
    let title = title.trim();
    if conversation_id.is_empty()
        || title.is_empty()
        || created_by.trim().is_empty()
        || version < 1
        || members.is_empty()
        || members
            .iter()
            .any(|member| member.peer_id.trim().is_empty())
    {
        return Err("invalid group sync".to_string());
    }
    if let Some(existing) = get_conversation(pool, conversation_id).await? {
        if existing.kind != "group" {
            return Err("group id conflicts with another conversation".to_string());
        }
        if existing.version > version {
            return Ok(existing);
        }
    }

    let now = unix_timestamp();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开始群同步事务失败: {}", e))?;
    let applied = sqlx::query(
        "INSERT INTO conversations
            (id, kind, title, created_by, created_at, updated_at, version)
         VALUES (?, 'group', ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            created_by = COALESCE(conversations.created_by, excluded.created_by),
            updated_at = MAX(conversations.updated_at, excluded.updated_at),
            version = excluded.version
         WHERE excluded.version >= conversations.version",
    )
    .bind(conversation_id)
    .bind(title)
    .bind(created_by)
    .bind(now)
    .bind(now)
    .bind(version)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("保存群同步失败: {}", e))?;
    if applied.rows_affected() == 0 {
        tx.rollback().await.ok();
        return get_conversation(pool, conversation_id)
            .await?
            .ok_or_else(|| "group conversation was not saved".to_string());
    }

    sqlx::query("DELETE FROM conversation_members WHERE conversation_id = ?")
        .bind(conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("替换群聊成员失败: {}", e))?;
    for member in members {
        let role = if member.role.trim().is_empty() {
            "member"
        } else {
            member.role.trim()
        };
        sqlx::query(
            "INSERT INTO conversation_members
                (conversation_id, peer_id, display_name, role, joined_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(conversation_id)
        .bind(member.peer_id.trim())
        .bind(member.display_name.trim())
        .bind(role)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("保存群聊成员失败: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("提交群同步事务失败: {}", e))?;
    get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "group conversation was not saved".to_string())
}

pub async fn list_conversations(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Vec<ConversationRecord>, String> {
    let my_id = get_user_id(pool).await?;
    let legacy_peers = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT peer_id, COALESCE(MIN(timestamp), 0), COALESCE(MAX(timestamp), 0)
         FROM (
            SELECT
                CASE
                    WHEN sender_id = 'me' OR sender_id = ? THEN receiver_id
                    ELSE sender_id
                END AS peer_id,
                timestamp
            FROM messages
            WHERE conversation_id IS NULL
         )
         WHERE peer_id IS NOT NULL AND peer_id != '' AND peer_id != ?
         GROUP BY peer_id",
    )
    .bind(&my_id)
    .bind(&my_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询旧会话失败: {}", e))?;

    for (peer_id, created_at, updated_at) in legacy_peers {
        ensure_direct_conversation_at(pool, &my_id, &peer_id, created_at, updated_at).await?;
    }

    sqlx::query_as::<_, ConversationRecord>(
        "SELECT id, kind, peer_id, title, created_by, pinned, forced_unread, draft,
                created_at, updated_at, version
         FROM conversations
         ORDER BY pinned DESC, updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询会话列表失败: {}", e))
}

pub async fn get_conversation_members(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
) -> Result<Vec<ConversationMemberRecord>, String> {
    sqlx::query_as::<_, ConversationMemberRecord>(
        "SELECT conversation_id, peer_id, display_name, role, joined_at
         FROM conversation_members
         WHERE conversation_id = ?
         ORDER BY joined_at ASC, peer_id ASC",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询会话成员失败: {}", e))
}

async fn hydrate_legacy_conversation_ids(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    messages: &mut [MessageRecord],
) -> Result<(), String> {
    let my_id = get_user_id(pool).await?;
    let mut peers = std::collections::BTreeMap::<String, (i64, i64)>::new();

    for message in messages {
        if message.conversation_id.is_some() {
            continue;
        }
        let peer_id = if message.sender_id == "me" || message.sender_id == my_id {
            message.receiver_id.as_deref()
        } else {
            Some(message.sender_id.as_str())
        };
        let Some(peer_id) = peer_id.filter(|id| !id.is_empty() && *id != my_id) else {
            continue;
        };

        message.conversation_id = Some(stable_direct_conversation_id(&my_id, peer_id));
        peers
            .entry(peer_id.to_string())
            .and_modify(|range| {
                range.0 = range.0.min(message.timestamp);
                range.1 = range.1.max(message.timestamp);
            })
            .or_insert((message.timestamp, message.timestamp));
    }

    for (peer_id, (created_at, updated_at)) in peers {
        ensure_direct_conversation_at(pool, &my_id, &peer_id, created_at, updated_at).await?;
    }
    Ok(())
}

pub async fn save_conversation_message(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
    sender_id: &str,
    receiver_id: Option<&str>,
    content: &str,
    msg_type: &str,
    timestamp: i64,
    status: &str,
    client_message_id: &str,
) -> Result<MessageRecord, String> {
    if conversation_id.trim().is_empty()
        || sender_id.trim().is_empty()
        || msg_type.trim().is_empty()
        || client_message_id.trim().is_empty()
    {
        return Err("conversation, sender, message type and client message id are required".into());
    }
    if get_conversation(pool, conversation_id).await?.is_none() {
        return Err("conversation not found".to_string());
    }

    let timestamp = if timestamp > 0 {
        timestamp
    } else {
        unix_timestamp()
    };
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开始消息事务失败: {}", e))?;

    sqlx::query(
        "INSERT INTO messages
            (sender_id, receiver_id, content, msg_type, timestamp, status,
             conversation_id, client_message_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(client_message_id) DO UPDATE SET
            status = CASE
                WHEN messages.status = 'read' THEN messages.status
                WHEN messages.status = 'delivered'
                     AND excluded.status IN ('pending', 'sent') THEN messages.status
                WHEN messages.status = 'sent' AND excluded.status = 'pending'
                     THEN messages.status
                ELSE excluded.status
            END",
    )
    .bind(sender_id)
    .bind(receiver_id)
    .bind(content)
    .bind(msg_type)
    .bind(timestamp)
    .bind(status)
    .bind(conversation_id)
    .bind(client_message_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("保存会话消息失败: {}", e))?;

    let message = sqlx::query_as::<_, MessageRecord>(
        "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                file_status, file_size, sender_msg_id, status, conversation_id,
                client_message_id
         FROM messages WHERE client_message_id = ?",
    )
    .bind(client_message_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("读取会话消息失败: {}", e))?;

    if message.conversation_id.as_deref() != Some(conversation_id)
        || message.sender_id != sender_id
        || message.content != content
        || message.msg_type != msg_type
    {
        tx.rollback().await.ok();
        return Err("client message id conflicts with another message".to_string());
    }

    sqlx::query(
        "UPDATE conversations
         SET updated_at = MAX(updated_at, ?)
         WHERE id = ?",
    )
    .bind(timestamp)
    .bind(conversation_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("更新会话时间失败: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("提交消息事务失败: {}", e))?;
    Ok(message)
}

pub async fn get_message_by_client_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    client_message_id: &str,
) -> Result<Option<MessageRecord>, String> {
    sqlx::query_as::<_, MessageRecord>(
        "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                file_status, file_size, sender_msg_id, status, conversation_id,
                client_message_id
         FROM messages WHERE client_message_id = ?",
    )
    .bind(client_message_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("按稳定 ID 查询消息失败: {}", e))
}

pub async fn mark_message_status_by_client_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    client_message_id: &str,
    status: &str,
) -> Result<MessageRecord, String> {
    let result = sqlx::query(
        "UPDATE messages SET status = CASE
            WHEN status = 'read' THEN status
            WHEN status = 'delivered' AND ? IN ('pending', 'sent') THEN status
            WHEN status = 'sent' AND ? = 'pending' THEN status
            ELSE ?
         END
         WHERE client_message_id = ?",
    )
    .bind(status)
    .bind(status)
    .bind(status)
    .bind(client_message_id)
    .execute(pool)
    .await
    .map_err(|e| format!("更新消息状态失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err("message not found".to_string());
    }
    get_message_by_client_id(pool, client_message_id)
        .await?
        .ok_or_else(|| "message not found".to_string())
}

pub async fn get_conversation_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    conversation_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<MessageRecord>, String> {
    let conversation = get_conversation(pool, conversation_id)
        .await?
        .ok_or_else(|| "conversation not found".to_string())?;
    let limit = limit.clamp(1, 200);
    let offset = offset.max(0);

    let mut messages = if conversation.kind == "direct" {
        let peer_id = conversation
            .peer_id
            .as_deref()
            .ok_or_else(|| "direct conversation has no peer".to_string())?;
        let my_id = get_user_id(pool).await?;
        sqlx::query_as::<_, MessageRecord>(
            "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                    file_status, file_size, sender_msg_id, status, conversation_id,
                    client_message_id
             FROM (
                SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                       file_status, file_size, sender_msg_id, status, conversation_id,
                       client_message_id
                FROM messages
                WHERE conversation_id = ?
                   OR (
                        conversation_id IS NULL AND (
                            (sender_id = 'me' AND receiver_id = ?)
                            OR (sender_id = ? AND (receiver_id = ? OR receiver_id IS NULL))
                            OR (sender_id = ? AND receiver_id = ?)
                        )
                   )
                ORDER BY timestamp DESC, id DESC
                LIMIT ? OFFSET ?
             )
             ORDER BY timestamp ASC, id ASC",
        )
        .bind(conversation_id)
        .bind(peer_id)
        .bind(peer_id)
        .bind(&my_id)
        .bind(&my_id)
        .bind(peer_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("查询直接会话消息失败: {}", e))?
    } else {
        sqlx::query_as::<_, MessageRecord>(
            "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                    file_status, file_size, sender_msg_id, status, conversation_id,
                    client_message_id
             FROM (
                SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                       file_status, file_size, sender_msg_id, status, conversation_id,
                       client_message_id
                FROM messages
                WHERE conversation_id = ?
                ORDER BY timestamp DESC, id DESC
                LIMIT ? OFFSET ?
             )
             ORDER BY timestamp ASC, id ASC",
        )
        .bind(conversation_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("查询群聊消息失败: {}", e))?
    };

    hydrate_legacy_conversation_ids(pool, &mut messages).await?;
    Ok(messages)
}

fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('%');
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('%');
    escaped
}

pub async fn search_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    search: &str,
    limit: i64,
) -> Result<Vec<MessageRecord>, String> {
    let search = search.trim();
    if search.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = escape_like_pattern(search);
    let mut messages = sqlx::query_as::<_, MessageRecord>(
        "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                file_status, file_size, sender_msg_id, status, conversation_id,
                client_message_id
         FROM messages
         WHERE msg_type IN ('text', 'file')
           AND content LIKE ? ESCAPE '\\' COLLATE NOCASE
         ORDER BY timestamp DESC, id DESC
         LIMIT ?",
    )
    .bind(pattern)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("搜索消息失败: {}", e))?;

    hydrate_legacy_conversation_ids(pool, &mut messages).await?;
    Ok(messages)
}

pub async fn ensure_message_recipients(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    client_message_id: &str,
    peer_ids: &[String],
) -> Result<(), String> {
    if client_message_id.trim().is_empty() {
        return Err("client message id is required".to_string());
    }
    if get_message_by_client_id(pool, client_message_id)
        .await?
        .is_none()
    {
        return Err("message not found".to_string());
    }

    let now = unix_timestamp();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开始消息目标事务失败: {}", e))?;
    for peer_id in peer_ids {
        if peer_id.trim().is_empty() {
            tx.rollback().await.ok();
            return Err("message recipient cannot be empty".to_string());
        }
        sqlx::query(
            "INSERT INTO message_receipts
                (message_client_id, reader_id, delivered_at, read_at, updated_at)
             VALUES (?, ?, NULL, NULL, ?)
             ON CONFLICT(message_client_id, reader_id) DO NOTHING",
        )
        .bind(client_message_id)
        .bind(peer_id.trim())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("保存消息目标失败: {}", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("提交消息目标事务失败: {}", e))
}

pub async fn save_message_receipt(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    message_client_id: &str,
    reader_id: &str,
    delivered_at: Option<i64>,
    read_at: Option<i64>,
) -> Result<MessageReceiptRecord, String> {
    if message_client_id.trim().is_empty() || reader_id.trim().is_empty() {
        return Err("message client id and reader id are required".to_string());
    }
    if delivered_at.is_none() && read_at.is_none() {
        return Err("receipt must contain delivery or read acknowledgement".to_string());
    }
    if get_message_by_client_id(pool, message_client_id)
        .await?
        .is_none()
    {
        return Err("message not found".to_string());
    }

    let now = unix_timestamp();
    sqlx::query(
        "INSERT INTO message_receipts
            (message_client_id, reader_id, delivered_at, read_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(message_client_id, reader_id) DO UPDATE SET
            delivered_at = COALESCE(message_receipts.delivered_at, excluded.delivered_at),
            read_at = COALESCE(message_receipts.read_at, excluded.read_at),
            updated_at = MAX(message_receipts.updated_at, excluded.updated_at),
            delivery_ack_sent_at = CASE
                WHEN excluded.delivered_at IS NOT NULL THEN NULL
                ELSE message_receipts.delivery_ack_sent_at
            END,
            read_ack_sent_at = CASE
                WHEN excluded.read_at IS NOT NULL THEN NULL
                ELSE message_receipts.read_ack_sent_at
            END",
    )
    .bind(message_client_id)
    .bind(reader_id)
    .bind(delivered_at)
    .bind(read_at)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| format!("保存消息回执失败: {}", e))?;

    sqlx::query_as::<_, MessageReceiptRecord>(
        "SELECT message_client_id, reader_id, delivered_at, read_at, updated_at,
                delivery_ack_sent_at, read_ack_sent_at
         FROM message_receipts
         WHERE message_client_id = ? AND reader_id = ?",
    )
    .bind(message_client_id)
    .bind(reader_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("读取消息回执失败: {}", e))
}

pub async fn get_message_receipts(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    message_client_id: &str,
) -> Result<Vec<MessageReceiptRecord>, String> {
    sqlx::query_as::<_, MessageReceiptRecord>(
        "SELECT message_client_id, reader_id, delivered_at, read_at, updated_at,
                delivery_ack_sent_at, read_ack_sent_at
         FROM message_receipts
         WHERE message_client_id = ?
         ORDER BY reader_id ASC",
    )
    .bind(message_client_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询消息回执失败: {}", e))
}

pub async fn get_pending_receipts_for_peer(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    peer_id: &str,
) -> Result<Vec<PendingReceiptRecord>, String> {
    sqlx::query_as::<_, PendingReceiptRecord>(
        "SELECT r.message_client_id, m.conversation_id, r.reader_id,
                r.delivered_at, r.read_at, r.delivery_ack_sent_at,
                r.read_ack_sent_at
         FROM message_receipts r
         INNER JOIN messages m ON m.client_message_id = r.message_client_id
         WHERE m.sender_id = ?
           AND m.conversation_id IS NOT NULL
           AND (
                (r.delivered_at IS NOT NULL AND r.delivery_ack_sent_at IS NULL)
                OR (r.read_at IS NOT NULL AND r.read_ack_sent_at IS NULL)
           )
         ORDER BY r.updated_at ASC",
    )
    .bind(peer_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询待回送消息回执失败: {}", e))
}

pub async fn mark_receipt_ack_sent(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    message_client_id: &str,
    reader_id: &str,
    ack_kind: &str,
) -> Result<MessageReceiptRecord, String> {
    let (column, acknowledged_column) = match ack_kind {
        "delivery" => ("delivery_ack_sent_at", "delivered_at"),
        "read" => ("read_ack_sent_at", "read_at"),
        _ => return Err("ack kind must be delivery or read".to_string()),
    };
    // column names are selected from the closed match above, never from user input.
    let query = format!(
        "UPDATE message_receipts SET {column} = ?
         WHERE message_client_id = ? AND reader_id = ?
           AND {acknowledged_column} IS NOT NULL"
    );
    let result = sqlx::query(&query)
        .bind(unix_timestamp())
        .bind(message_client_id)
        .bind(reader_id)
        .execute(pool)
        .await
        .map_err(|e| format!("标记消息回执已发送失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err("receipt acknowledgement not found".to_string());
    }
    get_message_receipts(pool, message_client_id)
        .await?
        .into_iter()
        .find(|receipt| receipt.reader_id == reader_id)
        .ok_or_else(|| "receipt acknowledgement not found".to_string())
}

pub async fn get_undelivered_messages_for_peer(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    peer_id: &str,
) -> Result<Vec<MessageRecord>, String> {
    sqlx::query_as::<_, MessageRecord>(
        "SELECT m.id, m.sender_id, m.receiver_id, m.content, m.msg_type, m.timestamp,
                m.file_path, m.file_status, m.file_size, m.sender_msg_id, m.status,
                m.conversation_id, m.client_message_id
         FROM messages m
         INNER JOIN message_receipts r
           ON r.message_client_id = m.client_message_id
         WHERE r.reader_id = ? AND r.delivered_at IS NULL
         ORDER BY m.timestamp ASC, m.id ASC",
    )
    .bind(peer_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询未送达消息失败: {}", e))
}

pub async fn list_groups_for_member(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    peer_id: &str,
) -> Result<Vec<ConversationRecord>, String> {
    sqlx::query_as::<_, ConversationRecord>(
        "SELECT c.id, c.kind, c.peer_id, c.title, c.created_by, c.pinned,
                c.forced_unread, c.draft, c.created_at, c.updated_at, c.version
         FROM conversations c
         INNER JOIN conversation_members cm ON cm.conversation_id = c.id
         WHERE c.kind = 'group' AND cm.peer_id = ?
         ORDER BY c.updated_at DESC",
    )
    .bind(peer_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询成员群聊失败: {}", e))
}

pub async fn list_file_messages(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    limit: i64,
    offset: i64,
) -> Result<Vec<FileMessageRecord>, String> {
    let mut messages = sqlx::query_as::<_, FileMessageRecord>(
        "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                file_status, file_size, sender_msg_id, status, conversation_id,
                client_message_id
         FROM messages
         WHERE msg_type = 'file'
         ORDER BY timestamp DESC, id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(limit.clamp(1, 500))
    .bind(offset.max(0))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询文件消息失败: {}", e))?;

    hydrate_legacy_conversation_ids(pool, &mut messages).await?;
    Ok(messages)
}

pub async fn get_file_message_by_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    message_id: i64,
) -> Result<Option<FileMessageRecord>, String> {
    sqlx::query_as::<_, FileMessageRecord>(
        "SELECT id, sender_id, receiver_id, content, msg_type, timestamp, file_path,
                file_status, file_size, sender_msg_id, status, conversation_id,
                client_message_id
         FROM messages
         WHERE id = ? AND msg_type = 'file'",
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询文件消息失败: {}", e))
}

pub async fn set_file_message_metadata(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    message_id: i64,
    file_path: &str,
    file_size: i64,
    file_status: &str,
) -> Result<FileMessageRecord, String> {
    if file_path.trim().is_empty() || file_size < 0 || file_status.trim().is_empty() {
        return Err("invalid file metadata".to_string());
    }
    let result = sqlx::query(
        "UPDATE messages
         SET file_path = ?, file_size = ?, file_status = ?
         WHERE id = ? AND msg_type = 'file'",
    )
    .bind(file_path)
    .bind(file_size)
    .bind(file_status)
    .bind(message_id)
    .execute(pool)
    .await
    .map_err(|e| format!("保存文件元数据失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err("file message not found".to_string());
    }
    get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())
}

pub async fn clear_file_path_and_mark_removed(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    message_id: i64,
) -> Result<FileMessageRecord, String> {
    let message = get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())?;
    let my_id = get_user_id(pool).await?;
    if message.sender_id == "me" || message.sender_id == my_id {
        return Err("cannot remove sender source file".to_string());
    }

    sqlx::query(
        "UPDATE messages
         SET file_path = NULL, file_status = 'removed'
         WHERE id = ? AND msg_type = 'file'",
    )
    .bind(message_id)
    .execute(pool)
    .await
    .map_err(|e| format!("标记本地文件已删除失败: {}", e))?;

    get_file_message_by_id(pool, message_id)
        .await?
        .ok_or_else(|| "file message not found".to_string())
}

fn valid_transfer_direction(direction: &str) -> bool {
    matches!(direction, "send" | "receive")
}

fn valid_transfer_status(status: &str) -> bool {
    matches!(
        status,
        "queued"
            | "waiting_peer"
            | "offering"
            | "awaiting_acceptance"
            | "transferring"
            | "completed"
            | "cancelling"
            | "cancelled"
            | "failed"
    )
}

pub async fn get_transfer(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    transfer_id: &str,
) -> Result<Option<TransferRecord>, String> {
    sqlx::query_as::<_, TransferRecord>(
        "SELECT id, message_id, conversation_id, peer_id, direction, status,
                bytes_total, bytes_transferred, error, created_at, updated_at
         FROM transfers WHERE id = ?",
    )
    .bind(transfer_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询传输失败: {}", e))
}

pub async fn create_transfer(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    transfer_id: &str,
    message_id: Option<i64>,
    conversation_id: &str,
    peer_id: &str,
    direction: &str,
    status: &str,
    bytes_total: i64,
) -> Result<TransferRecord, String> {
    if transfer_id.trim().is_empty()
        || conversation_id.trim().is_empty()
        || peer_id.trim().is_empty()
        || !valid_transfer_direction(direction)
        || !valid_transfer_status(status)
        || bytes_total < 0
    {
        return Err("invalid transfer".to_string());
    }
    if get_conversation(pool, conversation_id).await?.is_none() {
        return Err("conversation not found".to_string());
    }

    let now = unix_timestamp();
    sqlx::query(
        "INSERT INTO transfers
            (id, message_id, conversation_id, peer_id, direction, status,
             bytes_total, bytes_transferred, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(transfer_id)
    .bind(message_id)
    .bind(conversation_id)
    .bind(peer_id)
    .bind(direction)
    .bind(status)
    .bind(bytes_total)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| format!("创建传输失败: {}", e))?;

    let transfer = get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer was not saved".to_string())?;
    if transfer.conversation_id != conversation_id
        || transfer.peer_id != peer_id
        || transfer.direction != direction
        || transfer.message_id != message_id
    {
        return Err("transfer id conflicts with another transfer".to_string());
    }
    Ok(transfer)
}

pub async fn update_transfer(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    transfer_id: &str,
    status: &str,
    bytes_transferred: i64,
    error: Option<&str>,
) -> Result<TransferRecord, String> {
    if !valid_transfer_status(status) || bytes_transferred < 0 {
        return Err("invalid transfer update".to_string());
    }
    let current = get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer not found".to_string())?;
    if matches!(current.status.as_str(), "completed" | "cancelled") {
        return Ok(current);
    }

    sqlx::query(
        "UPDATE transfers
         SET status = ?,
             bytes_transferred = MAX(bytes_transferred, MIN(?, bytes_total)),
             error = ?,
             updated_at = ?
         WHERE id = ? AND status NOT IN ('completed', 'cancelled')",
    )
    .bind(status)
    .bind(bytes_transferred)
    .bind(error)
    .bind(unix_timestamp())
    .bind(transfer_id)
    .execute(pool)
    .await
    .map_err(|e| format!("更新传输失败: {}", e))?;

    get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer not found".to_string())
}

pub async fn transition_transfer_status(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    transfer_id: &str,
    expected_status: &str,
    status: &str,
    bytes_transferred: i64,
    error: Option<&str>,
) -> Result<Option<TransferRecord>, String> {
    if !valid_transfer_status(expected_status)
        || !valid_transfer_status(status)
        || bytes_transferred < 0
    {
        return Err("invalid transfer transition".to_string());
    }
    let result = sqlx::query(
        "UPDATE transfers
         SET status = ?,
             bytes_transferred = MAX(bytes_transferred, MIN(?, bytes_total)),
             error = ?,
             updated_at = ?
         WHERE id = ? AND status = ?",
    )
    .bind(status)
    .bind(bytes_transferred)
    .bind(error)
    .bind(unix_timestamp())
    .bind(transfer_id)
    .bind(expected_status)
    .execute(pool)
    .await
    .map_err(|e| format!("更新传输状态失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    get_transfer(pool, transfer_id).await
}

pub async fn cancel_transfer(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    transfer_id: &str,
) -> Result<TransferRecord, String> {
    let result = sqlx::query(
        "UPDATE transfers
         SET status = CASE
                WHEN status IN ('queued', 'waiting_peer', 'offering', 'awaiting_acceptance')
                    THEN 'cancelled'
                WHEN status = 'transferring' THEN 'cancelling'
                ELSE status
             END,
             updated_at = CASE
                WHEN status IN ('queued', 'waiting_peer', 'offering',
                                'awaiting_acceptance', 'transferring')
                    THEN ?
                ELSE updated_at
             END
         WHERE id = ?",
    )
    .bind(unix_timestamp())
    .bind(transfer_id)
    .execute(pool)
    .await
    .map_err(|e| format!("取消传输失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err("transfer not found".to_string());
    }

    get_transfer(pool, transfer_id)
        .await?
        .ok_or_else(|| "transfer not found".to_string())
}

pub async fn list_transfers(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    limit: i64,
) -> Result<Vec<TransferRecord>, String> {
    sqlx::query_as::<_, TransferRecord>(
        "SELECT id, message_id, conversation_id, peer_id, direction, status,
                bytes_total, bytes_transferred, error, created_at, updated_at
         FROM transfers
         ORDER BY updated_at DESC
         LIMIT ?",
    )
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询传输列表失败: {}", e))
}

pub async fn update_user_metadata(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    user_id: &str,
    hostname: Option<&str>,
    mac_address: Option<&str>,
    discovery_source: Option<&str>,
) -> Result<UserRecord, String> {
    if user_id.trim().is_empty() {
        return Err("user id is required".to_string());
    }
    let now = unix_timestamp();
    sqlx::query(
        "INSERT INTO users
            (id, name, addr, last_seen, is_offline, available_memory_mb,
             hostname, mac_address, discovery_source)
         VALUES (?, ?, '', ?, 1, 0, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            hostname = COALESCE(excluded.hostname, users.hostname),
            mac_address = COALESCE(excluded.mac_address, users.mac_address),
            discovery_source = COALESCE(excluded.discovery_source, users.discovery_source)",
    )
    .bind(user_id)
    .bind(user_id)
    .bind(now)
    .bind(hostname.map(str::trim).filter(|value| !value.is_empty()))
    .bind(mac_address.map(str::trim).filter(|value| !value.is_empty()))
    .bind(
        discovery_source
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )
    .execute(pool)
    .await
    .map_err(|e| format!("保存设备元数据失败: {}", e))?;

    get_user_metadata(pool, user_id)
        .await?
        .ok_or_else(|| "user metadata was not saved".to_string())
}

pub async fn set_user_remark(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    user_id: &str,
    remark: Option<&str>,
) -> Result<UserRecord, String> {
    if user_id.trim().is_empty() {
        return Err("user id is required".to_string());
    }
    let now = unix_timestamp();
    let remark = remark.map(str::trim).filter(|value| !value.is_empty());
    sqlx::query(
        "INSERT INTO users
            (id, name, addr, last_seen, is_offline, available_memory_mb, remark)
         VALUES (?, ?, '', ?, 1, 0, ?)
         ON CONFLICT(id) DO UPDATE SET remark = excluded.remark",
    )
    .bind(user_id)
    .bind(user_id)
    .bind(now)
    .bind(remark)
    .execute(pool)
    .await
    .map_err(|e| format!("保存设备备注失败: {}", e))?;

    get_user_metadata(pool, user_id)
        .await?
        .ok_or_else(|| "user remark was not saved".to_string())
}

pub async fn get_user_metadata(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    user_id: &str,
) -> Result<Option<UserRecord>, String> {
    sqlx::query_as::<_, UserRecord>(
        "SELECT id, name, addr, last_seen, is_offline, available_memory_mb,
                hostname, mac_address, remark, discovery_source
         FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询设备元数据失败: {}", e))
}

pub async fn list_users_with_metadata(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Vec<UserRecord>, String> {
    sqlx::query_as::<_, UserRecord>(
        "SELECT id, name, addr, last_seen, is_offline, available_memory_mb,
                hostname, mac_address, remark, discovery_source
         FROM users ORDER BY last_seen DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询设备列表失败: {}", e))
}

pub async fn get_setting(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    key: &str,
) -> Result<Option<String>, String> {
    if key.trim().is_empty() {
        return Err("setting key is required".to_string());
    }
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询设置失败: {}", e))
}

pub async fn set_setting(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("setting key is required".to_string());
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|e| format!("保存设置失败: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovered_user_metadata_keeps_the_first_authoritative_memory_snapshot() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE users (
                id TEXT PRIMARY KEY,
                name TEXT,
                addr TEXT,
                last_seen INTEGER,
                is_offline INTEGER DEFAULT 0,
                available_memory_mb INTEGER DEFAULT 0,
                hostname TEXT,
                mac_address TEXT,
                remark TEXT,
                discovery_source TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        save_or_update_discovered_user(
            &pool,
            "peer-1",
            "Alice",
            "127.0.0.1:8888",
            0,
            Some("reply-host"),
            Some("ac:de:48:00:11:22"),
            Some("lan"),
            false,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE users
             SET hostname = 'reply-host', mac_address = 'ac:de:48:00:11:22'
             WHERE id = 'peer-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        save_or_update_discovered_user(
            &pool,
            "peer-1",
            "Alice",
            "127.0.0.1:8888",
            2356,
            Some("alice-mac"),
            Some("82:ae:17:28:c4:04"),
            Some("lan"),
            true,
        )
        .await
        .unwrap();
        save_or_update_discovered_user(
            &pool,
            "peer-1",
            "Alice",
            "127.0.0.1:8888",
            0,
            Some("reply-host"),
            Some("ac:de:48:00:11:22"),
            Some("lan"),
            false,
        )
        .await
        .unwrap();
        save_or_update_discovered_user(
            &pool,
            "peer-1",
            "Alice",
            "127.0.0.1:8888",
            2367,
            Some("alice-mac"),
            Some("82:ae:17:28:c4:04"),
            Some("lan"),
            true,
        )
        .await
        .unwrap();

        let peer = get_user_metadata(&pool, "peer-1").await.unwrap().unwrap();
        assert_eq!(peer.available_memory_mb, 2356);
        assert_eq!(peer.hostname.as_deref(), Some("alice-mac"));
        assert_eq!(peer.mac_address.as_deref(), Some("82:ae:17:28:c4:04"));
    }

    #[tokio::test]
    async fn machine_name_migration_preserves_custom_names() {
        let app_dir =
            std::env::temp_dir().join(format!("xchat-name-test-{}", uuid::Uuid::new_v4()));
        let pool = init_db_with_path_and_machine_name(app_dir.clone(), "Office Mac")
            .await
            .unwrap();
        assert_eq!(get_username(&pool).await.unwrap(), "Office Mac");
        assert_eq!(
            get_setting(&pool, "username_source")
                .await
                .unwrap()
                .as_deref(),
            Some("machine")
        );
        pool.close().await;
        let pool = init_db_with_path_and_machine_name(app_dir.clone(), "Renamed Office Mac")
            .await
            .unwrap();
        assert_eq!(get_username(&pool).await.unwrap(), "Renamed Office Mac");

        sqlx::query("UPDATE settings SET value = 'Happy-Fox-662' WHERE key = 'username'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM settings WHERE key = 'username_source'")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let pool = init_db_with_path_and_machine_name(app_dir.clone(), "Renamed Mac")
            .await
            .unwrap();
        assert_eq!(get_username(&pool).await.unwrap(), "Renamed Mac");
        assert_eq!(
            get_setting(&pool, "username_source")
                .await
                .unwrap()
                .as_deref(),
            Some("machine")
        );

        sqlx::query("UPDATE settings SET value = 'Alice' WHERE key = 'username'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM settings WHERE key = 'username_source'")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let pool = init_db_with_path_and_machine_name(app_dir.clone(), "Third Mac")
            .await
            .unwrap();
        assert_eq!(get_username(&pool).await.unwrap(), "Alice");
        assert_eq!(
            get_setting(&pool, "username_source")
                .await
                .unwrap()
                .as_deref(),
            Some("custom")
        );

        update_username(&pool, "Bob".into()).await.unwrap();
        assert_eq!(
            get_setting(&pool, "username_source")
                .await
                .unwrap()
                .as_deref(),
            Some("custom")
        );
        pool.close().await;
        let pool = init_db_with_path_and_machine_name(app_dir.clone(), "Fourth Mac")
            .await
            .unwrap();
        assert_eq!(get_username(&pool).await.unwrap(), "Bob");
        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }

    #[test]
    fn stable_direct_ids_and_literal_search_patterns() {
        assert_eq!(
            stable_direct_conversation_id("peer-b", "peer-a"),
            "direct:peer-a:peer-b"
        );
        assert_eq!(escape_like_pattern(r"50%_done\ok"), r"%50\%\_done\\ok%");
    }

    #[tokio::test]
    async fn clears_legacy_and_current_direct_history_before_deleting_a_user() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY,
                sender_id TEXT,
                receiver_id TEXT,
                content TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE users (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE conversations (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                peer_id TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages (id, sender_id, receiver_id, content) VALUES
                (1, 'me', 'peer-a', 'legacy outgoing'),
                (2, 'self-id', 'peer-a', 'current outgoing'),
                (3, 'peer-a', NULL, 'legacy incoming'),
                (4, 'peer-a', 'me', 'legacy addressed incoming'),
                (5, 'peer-a', 'self-id', 'current incoming'),
                (6, 'self-id', 'peer-b', 'unrelated outgoing'),
                (7, 'peer-b', 'self-id', 'unrelated incoming')",
        )
        .execute(&pool)
        .await
        .unwrap();

        clear_chat_history(&pool, "self-id", "peer-a")
            .await
            .unwrap();
        let remaining = sqlx::query_scalar::<_, String>("SELECT content FROM messages ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            remaining,
            vec![
                "unrelated outgoing".to_string(),
                "unrelated incoming".to_string()
            ]
        );

        sqlx::query("INSERT INTO users (id) VALUES ('peer-a')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO conversations (id, kind, peer_id) VALUES
                ('direct-a', 'direct', 'peer-a'),
                ('direct-b', 'direct', 'peer-b'),
                ('group-a', 'group', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages (id, sender_id, receiver_id, content)
             VALUES (8, 'self-id', 'peer-a', 'delete with user')",
        )
        .execute(&pool)
        .await
        .unwrap();
        delete_user_and_history(&pool, "self-id", "peer-a")
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = 'peer-a'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM messages WHERE sender_id = 'peer-a' OR receiver_id = 'peer-a'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM conversations WHERE peer_id = 'peer-a'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn clears_only_the_selected_conversation_and_its_receipts() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY,
                conversation_id TEXT,
                client_message_id TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE message_receipts (
                message_client_id TEXT,
                reader_id TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages VALUES
                (1, 'group-a', 'message-a'),
                (2, 'group-b', 'message-b')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO message_receipts VALUES
                ('message-a', 'peer-a'),
                ('message-b', 'peer-b')",
        )
        .execute(&pool)
        .await
        .unwrap();

        clear_conversation_messages(&pool, "group-a")
            .await
            .unwrap();

        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT message_client_id FROM message_receipts")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "message-b"
        );
    }

    #[tokio::test]
    async fn migrates_old_database_and_persists_new_state_idempotently() {
        let app_dir = std::env::temp_dir().join(format!("xchat-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&app_dir).unwrap();
        let db_path = app_dir.join("xchat.db");
        std::fs::File::create(&db_path).unwrap();
        let db_url = format!("sqlite:{}", db_path.to_string_lossy());
        let old_pool = SqlitePool::connect(&db_url).await.unwrap();

        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender_id TEXT,
                receiver_id TEXT,
                content TEXT,
                msg_type TEXT,
                timestamp INTEGER,
                file_path TEXT,
                file_status TEXT
            )",
        )
        .execute(&old_pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT)")
            .execute(&old_pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE users (
                id TEXT PRIMARY KEY,
                name TEXT,
                addr TEXT,
                last_seen INTEGER,
                is_offline INTEGER DEFAULT 0,
                available_memory_mb INTEGER DEFAULT 0
            )",
        )
        .execute(&old_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO settings (key, value)
             VALUES ('username', 'Me'), ('user_id', 'self')",
        )
        .execute(&old_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (id, name, addr, last_seen)
             VALUES ('peer-a', 'Peer A', '127.0.0.1:8888', 10)",
        )
        .execute(&old_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages
                (sender_id, receiver_id, content, msg_type, timestamp)
             VALUES ('peer-a', 'self', 'legacy', 'text', 10)",
        )
        .execute(&old_pool)
        .await
        .unwrap();
        old_pool.close().await;

        let pool = init_db_with_path(app_dir.clone()).await.unwrap();
        pool.close().await;
        let pool = init_db_with_path(app_dir.clone()).await.unwrap();

        let message_columns =
            sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info('messages')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(message_columns.contains(&"conversation_id".to_string()));
        assert!(message_columns.contains(&"client_message_id".to_string()));

        let conversations = list_conversations(&pool).await.unwrap();
        let direct_id = stable_direct_conversation_id("self", "peer-a");
        assert!(conversations.iter().any(|item| item.id == direct_id));
        let state =
            update_conversation_state(&pool, &direct_id, Some(true), Some(true), Some("draft"))
                .await
                .unwrap();
        assert!(state.pinned && state.forced_unread);
        assert_eq!(state.draft, "draft");

        let initial_members = vec![
            NewConversationMember {
                peer_id: "self".into(),
                display_name: "Me".into(),
                role: "owner".into(),
            },
            NewConversationMember {
                peer_id: "peer-a".into(),
                display_name: "Peer A".into(),
                role: "member".into(),
            },
        ];
        let group = create_group_conversation(
            &pool,
            Some("group-1"),
            "First title",
            "self",
            &initial_members,
        )
        .await
        .unwrap();
        assert_eq!(group.version, 1);
        let replacement_members = vec![NewConversationMember {
            peer_id: "peer-b".into(),
            display_name: "Peer B".into(),
            role: "member".into(),
        }];
        assert_eq!(
            apply_group_sync(
                &pool,
                "group-1",
                "Second title",
                "self",
                2,
                &replacement_members,
            )
            .await
            .unwrap()
            .version,
            2
        );
        assert_eq!(
            apply_group_sync(&pool, "group-1", "Stale title", "self", 1, &initial_members,)
                .await
                .unwrap()
                .title
                .as_deref(),
            Some("Second title")
        );
        assert_eq!(
            get_conversation_members(&pool, "group-1").await.unwrap()[0].peer_id,
            "peer-b"
        );

        let message = save_conversation_message(
            &pool,
            &direct_id,
            "self",
            Some("peer-a"),
            "100% ready",
            "text",
            20,
            "sent",
            "client-1",
        )
        .await
        .unwrap();
        let duplicate = save_conversation_message(
            &pool,
            &direct_id,
            "self",
            Some("peer-a"),
            "100% ready",
            "text",
            20,
            "pending",
            "client-1",
        )
        .await
        .unwrap();
        assert_eq!(message.id, duplicate.id);
        assert_eq!(duplicate.status.as_deref(), Some("sent"));
        assert_eq!(search_messages(&pool, "%", 100).await.unwrap().len(), 1);

        ensure_message_recipients(
            &pool,
            "client-1",
            &["peer-a".to_string(), "peer-b".to_string()],
        )
        .await
        .unwrap();
        save_message_receipt(&pool, "client-1", "peer-a", Some(30), None)
            .await
            .unwrap();
        let receipt = save_message_receipt(&pool, "client-1", "peer-a", None, Some(40))
            .await
            .unwrap();
        assert_eq!(receipt.delivered_at, Some(30));
        assert_eq!(receipt.read_at, Some(40));
        assert!(get_undelivered_messages_for_peer(&pool, "peer-a")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            get_undelivered_messages_for_peer(&pool, "peer-b")
                .await
                .unwrap()
                .len(),
            1
        );

        let transfer = create_transfer(
            &pool,
            "transfer-1",
            Some(message.id),
            &direct_id,
            "peer-a",
            "send",
            "queued",
            100,
        )
        .await
        .unwrap();
        assert_eq!(transfer.status, "queued");
        assert_eq!(
            cancel_transfer(&pool, "transfer-1").await.unwrap().status,
            "cancelled"
        );
        create_transfer(
            &pool,
            "transfer-claim",
            Some(message.id),
            &direct_id,
            "peer-a",
            "send",
            "queued",
            100,
        )
        .await
        .unwrap();
        let (first, second) = tokio::join!(
            transition_transfer_status(
                &pool,
                "transfer-claim",
                "queued",
                "transferring",
                0,
                None
            ),
            transition_transfer_status(
                &pool,
                "transfer-claim",
                "queued",
                "transferring",
                0,
                None
            )
        );
        assert_eq!(
            [first.unwrap(), second.unwrap()]
                .into_iter()
                .filter(Option::is_some)
                .count(),
            1
        );

        let sent_file = save_conversation_message(
            &pool,
            &direct_id,
            "self",
            Some("peer-a"),
            "sent.png",
            "file",
            50,
            "sent",
            "client-file-sent",
        )
        .await
        .unwrap();
        update_file_path_by_id(&pool, sent_file.id, "/tmp/sent.png")
            .await
            .unwrap();
        assert!(clear_file_path_and_mark_removed(&pool, sent_file.id)
            .await
            .is_err());

        let received_file = save_conversation_message(
            &pool,
            &direct_id,
            "peer-a",
            Some("self"),
            "received.png",
            "file",
            51,
            "sent",
            "client-file-received",
        )
        .await
        .unwrap();
        let received_file =
            set_file_message_metadata(&pool, received_file.id, "/tmp/received.png", 42, "received")
                .await
                .unwrap();
        assert_eq!(received_file.file_size, Some(42));
        assert_eq!(received_file.file_status.as_deref(), Some("received"));
        let removed = clear_file_path_and_mark_removed(&pool, received_file.id)
            .await
            .unwrap();
        assert!(removed.file_path.is_none());
        assert_eq!(removed.file_status.as_deref(), Some("removed"));
        save_message_receipt(&pool, "client-file-received", "self", Some(60), None)
            .await
            .unwrap();
        assert_eq!(
            get_pending_receipts_for_peer(&pool, "peer-a")
                .await
                .unwrap()
                .len(),
            1
        );
        let sent_ack = mark_receipt_ack_sent(&pool, "client-file-received", "self", "delivery")
            .await
            .unwrap();
        assert!(sent_ack.delivery_ack_sent_at.is_some());
        assert!(get_pending_receipts_for_peer(&pool, "peer-a")
            .await
            .unwrap()
            .is_empty());
        save_message_receipt(&pool, "client-file-received", "self", Some(60), None)
            .await
            .unwrap();
        assert_eq!(
            get_pending_receipts_for_peer(&pool, "peer-a")
                .await
                .unwrap()
                .len(),
            1
        );

        update_user_metadata(
            &pool,
            "peer-a",
            Some("peer-host"),
            Some("aa:bb:cc:dd:ee:ff"),
            Some("udp"),
        )
        .await
        .unwrap();
        save_or_update_user(
            &pool,
            "peer-a".into(),
            "Renamed".into(),
            "127.0.0.1:9999".into(),
            false,
            42,
        )
        .await
        .unwrap();
        save_or_update_user(
            &pool,
            "peer-a".into(),
            "Renamed".into(),
            "127.0.0.1:9999".into(),
            false,
            0,
        )
        .await
        .unwrap();
        let peer = get_user_metadata(&pool, "peer-a").await.unwrap().unwrap();
        assert_eq!(peer.available_memory_mb, 42);
        assert_eq!(peer.hostname.as_deref(), Some("peer-host"));

        pool.close().await;
        std::fs::remove_dir_all(app_dir).unwrap();
    }
}
