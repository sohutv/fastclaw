use crate::channels::SessionId;
use crate::hash_map;
use crate::memory::{MemoryManager, SearchResult, SearchResultItem};
use crate::service_provider::{EmbeddingArgs, EmbeddingResources, EmbeddingResult, Vector};
use anyhow::anyhow;
use chrono::{Duration, Local};
use itertools::Itertools;
use rig::completion::Message;
use rusqlite::Connection;
use std::collections::hash_map;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::io::AsyncWriteExt;

impl MemoryManager {
    pub async fn create_index(
        &self,
        session_id: &SessionId,
        messages: &[Message],
    ) -> crate::Result<PathBuf> {
        let (daily_memory_path, resources) = {
            let memory_path = self.context.workspace.memory_path.lock().await;
            let daily_memory_path = memory_path.join(format!(
                "{}.md",
                Local::now().date_naive().format("%Y-%m-%d")
            ));
            let daily_memory_file = tokio::fs::File::options()
                .append(true)
                .create(true)
                .open(&daily_memory_path)
                .await?;
            let mut writer = tokio::io::BufWriter::new(daily_memory_file);
            let mut resources_map = hash_map::HashMap::new();
            for message in messages {
                let id = uuid::Uuid::new_v4();
                let resources = EmbeddingResources::try_from(message)?;
                if resources.is_empty() {
                    continue;
                }
                let text = format!(
                    r#"
- {}
```
{}
```
"#,
                    id, resources
                );
                let _ = writer.write(text.as_bytes()).await?;
                resources_map.insert(id, resources);
            }
            let _ = writer.flush().await?;
            (daily_memory_path, resources_map)
        };

        let embedding = self.context.embedding_configs.try_into_embedding().await?;
        let EmbeddingResult { vectors } = embedding
            .embedding(self.context.workspace, EmbeddingArgs { resources })
            .await?;
        {
            let daily_memory_path = format!("{}", daily_memory_path.display());
            let conn = self.context.workspace.memory_conn(&session_id).await?;
            let db = conn.lock().await;
            let mut meta_data_insert_stmt =
                db.prepare("INSERT INTO memory_data(content,file_ref) VALUES (?, ?)")?;
            let mut vector_insert_stmt =
                db.prepare("INSERT INTO memory_data_vec(rowid, embedding) VALUES (?, ?)")?;
            for (_, Vector { resources, vector }) in &vectors {
                let _ = meta_data_insert_stmt.execute(rusqlite::params![
                    format!("{}", resources),
                    &daily_memory_path
                ])?;
                let id = db.last_insert_rowid() as usize;
                let _ = vector_insert_stmt.execute(rusqlite::params![id, vector.as_bytes()])?;
            }
        }
        Ok(daily_memory_path)
    }

    pub async fn search(
        &self,
        session_id: &SessionId,
        query: &str,
        top_k: usize,
        dt: Option<chrono::DateTime<Local>>,
    ) -> crate::Result<SearchResult> {
        let embedding = self.context.embedding_configs.try_into_embedding().await?;
        let id = uuid::Uuid::new_v4();
        let EmbeddingResult { vectors } = embedding
            .embedding(
                self.context.workspace,
                EmbeddingArgs {
                    resources: hash_map!(
                       id.clone() => query.into(),
                    ),
                },
            )
            .await?;
        let query_vector = vectors.get(&id).ok_or(anyhow!("id {} not found!", id))?;
        let conn = self.context.workspace.memory_conn(&session_id).await?;
        let db = conn.lock().await;

        let vec_rows: Vec<(usize, f32)> = db
            .prepare("select rowid, distance from memory_data_vec where embedding match ? ORDER BY distance LIMIT ?")?
            .query_map(rusqlite::params![query_vector.as_bytes(), top_k], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .into_iter()
            .flatten()
            .flatten()
            .collect_vec();

        if vec_rows.is_empty() {
            return Ok(Default::default());
        }
        let data_rows = db
            .prepare(&format!(
                "select id, content, file_ref from memory_data where id in ({}) and created_at > '{}'",
                vec_rows.iter().map(|(id, _)| *id).join(","),
                dt.unwrap_or(Local::now() - Duration::days(3)).to_utc().format(DATETIME_FORMAT)
            ))?
            .query_map::<(usize, String, String), _, _>((), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .into_iter()
            .flatten()
            .flatten()
            .collect_vec();
        let result = data_rows
            .into_iter()
            .map(|(id, message, file_ref)| SearchResultItem {
                id,
                message,
                file_ref: PathBuf::from_str(&file_ref).ok(),
            })
            .collect_vec()
            .into();
        Ok(result)
    }
}

const DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

impl MemoryManager {
    pub async fn init(db: Connection) -> crate::Result<Connection> {
        let _ = db.execute(
            r#"
CREATE TABLE IF NOT EXISTS memory_data
(
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT,
    file_ref TEXT,
    created_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
        "#,
            (),
        )?;

        let _ = db.execute(
            r#"
CREATE VIRTUAL TABLE IF NOT EXISTS memory_data_vec USING vec0
(
    embedding float[1024]
);
        "#,
            (),
        )?;
        Ok(db)
    }
}
