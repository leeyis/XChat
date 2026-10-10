//! Local backup jobs share the same core for Tauri and the web UI.
mod archive;
#[cfg(feature = "desktop")]
pub mod commands;
pub mod http;
mod records;
mod restore;
mod snapshot;
#[cfg(test)]
mod tests;

use crate::db;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const RESERVE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub include_attachments: bool,
    pub include_settings: bool,
    #[serde(default)]
    pub directory: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct JobView {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub phase: String,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub cancellable: bool,
    pub error: Option<String>,
    pub result: Option<serde_json::Value>,
}

pub(super) struct Job {
    scope: String,
    state: Arc<Mutex<JobView>>,
    cancelled: Arc<AtomicBool>,
    prepared: Mutex<Option<Prepared>>,
    created_at: i64,
}

struct Prepared {
    root: PathBuf,
    verified: archive::Verified,
}

impl Job {
    fn new(scope: String, kind: &str) -> Self {
        Self {
            scope,
            state: Arc::new(Mutex::new(JobView {
                id: uuid::Uuid::new_v4().to_string(),
                kind: kind.into(),
                status: "running".into(),
                phase: "正在准备".into(),
                completed_bytes: 0,
                total_bytes: 0,
                cancellable: true,
                error: None,
                result: None,
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
            prepared: Mutex::new(None),
            created_at: chrono::Utc::now().timestamp(),
        }
    }
    fn view(&self) -> JobView {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("本次操作已取消".into())
        } else {
            Ok(())
        }
    }
    fn phase(&self, phase: &str, total: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.phase = phase.into();
        state.completed_bytes = 0;
        state.total_bytes = total;
    }
    fn advance(&self, bytes: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.completed_bytes = state.completed_bytes.saturating_add(bytes);
    }
    fn commit_point(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.check_cancelled()?;
        state.cancellable = false;
        Ok(())
    }
    fn finish(&self, result: Result<serde_json::Value>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.cancellable = false;
        match result {
            Ok(value) => {
                state.status = "done".into();
                state.phase = "已完成".into();
                state.result = Some(value);
            }
            Err(error) => {
                state.status = if self.cancelled.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                state.error = Some(error.to_string());
            }
        }
    }
}

fn jobs() -> &'static Mutex<HashMap<String, Arc<Job>>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Arc<Job>>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn init_schema(pool: &Pool<Sqlite>) -> std::result::Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS restored_messages(message_client_id TEXT PRIMARY KEY REFERENCES messages(client_message_id) ON DELETE CASCADE)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS backup_records(id TEXT PRIMARY KEY,path TEXT NOT NULL,created_at INTEGER NOT NULL,bytes INTEGER NOT NULL,attachments INTEGER NOT NULL,settings INTEGER NOT NULL,sha256 TEXT NOT NULL)").execute(pool).await?;
    Ok(())
}

pub(crate) async fn is_history(pool: &Pool<Sqlite>, id: &str) -> std::result::Result<bool, String> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM restored_messages WHERE message_client_id=?)")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())
}

async fn database_path(pool: &Pool<Sqlite>) -> Result<PathBuf> {
    let rows = sqlx::query("PRAGMA database_list").fetch_all(pool).await?;
    let path = rows
        .into_iter()
        .find(|row| row.get::<String, _>("name") == "main")
        .ok_or("数据库路径不可用")?
        .get::<String, _>("file");
    if path.is_empty() {
        return Err("内存数据库不支持本地备份".into());
    }
    Ok(PathBuf::from(path))
}

async fn root(pool: &Pool<Sqlite>) -> Result<PathBuf> {
    Ok(database_path(pool)
        .await?
        .parent()
        .ok_or("数据库目录不可用")?
        .join("backups"))
}

pub(super) async fn require_space(path: &Path, bytes: u64) -> Result<()> {
    let available = crate::diagnostics::free_space(path.to_path_buf()).await?;
    if available.is_some_and(|free| free < bytes.saturating_add(RESERVE_BYTES)) {
        return Err(format!(
            "保存位置空间不足：至少需要 {} MiB（含安全余量）",
            bytes.saturating_add(RESERVE_BYTES) / 1024 / 1024
        )
        .into());
    }
    Ok(())
}

async fn snapshot(pool: &Pool<Sqlite>, destination: &Path, job: &Job) -> Result<()> {
    let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(pool)
        .await?;
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
        .fetch_one(pool)
        .await?;
    require_space(destination, (pages as u64).saturating_mul(page_size as u64)).await?;
    snapshot::copy(
        database_path(pool).await?,
        destination.to_owned(),
        job.cancelled.clone(),
        job.state.clone(),
        page_size as u64,
    )
    .await?;
    let mut connection = records::connect(destination, false).await?;
    let check: String = sqlx::query_scalar("PRAGMA integrity_check(1)")
        .fetch_one(&mut connection)
        .await?;
    if check != "ok" {
        return Err("一致快照完整性校验失败".into());
    }
    sqlx::Connection::close(connection).await?;
    Ok(())
}

async fn register(pool: &Pool<Sqlite>, kind: &str) -> Result<Arc<Job>> {
    let scope = database_path(pool).await?.to_string_lossy().into_owned();
    let mut registry = jobs().lock().unwrap_or_else(|e| e.into_inner());
    registry.retain(|_, job| {
        job.created_at > chrono::Utc::now().timestamp() - 3600
            || matches!(job.view().status.as_str(), "running" | "preview")
    });
    if registry.values().any(|job| {
        job.scope == scope && matches!(job.view().status.as_str(), "running" | "preview")
    }) {
        return Err("请先完成或取消当前备份/恢复操作".into());
    }
    let job = Arc::new(Job::new(scope, kind));
    registry.insert(job.view().id.clone(), job.clone());
    Ok(job)
}

async fn find_job(pool: &Pool<Sqlite>, id: &str) -> Result<Arc<Job>> {
    let scope = database_path(pool).await?.to_string_lossy().into_owned();
    jobs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .filter(|job| job.scope == scope)
        .cloned()
        .ok_or_else(|| "操作已过期，请重新开始".into())
}

pub async fn job_status(pool: &Pool<Sqlite>, id: &str) -> std::result::Result<JobView, String> {
    find_job(pool, id)
        .await
        .map(|job| job.view())
        .map_err(|e| e.to_string())
}

pub async fn cancel(pool: &Pool<Sqlite>, id: &str) -> std::result::Result<JobView, String> {
    let job = find_job(pool, id).await.map_err(|e| e.to_string())?;
    {
        let mut state = job.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.cancellable {
            return Ok(state.clone());
        }
        job.cancelled.store(true, Ordering::Release);
        state.phase = "正在取消，等待当前步骤结束".into();
        state.cancellable = false;
        if state.status == "preview" {
            state.status = "cancelled".into();
            state.cancellable = false;
        }
    }
    let prepared = job
        .prepared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(prepared) = prepared {
        cleanup(&prepared.root).await;
    }
    Ok(job.view())
}

// All callers pass a directory generated by this module, never a supplied archive entry.
async fn cleanup(path: &Path) {
    let _ = tokio::fs::remove_dir_all(path).await;
}

pub async fn overview(pool: &Pool<Sqlite>) -> std::result::Result<serde_json::Value, String> {
    async {
        let root=root(pool).await?;
        let source_id=db::get_user_id(pool).await?;
        let files=records::attachments(&database_path(pool).await?,&source_id).await?;
        let count: i64=sqlx::query_scalar("SELECT COUNT(*) FROM messages").fetch_one(pool).await?;
        let pages:i64=sqlx::query_scalar("PRAGMA page_count").fetch_one(pool).await?;
        let page_size:i64=sqlx::query_scalar("PRAGMA page_size").fetch_one(pool).await?;
        let rows=sqlx::query("SELECT * FROM backup_records ORDER BY created_at DESC LIMIT 100").fetch_all(pool).await?;
        let mut backups=Vec::new();
        for row in rows {
            let path=row.get::<String,_>("path");
            backups.push(serde_json::json!({"id":row.get::<String,_>("id"),"name":Path::new(&path).file_name().unwrap_or_default().to_string_lossy(),"path":path,
                "created_at":row.get::<i64,_>("created_at"),"bytes":row.get::<i64,_>("bytes"),"attachments":row.get::<bool,_>("attachments"),"settings":row.get::<bool,_>("settings"),
                "available":tokio::fs::metadata(&path).await.is_ok_and(|meta|meta.is_file())}));
        }
        let active=jobs().lock().unwrap_or_else(|e|e.into_inner()).values().find(|job|job.scope==database_path_string(&root)&&matches!(job.view().status.as_str(),"running"|"preview")).map(|job|job.view());
        Ok(serde_json::json!({"messages":count,"database_bytes":pages*page_size,"attachments":files.len(),"attachment_bytes":files.iter().map(|(_,_,size)|size).sum::<u64>(),
            "directory":root,"free_bytes":crate::diagnostics::free_space(root.clone()).await?,"backups":backups,"active":active}))
    }.await.map_err(|e:Box<dyn std::error::Error+Send+Sync>|e.to_string())
}

fn database_path_string(backup_root: &Path) -> String {
    backup_root
        .parent()
        .unwrap_or(backup_root)
        .join("xchat.db")
        .to_string_lossy()
        .into_owned()
}

pub async fn create(
    pool: &Pool<Sqlite>,
    request: CreateRequest,
) -> std::result::Result<JobView, String> {
    let job = register(pool, "create").await.map_err(|e| e.to_string())?;
    let view = job.view();
    let pool = pool.clone();
    tokio::spawn(async move {
        let result = create_archive(&pool, &request, &job).await;
        job.finish(result);
    });
    Ok(view)
}

async fn create_archive(
    pool: &Pool<Sqlite>,
    request: &CreateRequest,
    job: &Job,
) -> Result<serde_json::Value> {
    let root = root(pool).await?;
    tokio::fs::create_dir_all(&root).await?;
    let stage = root.join(format!("stage-{}", job.view().id));
    tokio::fs::create_dir(&stage).await?;
    let directory = request
        .directory
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or(root);
    let result=async{
        tokio::fs::create_dir_all(&directory).await?;
        job.phase("正在保存一致的聊天记录副本",0);
        let snapshot_path=stage.join("snapshot.sqlite");snapshot(pool,&snapshot_path,job).await?;job.check_cancelled()?;
        let mut snapshot_connection=records::connect(&snapshot_path,false).await?;
        let source_id:String=sqlx::query_scalar("SELECT value FROM settings WHERE key='user_id'").fetch_one(&mut snapshot_connection).await?;
        let messages:i64=sqlx::query_scalar("SELECT COUNT(*) FROM messages").fetch_one(&mut snapshot_connection).await?;
        sqlx::Connection::close(snapshot_connection).await?;
        let records_path=stage.join("records.sqlite");records::export(&snapshot_path,&records_path,&source_id,request.include_settings,job).await?;
        let mut files=vec![records_path.clone()];let mut entries=vec![archive::hash_file(&records_path,job).await?];
        if request.include_attachments{
            let attachments=records::attachments(&snapshot_path,&source_id).await?;
            let bytes=attachments.iter().map(|(_,_,n)|n).sum::<u64>();require_space(&stage,bytes).await?;
            job.phase("正在保存本地附件",bytes);
            for(key,path,expected)in attachments{
                let target=stage.join(format!("attachment-{}",entries.len()));
                let mut entry=archive::copy_file(&path,&target,job).await?;
                if entry.bytes!=expected{return Err("附件大小在备份期间发生变化".into());}
                entry.message_key=Some(key);files.push(target);entries.push(entry);
            }
        }
        let manifest=archive::Manifest{version:1,created_at:chrono::Utc::now().timestamp(),app_version:env!("CARGO_PKG_VERSION").into(),source_id,
            settings:request.include_settings,attachments:request.include_attachments,messages:messages as u64,entries};
        records::validate(&records_path,&manifest).await?;
        let expected=manifest.entries.iter().map(|entry|entry.bytes).sum::<u64>().saturating_add(16*1024*1024);
        require_space(&directory,expected).await?;
        let filename=format!("XChat-{}-{}.xchatbackup",chrono::Local::now().format("%Y%m%d-%H%M%S"),job.view().id);
        let final_path=directory.join(filename);let partial=final_path.with_extension("partial");
        let published=async{
            archive::write(&partial,&manifest,&files,job).await?;
            archive::verify(&partial,None,job).await?;
            let digest=archive::hash_file(&partial,job).await?;
            job.commit_point()?;
            tokio::fs::rename(&partial,&final_path).await?;
            let result=sqlx::query("INSERT INTO backup_records(id,path,created_at,bytes,attachments,settings,sha256) VALUES(?,?,?,?,?,?,?)")
                .bind(&job.view().id).bind(final_path.to_string_lossy().as_ref()).bind(manifest.created_at).bind(digest.bytes as i64)
                .bind(request.include_attachments).bind(request.include_settings).bind(digest.sha256).execute(pool).await;
            if let Err(error)=result{let _=tokio::fs::remove_file(&final_path).await;return Err(error.into());}
            Ok(serde_json::json!({"backup_id":job.view().id,"path":final_path,"bytes":digest.bytes,"messages":messages,"attachments":files.len()-1}))
        }.await;
        let _=tokio::fs::remove_file(partial).await;published
    }.await;
    cleanup(&stage).await;
    result
}

pub async fn file_path(pool: &Pool<Sqlite>, id: &str) -> std::result::Result<PathBuf, String> {
    sqlx::query_scalar::<_, String>("SELECT path FROM backup_records WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .map(PathBuf::from)
        .ok_or("备份记录不存在".into())
}

pub async fn prepare(
    pool: &Pool<Sqlite>,
    path: PathBuf,
    remove_source: bool,
) -> std::result::Result<JobView, String> {
    let job = register(pool, "restore").await.map_err(|e| e.to_string())?;
    let view = job.view();
    let pool = pool.clone();
    tokio::spawn(async move {
        let result = restore::prepare(&pool, &path, &job).await;
        if remove_source {
            let _ = tokio::fs::remove_file(path).await;
        }
        if let Err(error) = result {
            job.finish(Err(error));
        }
    });
    Ok(view)
}

pub async fn commit_restore(
    pool: &Pool<Sqlite>,
    id: &str,
    settings: bool,
) -> std::result::Result<JobView, String> {
    let job = find_job(pool, id).await.map_err(|e| e.to_string())?;
    {
        let mut state = job.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.status != "preview" {
            return Err("请先完成备份校验和恢复预览".into());
        }
        state.status = "running".into();
        state.phase = "正在保留恢复前副本".into();
        state.cancellable = false;
    }
    let prepared = job
        .prepared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or("恢复预览已过期")?;
    let view = job.view();
    let pool = pool.clone();
    tokio::spawn(async move {
        let result = restore::merge(&pool, &prepared, &job, settings).await;
        cleanup(&prepared.root).await;
        job.finish(result);
    });
    Ok(view)
}
