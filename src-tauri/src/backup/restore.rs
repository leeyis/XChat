use super::{archive, records, Job, Prepared, Result};
use crate::db;
use sqlx::{Connection, Pool, Sqlite};
use std::path::Path;

pub(super) async fn prepare(pool: &Pool<Sqlite>, source: &Path, job: &Job) -> Result<()> {
    let root = super::root(pool).await?;
    tokio::fs::create_dir_all(&root).await?;
    let stage = root.join(format!("restore-{}", job.view().id));
    tokio::fs::create_dir(&stage).await?;
    let result=async{
        let verified=archive::verify(source,Some(&stage),job).await?;
        records::validate(&verified.files[0],&verified.manifest).await?;
        let mut connection=pool.acquire().await?;
        sqlx::query("ATTACH DATABASE ? AS xchat_restore").bind(verified.files[0].to_string_lossy().as_ref()).execute(&mut *connection).await?;
        let count=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM xchat_restore.messages b WHERE EXISTS(SELECT 1 FROM main.messages m WHERE m.client_message_id=b.client_message_id)").fetch_one(&mut *connection).await;
        if sqlx::query("DETACH DATABASE xchat_restore").execute(&mut *connection).await.is_err(){connection.close().await?;}
        let duplicate=count?;
        let mut state=job.state.lock().unwrap_or_else(|e|e.into_inner());job.check_cancelled()?;
        state.status="preview".into();state.phase="备份完整，可以恢复".into();
        state.result=Some(serde_json::json!({"created_at":verified.manifest.created_at,"messages":verified.manifest.messages,
            "new_messages":verified.manifest.messages.saturating_sub(duplicate as u64),"duplicates":duplicate,"attachments":verified.files.len()-1,"settings":verified.manifest.settings}));
        *job.prepared.lock().unwrap_or_else(|e|e.into_inner())=Some(Prepared{root:stage.clone(),verified});
        Ok(())
    }.await;
    if result.is_err() {
        super::cleanup(&stage).await;
    }
    result
}

pub(super) async fn merge(
    pool: &Pool<Sqlite>,
    prepared: &Prepared,
    job: &Job,
    settings: bool,
) -> Result<serde_json::Value> {
    // Confirmation operates on our verified copy, independent of the originally selected file.
    for (entry, path) in prepared
        .verified
        .manifest
        .entries
        .iter()
        .zip(&prepared.verified.files)
    {
        let actual = archive::hash_file(path, job).await?;
        if actual.bytes != entry.bytes || actual.sha256 != entry.sha256 {
            return Err("校验后的暂存内容发生变化，请重新选择备份".into());
        }
    }
    records::validate(&prepared.verified.files[0], &prepared.verified.manifest).await?;
    let source_id = &prepared.verified.manifest.source_id;
    let self_id = db::get_user_id(pool).await?;
    let pre_backup = super::root(pool)
        .await?
        .join(format!("before-restore-{}.sqlite", job.view().id));
    super::snapshot(pool, &pre_backup, job).await?;
    let destination = std::path::PathBuf::from(db::get_download_path(pool).await?)
        .join(".xchat-restored-history")
        .join(job.view().id);
    super::require_space(
        &destination,
        prepared
            .verified
            .manifest
            .entries
            .iter()
            .skip(1)
            .map(|entry| entry.bytes)
            .sum(),
    )
    .await?;
    tokio::fs::create_dir_all(&destination).await?;
    let result=async{
        job.phase("正在恢复本地附件",prepared.verified.manifest.entries.iter().skip(1).map(|entry|entry.bytes).sum());
        let mut copied=Vec::new();
        let mut archive_connection=records::connect(&prepared.verified.files[0],false).await?;
        for(index,(entry,path))in prepared.verified.manifest.entries.iter().zip(&prepared.verified.files).enumerate().skip(1){
            let key=entry.message_key.as_ref().ok_or("附件标识缺失")?;
            if db::get_message_by_client_id(pool,key).await?.is_some(){continue;}
            let content:String=sqlx::query_scalar("SELECT content FROM messages WHERE client_message_id=?").bind(key).fetch_one(&mut archive_connection).await?;
            let file_name=content.replace('\\',"/").rsplit('/').next().unwrap_or("attachment").chars()
                .filter(|ch|ch.is_alphanumeric()||matches!(ch,'.'|'-'|'_'|' ')).take(100).collect::<String>();
            let target=destination.join(format!("{index}-{}",if file_name.is_empty(){"attachment"}else{&file_name}));
            let actual=archive::copy_file(path,&target,job).await?;
            if actual.sha256!=entry.sha256{return Err("恢复附件的摘要校验失败".into());}
            copied.push((key.clone(),target));
        }
        archive_connection.close().await?;
        job.phase("正在合并聊天记录",0);
        let mut connection=pool.acquire().await?;
        sqlx::query("ATTACH DATABASE ? AS xchat_restore").bind(prepared.verified.files[0].to_string_lossy().as_ref()).execute(&mut *connection).await?;
        let merged=async{
            let mut tx=connection.begin().await?;
            for name in ["backup_conversation_map","backup_new_messages","backup_restored_files"]{
                sqlx::query(&format!("DROP TABLE IF EXISTS temp.{name}")).execute(&mut *tx).await?;
            }
            sqlx::query("CREATE TEMP TABLE backup_conversation_map AS WITH peers AS (
                SELECT id old_id,kind,CASE WHEN peer_id IN (?,'me') THEN ? ELSE peer_id END peer_id FROM xchat_restore.conversations),
                mapped AS (SELECT old_id,kind,peer_id,CASE WHEN kind='direct' THEN CASE WHEN ?<peer_id THEN 'direct:'||?||':'||peer_id ELSE 'direct:'||peer_id||':'||? END ELSE old_id END new_id FROM peers)
                SELECT *,NOT EXISTS(SELECT 1 FROM main.conversations c WHERE c.id=new_id) is_new FROM mapped")
                .bind(source_id).bind(&self_id).bind(&self_id).bind(&self_id).bind(&self_id).execute(&mut *tx).await?;
            sqlx::query("CREATE TEMP TABLE backup_new_messages AS SELECT client_message_id FROM xchat_restore.messages b WHERE NOT EXISTS(SELECT 1 FROM main.messages m WHERE m.client_message_id=b.client_message_id)").execute(&mut *tx).await?;
            sqlx::query("CREATE UNIQUE INDEX temp.backup_new_messages_key ON backup_new_messages(client_message_id)").execute(&mut *tx).await?;
            sqlx::query("CREATE TEMP TABLE backup_restored_files(client_message_id TEXT PRIMARY KEY,path TEXT NOT NULL)").execute(&mut *tx).await?;
            for(key,path)in &copied{sqlx::query("INSERT INTO backup_restored_files VALUES(?,?)").bind(key).bind(path.to_string_lossy().as_ref()).execute(&mut *tx).await?;}
            let added:i64=sqlx::query_scalar("SELECT COUNT(*) FROM backup_new_messages").fetch_one(&mut *tx).await?;
            sqlx::query("INSERT OR IGNORE INTO main.users(id,name,addr,last_seen,is_offline,remark) SELECT id,name,'',0,1,remark FROM xchat_restore.contacts WHERE id NOT IN (?,?,'me')")
                .bind(source_id).bind(&self_id).execute(&mut *tx).await?;
            sqlx::query("INSERT OR IGNORE INTO main.conversations(id,kind,peer_id,title,created_by,pinned,draft,created_at,updated_at,version)
                SELECT map.new_id,c.kind,map.peer_id,c.title,CASE WHEN c.created_by IN (?,'me') THEN ? ELSE c.created_by END,c.pinned,c.draft,c.created_at,c.updated_at,c.version
                FROM xchat_restore.conversations c JOIN backup_conversation_map map ON map.old_id=c.id")
                .bind(source_id).bind(&self_id).execute(&mut *tx).await?;
            // Old group membership must not resurrect members removed from a current conversation.
            sqlx::query("INSERT OR IGNORE INTO main.conversation_members(conversation_id,peer_id,display_name,role,joined_at)
                SELECT map.new_id,CASE WHEN m.peer_id IN (?,'me') THEN ? ELSE m.peer_id END,m.display_name,m.role,m.joined_at
                FROM xchat_restore.members m JOIN backup_conversation_map map ON map.old_id=m.conversation_id WHERE map.is_new=1")
                .bind(source_id).bind(&self_id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO main.messages(sender_id,receiver_id,content,msg_type,timestamp,file_path,file_status,file_size,status,conversation_id,client_message_id)
                SELECT CASE WHEN b.sender_id IN (?,'me') THEN ? ELSE b.sender_id END,CASE WHEN b.receiver_id IN (?,'me') THEN ? ELSE b.receiver_id END,
                b.content,b.msg_type,b.timestamp,f.path,CASE WHEN b.msg_type IN ('file','voice') THEN CASE WHEN f.path IS NULL THEN 'expired' ELSE 'accepted' END END,b.file_size,
                CASE WHEN b.status='recalled' THEN 'recalled' ELSE 'restored' END,COALESCE(map.new_id,b.conversation_id),b.client_message_id
                FROM xchat_restore.messages b JOIN backup_new_messages n ON n.client_message_id=b.client_message_id
                LEFT JOIN backup_conversation_map map ON map.old_id=b.conversation_id LEFT JOIN backup_restored_files f ON f.client_message_id=b.client_message_id")
                .bind(source_id).bind(&self_id).bind(source_id).bind(&self_id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO main.restored_messages SELECT client_message_id FROM backup_new_messages").execute(&mut *tx).await?;
            sqlx::query("INSERT INTO main.message_receipts(message_client_id,reader_id,mentioned,delivered_at,read_at,updated_at,delivery_ack_sent_at,read_ack_sent_at)
                SELECT r.message_client_id,CASE WHEN r.reader_id IN (?,'me') THEN ? ELSE r.reader_id END,r.mentioned,r.delivered_at,r.read_at,r.updated_at,?,?
                FROM xchat_restore.receipts r JOIN backup_new_messages n ON n.client_message_id=r.message_client_id")
                .bind(source_id).bind(&self_id).bind(chrono::Utc::now().timestamp()).bind(chrono::Utc::now().timestamp()).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO main.message_reactions(message_client_id,reactor_id,emoji,updated_at)
                SELECT r.message_client_id,CASE WHEN r.reactor_id IN (?,'me') THEN ? ELSE r.reactor_id END,r.emoji,r.updated_at
                FROM xchat_restore.reactions r JOIN backup_new_messages n ON n.client_message_id=r.message_client_id")
                .bind(source_id).bind(&self_id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO main.voice_metadata SELECT v.message_client_id,v.duration_ms,v.mime_type FROM xchat_restore.voice v JOIN backup_new_messages n ON n.client_message_id=v.message_client_id").execute(&mut *tx).await?;
            // Receipt triggers may enqueue attempts. Delete them before commit; the restored status
            // additionally prevents forced leases and reconnect dispatch from ever claiming history.
            sqlx::query("DELETE FROM main.message_delivery_attempts WHERE message_client_id IN (SELECT client_message_id FROM backup_new_messages)").execute(&mut *tx).await?;
            if settings&&prepared.verified.manifest.settings{
                sqlx::query(&format!("INSERT INTO main.settings(key,value) SELECT key,value FROM xchat_restore.preferences WHERE key IN ({}) ON CONFLICT(key) DO UPDATE SET value=excluded.value",records::PREFERENCES))
                    .execute(&mut *tx).await?;
            }
            let kept:Vec<String>=sqlx::query_scalar("SELECT client_message_id FROM backup_new_messages").fetch_all(&mut *tx).await?;
            for name in ["backup_conversation_map","backup_new_messages","backup_restored_files"]{
                sqlx::query(&format!("DROP TABLE temp.{name}")).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>((added,kept))
        }.await;
        if sqlx::query("DETACH DATABASE xchat_restore").execute(&mut *connection).await.is_err(){let _=connection.close().await;}
        let(added,kept)=merged?;
        let kept:std::collections::HashSet<_>=kept.into_iter().collect();
        for(key,path)in copied{if !kept.contains(&key){let _=tokio::fs::remove_file(path).await;}}
        Ok(serde_json::json!({"added":added,"duplicates":prepared.verified.manifest.messages.saturating_sub(added as u64),
            "preferences_restored":settings&&prepared.verified.manifest.settings,"pre_restore_copy":pre_backup}))
    }.await;
    if result.is_err() {
        super::cleanup(&destination).await;
    }
    result
}
