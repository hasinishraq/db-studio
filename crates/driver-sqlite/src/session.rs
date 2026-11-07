//! Connecting and the session type.

use crate::execute::SqliteConn;
use crate::metadata;
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use studio_db::{Conn, ConnScope, ConnectConfig, DbError, Driver, Session};
use studio_types::{
    ColumnInfo, DbObject, Engine, FilterRule, PageRequest, SchemaRef, ServerInfo, TablePage, TableRef,
    TableStructure,
};
use tokio::sync::Mutex as AsyncMutex;

/// One SQLite connection, shared with the blocking thread that uses it.
pub(crate) type Shared = Arc<Mutex<Connection>>;

/// SQLite.
#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteDriver;

/// Maps a library error. SQLite messages hold no row values.
pub(crate) fn map_error(error: rusqlite::Error) -> DbError {
    use rusqlite::ffi::ErrorCode;
    match &error {
        rusqlite::Error::SqliteFailure(failure, message) => match failure.code {
            ErrorCode::OperationInterrupted => DbError::Cancelled,
            ErrorCode::ReadOnly => DbError::ReadOnlyViolation,
            ErrorCode::CannotOpen | ErrorCode::NotADatabase => DbError::Connect(error.to_string()),
            _ => DbError::query(message.clone().unwrap_or_else(|| error.to_string())),
        },
        _ => DbError::query(error.to_string()),
    }
}

/// Opens the file. A read-only open makes the engine itself refuse writes.
pub(crate) fn open(path: &PathBuf, read_only: bool) -> Result<Connection, DbError> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let conn = Connection::open_with_flags(path, flags).map_err(map_error)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(map_error)?;
    Ok(conn)
}

/// Runs blocking work off the async threads.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, DbError> + Send + 'static,
) -> Result<T, DbError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?
}

#[async_trait::async_trait]
impl Driver for SqliteDriver {
    fn engine(&self) -> Engine {
        Engine::Sqlite
    }

    async fn connect(&self, cfg: &ConnectConfig) -> Result<Box<dyn Session>, DbError> {
        let path = PathBuf::from(&cfg.database);
        let read_only = cfg.read_only;
        let opened = path.clone();
        let version = blocking(move || {
            let conn = open(&opened, read_only)?;
            conn.query_row("SELECT sqlite_version()", [], |row| row.get::<_, String>(0))
                .map_err(map_error)
        })
        .await?;
        Ok(Box::new(SqliteSession {
            path,
            read_only,
            version,
            pinned: AsyncMutex::new(None),
        }))
    }
}

/// An open SQLite session. Connections are cheap, so one is opened per use.
pub(crate) struct SqliteSession {
    pub(crate) path: PathBuf,
    pub(crate) read_only: bool,
    version: String,
    /// The connection holding a manual transaction.
    pinned: AsyncMutex<Option<Shared>>,
}

impl SqliteSession {
    /// Runs read-only work on its own connection.
    pub(crate) async fn with_reader<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Connection) -> Result<T, DbError> + Send + 'static,
    ) -> Result<T, DbError> {
        let path = self.path.clone();
        blocking(move || work(&open(&path, true)?)).await
    }

    async fn finish(&self, sql: &'static str) -> Result<(), DbError> {
        let conn = self.pinned.lock().await.take();
        let conn = conn.ok_or_else(|| DbError::query("there is no open transaction"))?;
        blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| DbError::Internal("connection poisoned".to_owned()))?;
            conn.execute_batch(sql).map_err(map_error)
        })
        .await
    }
}

#[async_trait::async_trait]
impl Session for SqliteSession {
    async fn server_info(&self) -> Result<ServerInfo, DbError> {
        Ok(ServerInfo {
            version: format!("SQLite {}", self.version),
            tls: false,
            tls_verified: false,
            certificates: Vec::new(),
        })
    }

    async fn list_schemas(&self) -> Result<Vec<SchemaRef>, DbError> {
        Ok(vec![SchemaRef {
            name: "main".to_owned(),
        }])
    }

    async fn list_objects(&self, schema: &SchemaRef) -> Result<Vec<DbObject>, DbError> {
        let schema = schema.name.clone();
        self.with_reader(move |conn| metadata::list_objects(conn, &schema))
            .await
    }

    async fn columns(&self, _schema: &SchemaRef) -> Result<Vec<ColumnInfo>, DbError> {
        self.with_reader(metadata::columns).await
    }

    async fn table_structure(&self, table: &TableRef) -> Result<TableStructure, DbError> {
        let table = table.clone();
        self.with_reader(move |conn| metadata::table_structure(conn, &table))
            .await
    }

    async fn acquire(&self, scope: ConnScope) -> Result<Box<dyn Conn>, DbError> {
        if scope == ConnScope::Current {
            if let Some(pinned) = self.pinned.lock().await.as_ref() {
                return Ok(SqliteConn::boxed(pinned.clone(), self.path.clone()));
            }
        }
        let (path, read_only) = (self.path.clone(), self.read_only);
        let conn = blocking(move || open(&path, read_only)).await?;
        Ok(SqliteConn::boxed(Arc::new(Mutex::new(conn)), self.path.clone()))
    }

    async fn table_page(&self, table: &TableRef, page: &PageRequest) -> Result<TablePage, DbError> {
        let (table, page) = (table.clone(), page.clone());
        self.with_reader(move |conn| metadata::table_page(conn, &table, &page))
            .await
    }

    async fn table_count(
        &self,
        table: &TableRef,
        filters: &[FilterRule],
        timeout: Duration,
    ) -> Result<Option<u64>, DbError> {
        let (table, filters) = (table.clone(), filters.to_vec());
        self.with_reader(move |conn| metadata::table_count(conn, &table, &filters, timeout))
            .await
    }

    async fn explain(&self, stmt: &str) -> Result<String, DbError> {
        let stmt = stmt.to_owned();
        self.with_reader(move |conn| metadata::explain(conn, &stmt)).await
    }

    async fn begin(&self, read_only: bool) -> Result<(), DbError> {
        let mut pinned = self.pinned.lock().await;
        if pinned.is_some() {
            return Err(DbError::query("a transaction is already open"));
        }
        let path = self.path.clone();
        let read_only = read_only || self.read_only;
        let conn = blocking(move || {
            let conn = open(&path, read_only)?;
            conn.execute_batch("BEGIN").map_err(map_error)?;
            Ok(conn)
        })
        .await?;
        *pinned = Some(Arc::new(Mutex::new(conn)));
        Ok(())
    }

    async fn commit(&self) -> Result<(), DbError> {
        self.finish("COMMIT").await
    }

    async fn rollback(&self) -> Result<(), DbError> {
        self.finish("ROLLBACK").await
    }

    async fn close(&self) {
        self.pinned.lock().await.take();
    }
}
