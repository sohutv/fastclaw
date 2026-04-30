use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use strum::Display;
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogConfig {
    logger: Logger,
    level: Level,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Logger {
    Stdout,
    File { logs_dir: PathBuf },
}
impl Default for Logger {
    fn default() -> Self {
        Self::File {
            logs_dir: "./logs".into(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Display, Default)]
pub enum Level {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

impl LogConfig {
    pub fn init<P: AsRef<Path>>(&self, workdir: P) -> crate::Result<()> {
        let env_filter = EnvFilter::builder().parse(self.level.to_string())?;
        let layered = tracing_subscriber::registry().with(env_filter);
        match &self.logger {
            Logger::Stdout => {
                layered
                    .with(
                        fmt::layer()
                            .with_thread_names(true)
                            .with_timer(OffsetTime::local_rfc_3339()?)
                            .with_line_number(true),
                    )
                    .init();
            }
            Logger::File { logs_dir } => {
                use tracing_appender::rolling;
                let logs_dir = if logs_dir.is_relative() {
                    workdir.as_ref().join(logs_dir)
                } else {
                    logs_dir.to_owned()
                };
                if !logs_dir.exists() {
                    std::fs::create_dir_all(&logs_dir)?;
                }
                layered
                    .with(
                        fmt::layer()
                            .with_thread_names(true)
                            .with_timer(OffsetTime::local_rfc_3339()?)
                            .with_line_number(true)
                            .with_writer(rolling::never(&logs_dir, "stdout.log")),
                    )
                    .init();
            }
        }
        Ok(())
    }
}
