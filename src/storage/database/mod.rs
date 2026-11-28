pub mod audit;
pub mod contacts;
pub mod idempotency;
pub mod media;
pub mod messages;
pub mod sessions;
pub mod transcriptions;
pub mod webhooks;

use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use thiserror::Error;

use crate::config::{JournalMode, SqliteConfig};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(cfg: &SqliteConfig) -> Result<Self, DbError> {
        let path_str = cfg.relay_database_path.to_string_lossy();
        let url = if path_str == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite://{path_str}")
        };

        let journal_mode = match cfg.journal_mode {
            JournalMode::Wal => SqliteJournalMode::Wal,
            JournalMode::Delete => SqliteJournalMode::Delete,
            JournalMode::Truncate => SqliteJournalMode::Truncate,
            JournalMode::Memory => SqliteJournalMode::Memory,
        };

        let options = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(journal_mode)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_millis(cfg.busy_timeout_ms.max(15_000)));

        // Serialize SQLite access through a single pooled connection in WAL mode
        // to eliminate transient SQLITE_BUSY (code 5) lock contention under bursts.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn health_check(&self) -> Result<(), DbError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}
