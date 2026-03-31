use crate::tools::TaskTools;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
    pub sql_pool: SqlitePool,
}

impl Workspace {
    pub async fn init<P: AsRef<Path>>(workdir: P) -> crate::Result<Self> {
        let workdir = workdir.as_ref();
        let self_ = Self {
            path: workdir.join("workspace"),
            sql_pool: {
                let sql_pool = SqlitePoolOptions::new()
                    .connect_with(
                        SqliteConnectOptions::from_str(&format!(
                            "sqlite://{}",
                            workdir.join("db.sqlite").display()
                        ))?
                        .create_if_missing(true)
                        .journal_mode(SqliteJournalMode::Wal)
                        .busy_timeout(std::time::Duration::from_secs(5)),
                    )
                    .await?;
                sql_pool
            },
        };
        TaskTools::init_cron_task(&self_).await?;
        Ok(self_)
    }
}
