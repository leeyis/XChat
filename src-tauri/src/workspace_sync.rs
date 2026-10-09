//! Collection revisions are additive and leave the original snapshot API intact.
use super::*;
use serde::Deserialize;
use sqlx::Row;
use std::hash::{Hash, Hasher};

static PROCESS_EPOCH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| uuid::Uuid::new_v4().to_string());
const CORRECTION_SECONDS: i64 = 30;

pub(crate) async fn init_schema(pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS workspace_revisions (
        id INTEGER PRIMARY KEY CHECK(id = 1), instance TEXT NOT NULL,
        conversations INTEGER NOT NULL DEFAULT 0, devices INTEGER NOT NULL DEFAULT 0,
        files INTEGER NOT NULL DEFAULT 0, transfers INTEGER NOT NULL DEFAULT 0,
        settings INTEGER NOT NULL DEFAULT 0)",
    )
    .execute(pool)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO workspace_revisions(id, instance) VALUES (1, ?)")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(pool)
        .await?;
    for table in [
        "users",
        "conversations",
        "conversation_members",
        "messages",
        "message_receipts",
        "message_reactions",
        "message_delivery_attempts",
        "transfers",
        "settings",
    ] {
        let columns = sqlx::query(&format!("PRAGMA table_info({table})"))
            .fetch_all(pool)
            .await?;
        let changed = columns
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .filter(|name| {
                table != "transfers"
                    || !matches!(name.as_str(), "lease_token" | "lease_until" | "updated_at")
            })
            .map(|name| format!("NEW.\"{name}\" IS NOT OLD.\"{name}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        for event in ["INSERT", "UPDATE", "DELETE"] {
            let row = if event == "DELETE" { "OLD" } else { "NEW" };
            let file_changed = if table == "messages" {
                if event == "UPDATE" {
                    "(OLD.msg_type = 'file' OR NEW.msg_type = 'file')".to_string()
                } else {
                    format!("{row}.msg_type = 'file'")
                }
            } else {
                format!("EXISTS(SELECT 1 FROM messages WHERE client_message_id = {row}.message_client_id AND msg_type = 'file')")
            };
            let update = match table {
                "users" if event == "UPDATE" => "devices = devices + 1,
                    conversations = conversations + CASE WHEN NEW.name IS NOT OLD.name OR NEW.remark IS NOT OLD.remark THEN 1 ELSE 0 END,
                    files = files + CASE WHEN NEW.name IS NOT OLD.name OR NEW.remark IS NOT OLD.remark OR NEW.addr IS NOT OLD.addr THEN 1 ELSE 0 END".into(),
                "users" => "devices = devices + 1, conversations = conversations + 1, files = files + 1".into(),
                "conversations" | "conversation_members" => "conversations = conversations + 1".into(),
                "messages" | "message_receipts" | "message_reactions" | "message_delivery_attempts" => format!("conversations = conversations + 1, files = files + CASE WHEN {file_changed} THEN 1 ELSE 0 END"),
                "transfers" => "transfers = transfers + 1".into(),
                "settings" => "settings = settings + 1".into(),
                _ => unreachable!(),
            };
            let condition = if event == "UPDATE" {
                format!("WHEN {changed}")
            } else {
                String::new()
            };
            sqlx::query(&format!(
                "CREATE TRIGGER IF NOT EXISTS workspace_sync_v1_{table}_{event}
                AFTER {event} ON {table} {condition}
                BEGIN UPDATE workspace_revisions SET {update} WHERE id = 1; END"
            ))
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    epoch: String,
    database: String,
    baseline_at: i64,
    versions: [i64; 5],
    peers: u64,
    processing: u64,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceSync {
    pub cursor: String,
    pub reset: bool,
    pub changes: serde_json::Map<String, serde_json::Value>,
}

fn peer_revision(manager: &PeerManager) -> u64 {
    let mut peers = manager.get_all_peers();
    peers.sort_by(|a, b| a.id.cmp(&b.id));
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for peer in peers {
        let connection = manager.connection_snapshot(&peer.id);
        let mut device = device_from_peer(peer);
        // Timestamp changes are already covered by the users table's revision.
        device.last_seen = 0;
        serde_json::to_string(&(device, connection))
            .unwrap_or_default()
            .hash(&mut hash);
    }
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_replays_changes_and_deletions_without_rebuilding_unchanged_collections() {
        let root = std::env::temp_dir().join(format!("xchat-phase2-sync-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(root.clone())).await.unwrap();
        init_schema(&pool).await.unwrap(); // additive migration is repeatable
        let manager = PeerManager::new();
        let conversation = db::ensure_direct_conversation(&pool, "peer").await.unwrap();
        let baseline = get_sync(&pool, &manager, None).await.unwrap();
        assert!(baseline.reset);
        let idle = get_sync(&pool, &manager, Some(&baseline.cursor))
            .await
            .unwrap();
        assert!(!idle.reset);
        assert!(idle.changes.is_empty());

        db::create_transfer(
            &pool,
            "sync-transfer",
            None,
            &conversation.id,
            "peer",
            "send",
            "queued",
            100,
        )
        .await
        .unwrap();
        let inserted = get_sync(&pool, &manager, Some(&idle.cursor)).await.unwrap();
        assert_eq!(
            inserted
                .changes
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["transfers"]
        );
        let mut materialized = baseline.changes.clone();
        materialized.extend(inserted.changes.clone());
        sqlx::query("UPDATE transfers SET bytes_transferred = 40, status = 'transferring' WHERE id = 'sync-transfer'").execute(&pool).await.unwrap();
        let progress = get_sync(&pool, &manager, Some(&inserted.cursor))
            .await
            .unwrap();
        assert_eq!(progress.changes.len(), 1);
        assert_eq!(progress.changes["transfers"][0]["bytes_transferred"], 40);
        materialized.extend(progress.changes.clone());
        let full = serde_json::to_value(get_snapshot(&pool, &manager).await.unwrap()).unwrap();
        assert_eq!(serde_json::Value::Object(materialized), full);

        sqlx::query("UPDATE transfers SET bytes_transferred = bytes_transferred, lease_until = 123, lease_token = 'worker'").execute(&pool).await.unwrap();
        let unchanged = get_sync(&pool, &manager, Some(&progress.cursor))
            .await
            .unwrap();
        assert!(
            unchanged.changes.is_empty(),
            "lease renewals and no-op writes are invisible to clients"
        );
        sqlx::query("DELETE FROM transfers")
            .execute(&pool)
            .await
            .unwrap();
        let deleted = get_sync(&pool, &manager, Some(&unchanged.cursor))
            .await
            .unwrap();
        assert_eq!(deleted.changes["transfers"], serde_json::json!([]));

        let mut stale: Cursor = serde_json::from_str(&deleted.cursor).unwrap();
        for field in 0..4 {
            match field {
                0 => stale.epoch = "old-process".into(),
                1 => stale.database = "another-db".into(),
                2 => stale.baseline_at = 0,
                _ => stale.versions[0] = i64::MAX,
            }
            assert!(
                get_sync(
                    &pool,
                    &manager,
                    Some(&serde_json::to_string(&stale).unwrap())
                )
                .await
                .unwrap()
                .reset
            );
            stale = serde_json::from_str(&deleted.cursor).unwrap();
        }
        pool.close().await;
        crate::db::remove_test_database(&pool, &root).await;
    }

    #[tokio::test]
    async fn batched_conversation_previews_preserve_recalls_unread_and_legacy_history() {
        let root =
            std::env::temp_dir().join(format!("xchat-phase2-preview-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(root.clone())).await.unwrap();
        let manager = PeerManager::new();
        let self_id = db::get_user_id(&pool).await.unwrap();
        db::save_or_update_user(
            &pool,
            "peer".into(),
            "Alice".into(),
            "127.0.0.1:9".into(),
            true,
            0,
        )
        .await
        .unwrap();
        let devices = devices(&pool, &manager, &self_id).await.unwrap();
        // The baseline creates the missing conversation AFTER capturing the cursor.
        let baseline = get_sync(&pool, &manager, None).await.unwrap();
        let replay = get_sync(&pool, &manager, Some(&baseline.cursor))
            .await
            .unwrap();
        assert!(replay.changes.contains_key("conversations"));
        let record = db::list_conversations(&pool).await.unwrap().remove(0);
        for (id, content, status, timestamp) in [
            ("a", "visible", "received", 10),
            ("b", "recalled", "recalled", 20),
        ] {
            db::save_conversation_message(
                &pool,
                &record.id,
                "peer",
                Some(&self_id),
                content,
                "text",
                timestamp,
                status,
                id,
            )
            .await
            .unwrap();
        }
        let names = devices
            .iter()
            .map(|device| (device.id.clone(), device.name.clone()))
            .collect();
        let expected = conversation_view(&pool, record.clone(), &names, &self_id)
            .await
            .unwrap();
        let batch = conversation_views(&pool, &devices, &self_id)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(
            serde_json::to_value(batch).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        sqlx::query("INSERT INTO messages(sender_id, receiver_id, content, msg_type, timestamp, status) VALUES ('peer', ?, 'legacy latest', 'text', 30, 'received')")
            .bind(&self_id).execute(&pool).await.unwrap();
        let legacy = conversation_views(&pool, &devices, &self_id)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(legacy.last_message, "legacy latest");
        assert_eq!(legacy.last_message_at, 30);
        let current = get_sync(&pool, &manager, Some(&replay.cursor))
            .await
            .unwrap();
        assert!(current.changes.contains_key("conversations"));
        assert!(
            !current.changes.contains_key("files"),
            "text messages do not invalidate the file center"
        );
        pool.close().await;
        crate::db::remove_test_database(&pool, &root).await;
    }
}

pub async fn get_sync(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    cursor: Option<&str>,
) -> Result<WorkspaceSync, String> {
    // Capture BEFORE reading collections: a concurrent commit is replayed on the
    // next poll instead of being hidden behind a cursor newer than its data.
    let row = sqlx::query("SELECT * FROM workspace_revisions WHERE id = 1")
        .fetch_one(pool)
        .await
        .map_err(|error| error.to_string())?;
    let current = Cursor {
        epoch: PROCESS_EPOCH.clone(),
        database: row.get("instance"),
        baseline_at: now(),
        versions: [
            row.get("conversations"),
            row.get("devices"),
            row.get("files"),
            row.get("transfers"),
            row.get("settings"),
        ],
        peers: peer_revision(manager),
        processing: crate::network::conversation_file::receive_processing_revision(),
    };
    let previous = cursor
        .filter(|value| value.len() <= 2048)
        .and_then(|value| serde_json::from_str::<Cursor>(value).ok())
        .filter(|old| {
            old.epoch == current.epoch
                && old.database == current.database
                && old.baseline_at <= current.baseline_at
                && current.baseline_at - old.baseline_at < CORRECTION_SECONDS
                && old
                    .versions
                    .iter()
                    .zip(current.versions)
                    .all(|(old, current)| *old >= 0 && *old <= current)
        });
    let reset = previous
        .as_ref()
        .is_none_or(|old| old.versions[4] != current.versions[4]);
    let mut changes = serde_json::Map::new();
    let mut next = current;
    if reset {
        changes = serde_json::to_value(get_snapshot(pool, manager).await?)
            .map_err(|e| e.to_string())?
            .as_object()
            .cloned()
            .ok_or("工作区快照格式错误")?;
    } else if let Some(old) = previous {
        next.baseline_at = old.baseline_at;
        let conversations_changed = old.versions[0] != next.versions[0];
        let devices_changed = old.versions[1] != next.versions[1] || old.peers != next.peers;
        if conversations_changed || devices_changed {
            let self_id = db::get_user_id(pool).await?;
            let devices = devices(pool, manager, &self_id).await?;
            if conversations_changed {
                changes.insert(
                    "conversations".into(),
                    serde_json::to_value(conversation_views(pool, &devices, &self_id).await?)
                        .map_err(|e| e.to_string())?,
                );
            }
            if devices_changed {
                changes.insert(
                    "devices".into(),
                    serde_json::to_value(devices).map_err(|e| e.to_string())?,
                );
            }
        }
        if old.versions[2] != next.versions[2] {
            changes.insert(
                "files".into(),
                serde_json::to_value(
                    message_views(pool, manager, db::list_file_messages(pool, 500, 0).await?)
                        .await?,
                )
                .map_err(|e| e.to_string())?,
            );
        }
        if old.versions[3] != next.versions[3] || old.processing != next.processing {
            changes.insert(
                "transfers".into(),
                serde_json::to_value(transfers(pool).await?).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(WorkspaceSync {
        cursor: serde_json::to_string(&next).map_err(|e| e.to_string())?,
        reset,
        changes,
    })
}

pub(super) async fn conversation_views(
    pool: &Pool<Sqlite>,
    devices: &[WorkspaceDevice],
    self_id: &str,
) -> Result<Vec<WorkspaceConversation>, String> {
    let mut records = db::list_conversations(pool).await?;
    let known = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<BTreeSet<_>>();
    let mut added = false;
    for device in devices {
        let id = db::stable_direct_conversation_id(self_id, &device.id);
        if !known.contains(&id) {
            db::ensure_direct_conversation(pool, &device.id).await?;
            added = true;
        }
    }
    if added {
        records = db::list_conversations(pool).await?;
    }
    // The historical reader also hydrates legacy messages without conversation_id.
    // Keep that migration/preview behavior until those rows have been consumed.
    let legacy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE conversation_id IS NULL AND COALESCE(status, '') != 'recalled')")
        .fetch_one(pool).await.map_err(|e| e.to_string())?;
    if legacy {
        let names = devices
            .iter()
            .map(|device| {
                (
                    device.id.clone(),
                    device.remark.clone().unwrap_or_else(|| device.name.clone()),
                )
            })
            .collect();
        let mut views = Vec::new();
        for record in records {
            views.push(conversation_view(pool, record, &names, self_id).await?);
        }
        return Ok(views);
    }
    let mut members: HashMap<String, Vec<ConversationMemberRecord>> = HashMap::new();
    for member in sqlx::query_as::<_, ConversationMemberRecord>("SELECT conversation_id, peer_id, display_name, role, joined_at FROM conversation_members ORDER BY joined_at, peer_id")
        .fetch_all(pool).await.map_err(|e| e.to_string())? {
        members.entry(member.conversation_id.clone()).or_default().push(member);
    }
    let stats = sqlx::query_as::<_, (String, String, i64, i64)>("SELECT c.id, COALESCE(m.content, ''), COALESCE(m.timestamp, c.updated_at),
        (SELECT COUNT(*) FROM messages u WHERE u.conversation_id = c.id AND u.sender_id NOT IN ('me', ?)
            AND u.client_message_id IS NOT NULL AND COALESCE(u.status, '') != 'recalled' AND COALESCE(u.status, 'received') != 'read')
        FROM conversations c LEFT JOIN messages m ON m.id =
            (SELECT id FROM messages WHERE conversation_id = c.id AND COALESCE(status, '') != 'recalled' ORDER BY timestamp DESC, id DESC LIMIT 1)")
        .bind(self_id).fetch_all(pool).await.map_err(|e| e.to_string())?
        .into_iter().map(|(id, content, timestamp, unread)| (id, (content, timestamp, unread))).collect::<HashMap<_, _>>();
    let names = devices
        .iter()
        .map(|device| {
            (
                device.id.clone(),
                device.remark.clone().unwrap_or_else(|| device.name.clone()),
            )
        })
        .collect::<HashMap<_, _>>();
    let self_name = db::get_username(pool).await?;
    Ok(records
        .into_iter()
        .map(|record| {
            let (last_message, last_message_at, unread) = stats
                .get(&record.id)
                .cloned()
                .unwrap_or((String::new(), record.updated_at, 0));
            let mut members = members.remove(&record.id).unwrap_or_default();
            for member in &mut members {
                if let Some(name) = names.get(&member.peer_id) {
                    member.display_name = name.clone();
                } else if record.kind == "direct" && member.peer_id == self_id {
                    member.display_name = self_name.clone();
                }
            }
            WorkspaceConversation {
                title: record
                    .peer_id
                    .as_ref()
                    .and_then(|id| names.get(id))
                    .cloned()
                    .or(record.title)
                    .unwrap_or_else(|| "未命名会话".into()),
                id: record.id,
                kind: record.kind,
                peer_id: record.peer_id,
                created_by: record.created_by,
                pinned: record.pinned,
                forced_unread: record.forced_unread,
                draft: record.draft,
                unread_count: unread.max(i64::from(record.forced_unread)),
                last_message,
                last_message_at,
                members,
                version: record.version,
            }
        })
        .collect())
}
