//! Opening the database.

use crate::secrets::SecretStore;
use serde::{de::DeserializeOwned, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Everything that can go wrong in this crate. Messages never contain secret values.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite failed.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    /// A migration failed.
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// Stored JSON or an enum could not be read.
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
    /// The OS credential store failed.
    #[error("secret store error: {0}")]
    Secret(String),
    /// A file operation failed.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    /// A row does not exist.
    #[error("not found: {0}")]
    NotFound(String),
}

/// The app's local database. Cheap to clone.
#[derive(Clone)]
pub struct Store {
    pub(crate) pool: SqlitePool,
    pub(crate) secrets: Arc<dyn SecretStore>,
    /// Serialises audit appends so the hash chain cannot fork.
    pub(crate) audit_lock: Arc<Mutex<()>>,
}

impl Store {
    /// Opens or creates `studio.db` in `dir`, enabling WAL and foreign keys and running migrations.
    pub async fn open(dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<Self, StoreError> {
        tokio::fs::create_dir_all(dir).await?;
        let path = dir.join("studio.db");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        restrict_permissions(&path)?;
        Ok(Self {
            pool,
            secrets,
            audit_lock: Arc::new(Mutex::new(())),
        })
    }

    /// The secret store this database was opened with.
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    /// Closes every connection.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

/// Current time in milliseconds since the epoch.
pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Stores a unit enum as its serde name.
pub(crate) fn enum_text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(StoreError::Corrupt("expected a unit enum".to_owned())),
    }
}

/// Reads a unit enum from its serde name.
pub(crate) fn parse_enum<T: DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|e| StoreError::Corrupt(e.to_string()))
}

/// Parses an id column.
pub(crate) fn parse_id<T: std::str::FromStr>(text: &str) -> Result<T, StoreError> {
    text.parse()
        .map_err(|_| StoreError::Corrupt("invalid id".to_owned()))
}

/// Parses a JSON column.
pub(crate) fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|e| StoreError::Corrupt(e.to_string()))
}

/// Serialises to a JSON column.
pub(crate) fn to_json<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|e| StoreError::Corrupt(e.to_string()))
}
