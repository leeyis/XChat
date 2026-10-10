use super::*;

async fn setup() -> (PathBuf, Pool<Sqlite>) {
    let root = std::env::temp_dir().join(format!("xchat-backup-test-{}", uuid::Uuid::new_v4()));
    let pool = db::init_db_standalone(Some(root.clone())).await.unwrap();
    db::update_download_path(&pool, root.join("downloads").to_string_lossy().into())
        .await
        .unwrap();
    (root, pool)
}

async fn seed(pool: &Pool<Sqlite>, id: &str, kind: &str, content: &str) -> i64 {
    let me = db::get_user_id(pool).await.unwrap();
    let conversation = db::ensure_direct_conversation(pool, "backup-peer")
        .await
        .unwrap();
    sqlx::query("INSERT INTO messages(client_message_id,sender_id,receiver_id,content,msg_type,timestamp,status,conversation_id) VALUES(?,?,'backup-peer',?,?,1,'pending',?)")
        .bind(id).bind(me).bind(content).bind(kind).bind(conversation.id).execute(pool).await.unwrap().last_insert_rowid()
}

#[tokio::test]
async fn checked_merge_preserves_current_data_identity_and_never_resends_history() {
    let (root, source) = setup().await;
    let id = "backup-pending-text";
    let message = seed(&source, id, "text", "private original").await;
    db::ensure_message_recipients(&source, id, &["backup-peer".into()])
        .await
        .unwrap();
    let voice_id = "backup-voice";
    let file = seed(&source, voice_id, "voice", "语音.webm").await;
    let path = root.join("downloads/audio.webm");
    tokio::fs::write(&path, b"voice attachment").await.unwrap();
    sqlx::query("UPDATE messages SET file_path=?,file_status='accepted',file_size=16 WHERE id=?")
        .bind(path.to_string_lossy().as_ref())
        .bind(file)
        .execute(&source)
        .await
        .unwrap();
    crate::voice::save_on(
        &mut *source.acquire().await.unwrap(),
        voice_id,
        &crate::voice::VoiceMetadata {
            duration_ms: 2000,
            mime_type: "audio/webm".into(),
        },
    )
    .await
    .unwrap();
    db::set_setting(&source, "current_theme", "dark")
        .await
        .unwrap();
    db::set_setting(&source, "capture_shortcut", "DO NOT MOVE")
        .await
        .unwrap();
    let job = Job::new(
        database_path(&source)
            .await
            .unwrap()
            .to_string_lossy()
            .into(),
        "create",
    );
    let result = create_archive(
        &source,
        &CreateRequest {
            include_attachments: true,
            include_settings: true,
            directory: None,
        },
        &job,
    )
    .await
    .unwrap();
    let backup = PathBuf::from(result["path"].as_str().unwrap());
    // Add current data after the snapshot. A duplicate must retain the current contents.
    sqlx::query("UPDATE messages SET content='current wins' WHERE id=?")
        .bind(message)
        .execute(&source)
        .await
        .unwrap();
    let (local, target) = setup().await;
    let self_id = db::get_user_id(&target).await.unwrap();
    seed(&target, id, "text", "current wins").await;
    let pending_id = "second-pending";
    seed(&source, pending_id, "text", "never dispatch").await;
    db::ensure_message_recipients(&source, pending_id, &["backup-peer".into()])
        .await
        .unwrap();
    // A second archive includes an actually pending message absent from the target.
    let second = Job::new(job.scope.clone(), "create");
    let result = create_archive(
        &source,
        &CreateRequest {
            include_attachments: true,
            include_settings: true,
            directory: None,
        },
        &second,
    )
    .await
    .unwrap();
    let second_path = PathBuf::from(result["path"].as_str().unwrap());
    let preview = Job::new(
        database_path(&target)
            .await
            .unwrap()
            .to_string_lossy()
            .into(),
        "restore",
    );
    restore::prepare(&target, &second_path, &preview)
        .await
        .unwrap();
    assert_eq!(preview.view().result.unwrap()["duplicates"], 1);
    let prepared = preview.prepared.lock().unwrap().take().unwrap();
    let restored = restore::merge(&target, &prepared, &preview, false)
        .await
        .unwrap();
    assert_eq!(restored["added"], 2);
    assert!(Path::new(restored["pre_restore_copy"].as_str().unwrap()).is_file());
    assert_eq!(db::get_user_id(&target).await.unwrap(), self_id);
    assert_eq!(
        db::get_message_by_client_id(&target, id)
            .await
            .unwrap()
            .unwrap()
            .content,
        "current wins"
    );
    let history = db::get_message_by_client_id(&target, pending_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(history.status.as_deref(), Some("restored"));
    assert_eq!(history.sender_id, self_id);
    // The durable history guard survives later status transitions or stale callbacks.
    sqlx::query("UPDATE messages SET status='pending' WHERE client_message_id=?")
        .bind(pending_id)
        .execute(&target)
        .await
        .unwrap();
    assert!(
        db::claim_message_delivery(&target, pending_id, "backup-peer", true)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!db::get_due_messages_for_peer(&target, "backup-peer")
        .await
        .unwrap()
        .iter()
        .any(|m| m.client_message_id.as_deref() == Some(pending_id)));
    let voice = db::get_message_by_client_id(&target, voice_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tokio::fs::read(voice.file_path.unwrap()).await.unwrap(),
        b"voice attachment"
    );
    assert_eq!(
        crate::voice::metadata(&target, voice_id)
            .await
            .unwrap()
            .unwrap()
            .duration_ms,
        2000
    );
    assert_ne!(
        db::get_setting(&target, "capture_shortcut")
            .await
            .unwrap()
            .as_deref(),
        Some("DO NOT MOVE")
    );
    assert_ne!(
        db::get_setting(&target, "current_theme")
            .await
            .unwrap()
            .as_deref(),
        Some("dark")
    );
    assert!(backup.is_file());
    let opt_in = Job::new(preview.scope.clone(), "restore");
    restore::prepare(&target, &second_path, &opt_in)
        .await
        .unwrap();
    let prepared_settings = opt_in.prepared.lock().unwrap().take().unwrap();
    let settings_result = restore::merge(&target, &prepared_settings, &opt_in, true)
        .await
        .unwrap();
    assert_eq!(settings_result["added"], 0);
    assert_eq!(
        db::get_setting(&target, "current_theme")
            .await
            .unwrap()
            .as_deref(),
        Some("dark")
    );
    assert_eq!(db::get_user_id(&target).await.unwrap(), self_id);
    source.close().await;
    target.close().await;
    for path in [root, local] {
        assert!(path.starts_with(std::env::temp_dir()));
        tokio::fs::remove_dir_all(path).await.unwrap();
    }
}

#[tokio::test]
async fn online_snapshot_allows_writes_between_batches_and_finishes_consistently() {
    let (root, pool) = setup().await;
    sqlx::query("CREATE TABLE snapshot_payload(data BLOB)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO snapshot_payload VALUES(zeroblob(33554432))")
        .execute(&pool)
        .await
        .unwrap();
    let job = Arc::new(Job::new(
        database_path(&pool).await.unwrap().to_string_lossy().into(),
        "create",
    ));
    let destination = root.join("online.sqlite");
    let snapshot_pool = pool.clone();
    let snapshot_path = destination.clone();
    let snapshot_job = job.clone();
    let copying =
        tokio::spawn(async move { snapshot(&snapshot_pool, &snapshot_path, &snapshot_job).await });
    // Wait for a completed page batch; writes before the worker starts do not prove concurrency.
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while job.view().completed_bytes == 0 && !copying.is_finished() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let mut concurrent = 0;
    for value in 0..20 {
        db::set_setting(&pool, "qa_write_during_backup", &value.to_string())
            .await
            .unwrap();
        if !copying.is_finished() {
            concurrent += 1;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    copying.await.unwrap().unwrap();
    assert!(
        concurrent > 0,
        "ordinary writes must succeed while copying pages"
    );
    let mut copied = records::connect(&destination, false).await.unwrap();
    let bytes: i64 = sqlx::query_scalar("SELECT length(data) FROM snapshot_payload")
        .fetch_one(&mut copied)
        .await
        .unwrap();
    assert_eq!(bytes, 33554432);
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check(1)")
        .fetch_one(&mut copied)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    sqlx::Connection::close(copied).await.unwrap();
    pool.close().await;
    assert!(root.starts_with(std::env::temp_dir()));
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn corrupt_cancelled_and_failed_restore_leave_live_records_unchanged() {
    let (root, pool) = setup().await;
    let id = "rollback-record";
    seed(&pool, id, "text", "safe original").await;
    let job = Job::new(
        database_path(&pool).await.unwrap().to_string_lossy().into(),
        "create",
    );
    let result = create_archive(
        &pool,
        &CreateRequest {
            include_attachments: false,
            include_settings: false,
            directory: None,
        },
        &job,
    )
    .await
    .unwrap();
    let archive = PathBuf::from(result["path"].as_str().unwrap());
    let broken = root.join("broken.xchatbackup");
    let mut bytes = tokio::fs::read(&archive).await.unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    tokio::fs::write(&broken, &bytes).await.unwrap();
    let bad = Job::new(job.scope.clone(), "restore");
    assert!(restore::prepare(&pool, &broken, &bad).await.is_err());
    assert!(bad.prepared.lock().unwrap().is_none());
    let mut header_corruption = tokio::fs::read(&archive).await.unwrap();
    let offset = header_corruption
        .windows(11)
        .position(|bytes| bytes == b"app_version")
        .unwrap()
        + 14;
    header_corruption[offset] ^= 1;
    tokio::fs::write(&broken, &header_corruption).await.unwrap();
    let bad_header = Job::new(job.scope.clone(), "restore");
    assert!(restore::prepare(&pool, &broken, &bad_header)
        .await
        .unwrap_err()
        .to_string()
        .contains("清单摘要"));
    let cancelled = Job::new(job.scope.clone(), "create");
    cancelled.cancelled.store(true, Ordering::Release);
    assert!(create_archive(
        &pool,
        &CreateRequest {
            include_attachments: false,
            include_settings: false,
            directory: None
        },
        &cancelled
    )
    .await
    .is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_records")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    // Simulate a transactional write failure after some conversation inserts.
    sqlx::query("DELETE FROM messages WHERE client_message_id=?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    seed(&pool, "retain-this", "text", "keep me").await;
    sqlx::query("CREATE TRIGGER reject_restore BEFORE INSERT ON messages WHEN NEW.client_message_id='rollback-record' BEGIN SELECT RAISE(ABORT,'injected failure'); END").execute(&pool).await.unwrap();
    let restore_job = Job::new(job.scope.clone(), "restore");
    restore::prepare(&pool, &archive, &restore_job)
        .await
        .unwrap();
    let prepared = restore_job.prepared.lock().unwrap().take().unwrap();
    assert!(restore::merge(&pool, &prepared, &restore_job, false)
        .await
        .is_err());
    assert!(db::get_message_by_client_id(&pool, id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        db::get_message_by_client_id(&pool, "retain-this")
            .await
            .unwrap()
            .unwrap()
            .content,
        "keep me"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    pool.close().await;
    assert!(root.starts_with(std::env::temp_dir()));
    tokio::fs::remove_dir_all(root).await.unwrap();
}
