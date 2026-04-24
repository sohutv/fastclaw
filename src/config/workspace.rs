use crate::channels::SessionId;
use crate::memory::MemoryManager;
use crate::tools::TaskTools;
use rusqlite::Connection;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
    sessions_path: PathBuf,
    pub memory_path: Arc<Mutex<PathBuf>>,
    pub downloads_path: PathBuf,
    pub sql_pools: Arc<RwLock<HashMap<SessionId, Arc<SqlitePool>>>>,
    pub memory_conns: Arc<RwLock<HashMap<SessionId, Arc<Mutex<Connection>>>>>,
}

impl Workspace {
    pub async fn init<P: AsRef<Path>>(workdir: P) -> crate::Result<Self> {
        {
            // load sqlite vec extension
            use rusqlite::ffi::sqlite3_auto_extension;
            use sqlite_vec::sqlite3_vec_init;
            unsafe {
                sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
            }
        }
        let workdir = workdir.as_ref();
        let path = workdir.join("workspace");
        if !path.exists() {
            let _ = tokio::fs::create_dir_all(&path).await?;
        }
        let sessions_path = path.join("sessions");
        if !sessions_path.exists() {
            let _ = tokio::fs::create_dir_all(&sessions_path).await?;
        }
        let memory_path = path.join("memory");
        if !memory_path.exists() {
            let _ = tokio::fs::create_dir_all(&memory_path).await?;
        }
        let downloads_path = path.join("downloads");
        if !downloads_path.exists() {
            let _ = tokio::fs::create_dir_all(&downloads_path).await?;
        }
        let self_ = Self {
            path,
            sessions_path,
            memory_path: Arc::new(Mutex::new(memory_path)),
            downloads_path,
            sql_pools: Default::default(),
            memory_conns: Default::default(),
        };
        Ok(self_)
    }

    pub fn downloads_path(&self) -> &Path {
        &self.downloads_path
    }

    pub async fn sql_pool(&self, session_id: &SessionId) -> crate::Result<Arc<SqlitePool>> {
        {
            let sql_pools = self.sql_pools.read().await;
            if let Some(pool) = sql_pools.get(session_id) {
                return Ok(Arc::clone(pool));
            }
        }
        let sql_pool = {
            let mut sql_pools = self.sql_pools.write().await;
            let sqlite_path = {
                let sqlite_path = self.session_path(session_id).join("db").join("db.sqlite");
                if let Some(parent) = sqlite_path.parent() {
                    if !parent.exists() {
                        let _ = tokio::fs::create_dir_all(parent).await?;
                    }
                } else {
                    panic!("unexpected db path: {}", sqlite_path.display());
                }
                sqlite_path
            };
            let sql_pool = SqlitePoolOptions::new()
                .connect_with(
                    SqliteConnectOptions::from_str(&format!("sqlite://{}", sqlite_path.display()))?
                        .create_if_missing(true)
                        .journal_mode(SqliteJournalMode::Wal)
                        .busy_timeout(std::time::Duration::from_secs(5)),
                )
                .await?;
            let sql_pool = Arc::new(TaskTools::init_cron_task(sql_pool).await?);
            sql_pools
                .entry(session_id.clone())
                .or_insert(Arc::clone(&sql_pool));
            sql_pool
        };
        Ok(sql_pool)
    }

    pub async fn memory_conn(
        &self,
        session_id: &SessionId,
    ) -> crate::Result<Arc<Mutex<Connection>>> {
        {
            let connections = self.memory_conns.read().await;
            if let Some(conn) = connections.get(session_id) {
                return Ok(Arc::clone(conn));
            }
        }
        let conn = {
            let memory_path = self.memory_path.lock().await;
            let mut connections = self.memory_conns.write().await;
            let sqlite_path = memory_path.join("db.sqlite");
            let conn = Arc::new(Mutex::new(
                MemoryManager::init(Connection::open(&sqlite_path)?).await?,
            ));
            connections
                .entry(session_id.clone())
                .or_insert(Arc::clone(&conn));
            conn
        };
        Ok(conn)
    }

    #[inline(always)]
    pub fn session_path(&self, session_id: &SessionId) -> PathBuf {
        self.sessions_path.join(session_id.deref())
    }
}

#[cfg(test)]
mod tests {
    use crate::channels::{Anonymous, SessionId};
    use zerocopy::IntoBytes;

    use crate::config::Workspace;

    #[tokio::test]
    async fn test_sqlite_vec() -> crate::Result<()> {
        let workspace = Workspace::init("/tmp").await?;
        let session_id = SessionId::from(&Anonymous("anonymous".to_string()));
        let conn = workspace.memory_conn(&session_id).await?;
        let db = conn.lock().await;
        let v: Vec<f32> = vec![0.1, 0.2, 0.3];

        let (vec_version, embedding): (String, String) = db.query_row(
            "select  vec_version(), vec_to_json(?)",
            &[v.as_bytes()],
            |x| Ok((x.get(0)?, x.get(1)?)),
        )?;
        println!("vec_version={vec_version}, embedding={embedding}");
        Ok(())
    }
}
