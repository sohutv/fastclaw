use crate::channels::SessionId;
use crate::tools::TaskTools;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
    pub sessions_path: PathBuf,
    pub downloads_path: PathBuf,
    pub sql_pools: Arc<RwLock<HashMap<SessionId, Arc<SqlitePool>>>>,
}

impl Workspace {
    pub async fn init<P: AsRef<Path>>(workdir: P) -> crate::Result<Self> {
        let workdir = workdir.as_ref();
        let path = workdir.join("workspace");
        if !path.exists() {
            let _ = tokio::fs::create_dir_all(&path).await?;
        }
        let sessions_path = path.join("sessions");
        if !sessions_path.exists() {
            let _ = tokio::fs::create_dir_all(&sessions_path).await?;
        }
        let downloads_path = path.join("downloads");
        if !downloads_path.exists() {
            let _ = tokio::fs::create_dir_all(&downloads_path).await?;
        }
        let cron_path = path.join("cron");
        if !cron_path.exists() {
            let _ = tokio::fs::create_dir_all(&cron_path).await?;
        }
        let self_ = Self {
            path,
            sessions_path,
            downloads_path,
            sql_pools: Default::default(),
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
            let sql_pool = Arc::new(sql_pool);
            sql_pools
                .entry(session_id.clone())
                .or_insert(Arc::clone(&sql_pool));
            TaskTools::init_cron_task(&sql_pool).await?;
            sql_pool
        };
        Ok(sql_pool)
    }

    #[inline(always)]
    pub fn session_path(&self, session_id: &SessionId) -> PathBuf {
        self.sessions_path.join(session_id.deref())
    }
}
