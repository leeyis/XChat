//! Each online backup step releases its source lock. C handles live on one blocking worker.
use super::{JobView, Result};
use libsqlite3_sys as sqlite;
use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Database(*mut sqlite::sqlite3);
impl Database {
    fn open(path: &std::path::Path, read_only: bool) -> Result<Self> {
        let path = CString::new(path.to_str().ok_or("数据库路径编码无效")?)?;
        let mut handle = ptr::null_mut();
        // SAFETY: the NUL-terminated path and output pointer are valid; this worker owns the handle.
        let code = unsafe {
            sqlite::sqlite3_open_v2(
                path.as_ptr(),
                &mut handle,
                (if read_only {
                    sqlite::SQLITE_OPEN_READONLY
                } else {
                    sqlite::SQLITE_OPEN_READWRITE
                }) | sqlite::SQLITE_OPEN_NOMUTEX,
                ptr::null(),
            )
        };
        let database = Self(handle);
        if code != sqlite::SQLITE_OK {
            return Err(database.error().into());
        }
        unsafe {
            sqlite::sqlite3_busy_timeout(database.0, 100);
        }
        Ok(database)
    }
    fn error(&self) -> String {
        if self.0.is_null() {
            return "无法打开备份数据库".into();
        }
        // SAFETY: the live handle owns this NUL-terminated error string.
        unsafe {
            CStr::from_ptr(sqlite::sqlite3_errmsg(self.0))
                .to_string_lossy()
                .into_owned()
        }
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite::sqlite3_close(self.0);
            }
        }
    }
}
struct Backup(*mut sqlite::sqlite3_backup);
impl Drop for Backup {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                sqlite::sqlite3_backup_finish(self.0);
            }
        }
    }
}

pub async fn copy(
    source: PathBuf,
    destination: PathBuf,
    cancelled: Arc<AtomicBool>,
    progress: Arc<Mutex<JobView>>,
    page_bytes: u64,
) -> Result<()> {
    // The native API receives only a newly reserved empty destination, never an existing file.
    tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .await?;
    tokio::task::spawn_blocking(move || {
        let source = Database::open(&source, true)?;
        let destination = Database::open(&destination, false)?;
        // SAFETY: both handles remain live on this thread; database names are static C strings.
        let mut backup = Backup(unsafe {
            sqlite::sqlite3_backup_init(destination.0, c"main".as_ptr(), source.0, c"main".as_ptr())
        });
        if backup.0.is_null() {
            return Err(destination.error().into());
        }
        let started = Instant::now();
        let mut last_progress = Instant::now();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("本次操作已取消".into());
            }
            if started.elapsed() > Duration::from_secs(1800) {
                return Err("数据库持续繁忙或备份时间过长，请稍后重试".into());
            }
            let code = unsafe { sqlite::sqlite3_backup_step(backup.0, 256) };
            match code {
                sqlite::SQLITE_OK | sqlite::SQLITE_DONE => {
                    last_progress = Instant::now();
                    let total = unsafe { sqlite::sqlite3_backup_pagecount(backup.0) }.max(0) as u64;
                    let remaining =
                        unsafe { sqlite::sqlite3_backup_remaining(backup.0) }.max(0) as u64;
                    let mut state = progress.lock().unwrap_or_else(|e| e.into_inner());
                    state.total_bytes = total.saturating_mul(page_bytes);
                    state.completed_bytes =
                        total.saturating_sub(remaining).saturating_mul(page_bytes);
                    if code == sqlite::SQLITE_DONE {
                        break;
                    }
                }
                sqlite::SQLITE_BUSY | sqlite::SQLITE_LOCKED => {
                    if last_progress.elapsed() > Duration::from_secs(30) {
                        return Err("数据库写入繁忙，备份尚未完成，请稍后重试".into());
                    }
                }
                _ => return Err(format!("一致快照失败：{}", destination.error()).into()),
            }
            // This is a dedicated blocking worker, never the async runtime or pool thread.
            std::thread::sleep(Duration::from_millis(if code == sqlite::SQLITE_OK {
                2
            } else {
                20
            }));
        }
        let code = unsafe { sqlite::sqlite3_backup_finish(backup.0) };
        backup.0 = ptr::null_mut();
        if code != sqlite::SQLITE_OK {
            return Err(destination.error().into());
        }
        Ok(())
    })
    .await?
}
