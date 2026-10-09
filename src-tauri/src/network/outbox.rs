//! Bounded, due-driven recovery. Discovery/reconnection remains a fast wakeup.
use crate::{peers::PeerManager, workspace};
use sqlx::{Pool, Sqlite};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct ScheduleState {
    due_after: String,
    maintenance_after: String,
}

const SEED_ATTEMPTS: &str = "INSERT OR IGNORE INTO message_delivery_attempts(message_client_id, reader_id)
    SELECT r.message_client_id, r.reader_id FROM message_receipts r JOIN messages m ON m.client_message_id = r.message_client_id
    WHERE r.delivered_at IS NULL AND r.read_at IS NULL AND m.msg_type IN ('text', 'quote', 'announcement')
      AND COALESCE(m.status, '') NOT IN ('recalled', 'delivered', 'read')
      AND m.sender_id IN ('me', (SELECT value FROM settings WHERE key = 'user_id'))";

pub(crate) async fn init_schema(pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pragma_table_info('message_receipts') WHERE name = 'ack_peer_id')")
        .fetch_one(pool).await?;
    if !exists {
        if let Err(error) = sqlx::query("ALTER TABLE message_receipts ADD COLUMN ack_peer_id TEXT")
            .execute(pool)
            .await
        {
            let migrated: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pragma_table_info('message_receipts') WHERE name = 'ack_peer_id')")
                .fetch_one(pool).await?;
            if !migrated {
                return Err(error);
            }
        }
    }
    for sql in [
        "CREATE INDEX IF NOT EXISTS idx_delivery_due ON message_delivery_attempts(next_retry_at, lease_until, reader_id) WHERE state != 'delivered' AND state != 'cancelled'",
        "CREATE INDEX IF NOT EXISTS idx_receipts_pending_ack ON message_receipts(ack_peer_id, message_client_id) WHERE ack_peer_id IS NOT NULL AND ((delivered_at IS NOT NULL AND delivery_ack_sent_at IS NULL) OR (read_at IS NOT NULL AND read_ack_sent_at IS NULL))",
        "CREATE INDEX IF NOT EXISTS idx_receipts_pending_recall ON message_receipts(reader_id, message_client_id) WHERE recall_requested_at IS NOT NULL AND recall_sent_at IS NULL",
        "CREATE INDEX IF NOT EXISTS idx_transfers_waiting_peer ON transfers(peer_id) WHERE direction = 'send' AND status = 'waiting_peer'",
        "CREATE TRIGGER IF NOT EXISTS enqueue_delivery_attempt AFTER INSERT ON message_receipts
         WHEN NEW.delivered_at IS NULL AND NEW.read_at IS NULL
         BEGIN INSERT OR IGNORE INTO message_delivery_attempts(message_client_id, reader_id)
            SELECT NEW.message_client_id, NEW.reader_id FROM messages m
            WHERE m.client_message_id = NEW.message_client_id AND m.msg_type IN ('text', 'quote', 'announcement')
              AND COALESCE(m.status, '') NOT IN ('recalled', 'delivered', 'read')
              AND m.sender_id IN ('me', (SELECT value FROM settings WHERE key = 'user_id')); END",
        "CREATE TRIGGER IF NOT EXISTS route_pending_ack AFTER INSERT ON message_receipts
         BEGIN UPDATE message_receipts SET ack_peer_id = (SELECT sender_id FROM messages
              WHERE client_message_id = NEW.message_client_id AND sender_id NOT IN ('me', (SELECT value FROM settings WHERE key = 'user_id')))
              WHERE message_client_id = NEW.message_client_id AND reader_id = NEW.reader_id; END",
        "UPDATE message_receipts SET ack_peer_id = (SELECT sender_id FROM messages WHERE client_message_id = message_receipts.message_client_id)
         WHERE ack_peer_id IS NULL AND EXISTS(SELECT 1 FROM messages WHERE client_message_id = message_receipts.message_client_id
              AND sender_id NOT IN ('me', (SELECT value FROM settings WHERE key = 'user_id')))",
        "CREATE TRIGGER IF NOT EXISTS retire_recalled_attempts AFTER UPDATE OF status ON messages WHEN NEW.status = 'recalled'
         BEGIN UPDATE message_delivery_attempts SET state = 'cancelled', lease_token = NULL, lease_until = 0, next_retry_at = 0
            WHERE message_client_id = NEW.client_message_id; END",
        "UPDATE message_delivery_attempts SET state = 'cancelled', lease_token = NULL, lease_until = 0, next_retry_at = 0
         WHERE state != 'cancelled' AND EXISTS(SELECT 1 FROM messages WHERE client_message_id = message_delivery_attempts.message_client_id AND status = 'recalled')",
        SEED_ATTEMPTS,
    ] {
        sqlx::query(sql).execute(pool).await?;
    }
    Ok(())
}

const DUE_PEERS: &str = "SELECT peer_id FROM (
    SELECT a.reader_id AS peer_id FROM message_delivery_attempts a
    JOIN message_receipts r ON r.message_client_id = a.message_client_id AND r.reader_id = a.reader_id
    JOIN messages m ON m.client_message_id = a.message_client_id
    WHERE a.state != 'delivered' AND a.state != 'cancelled' AND a.next_retry_at <= ? AND a.lease_until <= ?
      AND r.delivered_at IS NULL AND r.read_at IS NULL
      AND COALESCE(m.status, '') NOT IN ('recalled', 'delivered', 'read')
      AND m.msg_type IN ('text', 'quote', 'announcement')
    UNION
    SELECT r.ack_peer_id FROM message_receipts r JOIN messages m ON m.client_message_id = r.message_client_id
    WHERE r.ack_peer_id IS NOT NULL AND m.conversation_id IS NOT NULL AND ((r.delivered_at IS NOT NULL AND r.delivery_ack_sent_at IS NULL)
       OR (r.read_at IS NOT NULL AND r.read_ack_sent_at IS NULL))
    UNION
    SELECT r.reader_id FROM message_receipts r JOIN messages m ON m.client_message_id = r.message_client_id
    WHERE m.status = 'recalled' AND r.recall_requested_at IS NOT NULL AND r.recall_sent_at IS NULL
    UNION
    SELECT t.peer_id FROM transfers t JOIN users u ON u.id = t.peer_id
    WHERE t.direction = 'send' AND t.status = 'waiting_peer' AND u.is_offline = 0
) ORDER BY (peer_id <= ?), peer_id LIMIT ?";

pub(crate) async fn due_peers(
    pool: &Pool<Sqlite>,
    after: &str,
    limit: i64,
) -> Result<Vec<String>, String> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query_scalar(DUE_PEERS)
        .bind(now)
        .bind(now)
        .bind(after)
        .bind(limit.clamp(1, 32))
        .fetch_all(pool)
        .await
        .map_err(|error| format!("读取到期队列失败: {error}"))
}

pub(crate) async fn schedule_due(
    pool: &Pool<Sqlite>,
    manager: &PeerManager,
    tick: u32,
) -> Result<(), String> {
    let after = manager
        .outbox
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .due_after
        .clone();
    let candidates = due_peers(pool, &after, 32).await?;
    let peers = manager
        .get_all_peers()
        .into_iter()
        .map(|peer| (peer.id.clone(), peer))
        .collect::<BTreeMap<_, _>>();
    for id in candidates {
        if let Some(peer) = peers.get(&id) {
            if !workspace::schedule_resend(pool, manager, &id, &peer.addr) {
                break;
            }
        }
        manager
            .outbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .due_after = id;
    }
    // Rotate a bounded maintenance batch. No per-contact tasks on idle sweeps.
    // Announcements and user sends still validate a connection immediately.
    if tick % 15 == 0 {
        let after = manager
            .outbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .maintenance_after
            .clone();
        let mut online = peers
            .values()
            .filter(|peer| !peer.is_offline)
            .collect::<Vec<_>>();
        online.sort_by_key(|peer| (peer.id <= after, &peer.id));
        for peer in online.into_iter().take(16) {
            super::peer_connection::schedule_validation(pool, manager, &peer.id);
            if !workspace::schedule_resend(pool, manager, &peer.id, &peer.addr) {
                break;
            }
            manager
                .outbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .maintenance_after = peer.id.clone();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn only_due_work_is_selected_with_rotation_and_indexed_attempts() {
        let root =
            std::env::temp_dir().join(format!("xchat-phase2-outbox-{}", uuid::Uuid::new_v4()));
        let pool = db::init_db_standalone(Some(root.clone())).await.unwrap();
        sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<3000)
            INSERT INTO users(id,name,addr,last_seen,is_offline) SELECT printf('peer-%04d',x), 'Offline', '127.0.0.1:9', 1, 1 FROM n").execute(&pool).await.unwrap();
        assert!(due_peers(&pool, "", 32).await.unwrap().is_empty());
        let self_id = db::get_user_id(&pool).await.unwrap();
        for index in 1..=4 {
            let peer = format!("peer-{index:04}");
            let id = format!("outbox-{index}");
            let conversation = db::ensure_direct_conversation(&pool, &peer).await.unwrap();
            db::save_conversation_message(
                &pool,
                &conversation.id,
                &self_id,
                Some(&peer),
                "pending",
                "text",
                1,
                "pending",
                &id,
            )
            .await
            .unwrap();
            sqlx::query("INSERT INTO message_receipts(message_client_id,reader_id,updated_at) VALUES (?,?,1)")
                .bind(&id).bind(&peer).execute(&pool).await.unwrap();
        }
        assert_eq!(
            db::get_message_delivery_attempts(&pool, "outbox-1")
                .await
                .unwrap()
                .len(),
            1,
            "receipt and durable due task must be atomic"
        );
        sqlx::query("UPDATE message_delivery_attempts SET next_retry_at = 9223372036854775807 WHERE reader_id = 'peer-0002'").execute(&pool).await.unwrap();
        sqlx::query("UPDATE message_delivery_attempts SET lease_until = 9223372036854775807 WHERE reader_id = 'peer-0003'").execute(&pool).await.unwrap();
        assert_eq!(
            due_peers(&pool, "", 32).await.unwrap(),
            ["peer-0001", "peer-0004"]
        );
        assert_eq!(
            due_peers(&pool, "peer-0001", 1).await.unwrap(),
            ["peer-0004"]
        );
        assert_eq!(
            due_peers(&pool, "peer-0004", 1).await.unwrap(),
            ["peer-0001"]
        );
        sqlx::query("UPDATE message_receipts SET delivered_at = 2, delivery_ack_sent_at = 2 WHERE message_client_id = 'outbox-1'").execute(&pool).await.unwrap();
        assert_eq!(due_peers(&pool, "", 32).await.unwrap(), ["peer-0004"]);
        let incoming = db::ensure_direct_conversation(&pool, "peer-0003")
            .await
            .unwrap();
        db::save_conversation_message(
            &pool,
            &incoming.id,
            "peer-0003",
            Some(&self_id),
            "incoming",
            "text",
            2,
            "received",
            "incoming-ack",
        )
        .await
        .unwrap();
        db::save_message_receipt(&pool, "incoming-ack", &self_id, Some(2), None)
            .await
            .unwrap();
        assert_eq!(
            due_peers(&pool, "", 32).await.unwrap(),
            ["peer-0003", "peer-0004"]
        );
        db::mark_receipt_ack_sent(&pool, "incoming-ack", &self_id, "delivery")
            .await
            .unwrap();
        assert_eq!(due_peers(&pool, "", 32).await.unwrap(), ["peer-0004"]);
        sqlx::query("UPDATE messages SET status = 'recalled' WHERE client_message_id = 'outbox-2'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE message_receipts SET recall_requested_at = 3 WHERE message_client_id = 'outbox-2'").execute(&pool).await.unwrap();
        assert_eq!(
            due_peers(&pool, "", 32).await.unwrap(),
            ["peer-0002", "peer-0004"]
        );
        db::mark_recall_sent(&pool, "outbox-2", "peer-0002")
            .await
            .unwrap();
        db::create_transfer(
            &pool,
            "waiting-file",
            None,
            &incoming.id,
            "peer-0003",
            "send",
            "waiting_peer",
            100,
        )
        .await
        .unwrap();
        assert_eq!(due_peers(&pool, "", 32).await.unwrap(), ["peer-0004"]);
        sqlx::query("UPDATE users SET is_offline = 0 WHERE id = 'peer-0003'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            due_peers(&pool, "", 32).await.unwrap(),
            ["peer-0003", "peer-0004"]
        );
        let plans = sqlx::query_as::<_, (i64, i64, i64, String)>(&format!(
            "EXPLAIN QUERY PLAN {DUE_PEERS}"
        ))
        .bind(1)
        .bind(1)
        .bind("")
        .bind(32)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            plans.iter().any(|row| row.3.contains("idx_delivery_due")),
            "{plans:?}"
        );
        pool.close().await;
        crate::db::remove_test_database(&pool, &root).await;
    }
}
