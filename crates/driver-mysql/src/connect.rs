//! Connecting, the session type and error mapping.

use crate::execute::MyConn;
use crate::tls::ssl_opts;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn as RawConn, OptsBuilder, Pool, PoolConstraints, PoolOpts};
use secrecy::ExposeSecret;
use std::sync::Arc;
use std::time::Duration;
use studio_db::{Conn, ConnScope, ConnectConfig, DbError, Driver, Session};
use studio_types::{
    ColumnInfo, DbObject, Engine, FilterRule, PageRequest, SchemaRef, ServerInfo, TablePage, TableRef,
    TableStructure, TlsMode,
};
use tokio::sync::Mutex;

/// Server error codes we treat specially.
pub(crate) mod code {
    pub const ACCESS_DENIED: u16 = 1045;
    pub const DB_ACCESS_DENIED: u16 = 1044;
    pub const QUERY_INTERRUPTED: u16 = 1317;
    pub const READ_ONLY_TRANSACTION: u16 = 1792;
    pub const MARIADB_STATEMENT_TIMEOUT: u16 = 1969;
    pub const MYSQL_EXECUTION_TIME_EXCEEDED: u16 = 3024;
    pub const NOT_PREPARABLE: u16 = 1295;
}

/// MySQL and MariaDB.
#[derive(Clone, Copy, Debug, Default)]
pub struct MySqlDriver;

/// Maps a driver error. Nothing here carries a password.
pub(crate) fn map_error(error: mysql_async::Error) -> DbError {
    match error {
        mysql_async::Error::Server(server) => match server.code {
            code::ACCESS_DENIED | code::DB_ACCESS_DENIED => DbError::Auth(server.message),
            code::READ_ONLY_TRANSACTION => DbError::ReadOnlyViolation,
            code::MARIADB_STATEMENT_TIMEOUT | code::MYSQL_EXECUTION_TIME_EXCEEDED => DbError::Timeout,
            code::QUERY_INTERRUPTED => DbError::Cancelled,
            _ => DbError::Query {
                sqlstate: Some(server.state),
                message: server.message,
                position: None,
            },
        },
        mysql_async::Error::Io(mysql_async::IoError::Tls(tls)) => DbError::Tls(tls.to_string()),
        other => {
            let text = other.to_string();
            let lower = text.to_lowercase();
            if lower.contains("certificate") || lower.contains("tls") || lower.contains("ssl") {
                DbError::Tls(text)
            } else {
                DbError::Connect(text)
            }
        }
    }
}

fn build_opts(cfg: &ConnectConfig) -> Result<OptsBuilder, DbError> {
    let constraints =
        PoolConstraints::new(0, 4).ok_or_else(|| DbError::Internal("invalid pool constraints".to_owned()))?;
    let password = cfg.password.as_ref().map(|p| p.expose_secret().to_owned());
    let database = (!cfg.database.is_empty()).then(|| cfg.database.clone());
    let setup: Vec<String> = if cfg.read_only {
        vec!["SET SESSION TRANSACTION READ ONLY".to_owned()]
    } else {
        Vec::new()
    };
    Ok(OptsBuilder::default()
        .ip_or_hostname(cfg.host.clone())
        .tcp_port(cfg.port)
        .user(Some(cfg.username.clone()))
        .pass(password)
        .db_name(database)
        .prefer_socket(false)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .setup(setup)
        .pool_opts(PoolOpts::default().with_constraints(constraints))
        .ssl_opts(ssl_opts(&cfg.tls)?))
}

/// Facts shared by everything that runs on a session.
pub(crate) struct Shared {
    pub pool: Pool,
    pub mariadb: bool,
    /// Whether the profile keeps every connection read-only.
    pub read_only: bool,
    timeout: Duration,
}

impl Shared {
    /// A free connection. Dialling has its own deadline because the library sets none.
    pub(crate) async fn get(&self) -> Result<RawConn, DbError> {
        match tokio::time::timeout(self.timeout, self.pool.get_conn()).await {
            Ok(result) => result.map_err(map_error),
            Err(_) => Err(DbError::Connect("timed out connecting to the server".to_owned())),
        }
    }
}

#[async_trait::async_trait]
impl Driver for MySqlDriver {
    fn engine(&self) -> Engine {
        Engine::MySql
    }

    async fn connect(&self, cfg: &ConnectConfig) -> Result<Box<dyn Session>, DbError> {
        let pool = Pool::new(build_opts(cfg)?);
        let mut shared = Shared {
            pool,
            mariadb: false,
            read_only: cfg.read_only,
            timeout: cfg.connect_timeout,
        };
        let mut conn = shared.get().await?;
        let version: Option<String> = conn.query_first("SELECT VERSION()").await.map_err(map_error)?;
        drop(conn);
        let version = version.unwrap_or_default();
        shared.mariadb = version.to_lowercase().contains("mariadb");
        Ok(Box::new(MySession {
            shared: Arc::new(shared),
            version,
            mode: cfg.tls.mode,
            pinned: Arc::new(Mutex::new(None)),
        }))
    }
}

/// An open MySQL or MariaDB session.
pub(crate) struct MySession {
    pub(crate) shared: Arc<Shared>,
    version: String,
    mode: TlsMode,
    /// The connection holding a manual transaction.
    pinned: Arc<Mutex<Option<RawConn>>>,
}

impl MySession {
    async fn finish(&self, sql: &str) -> Result<(), DbError> {
        let conn = self.pinned.lock().await.take();
        let mut conn = conn.ok_or_else(|| DbError::query("there is no open transaction"))?;
        conn.query_drop(sql).await.map_err(map_error)
    }
}

#[async_trait::async_trait]
impl Session for MySession {
    async fn server_info(&self) -> Result<ServerInfo, DbError> {
        Ok(ServerInfo {
            version: self.version.clone(),
            tls: self.mode != TlsMode::Disable,
            tls_verified: matches!(self.mode, TlsMode::VerifyCa | TlsMode::VerifyFull),
            certificates: Vec::new(),
        })
    }

    async fn list_schemas(&self) -> Result<Vec<SchemaRef>, DbError> {
        crate::metadata::list_schemas(self).await
    }

    async fn list_objects(&self, schema: &SchemaRef) -> Result<Vec<DbObject>, DbError> {
        crate::metadata::list_objects(self, schema).await
    }

    async fn columns(&self, schema: &SchemaRef) -> Result<Vec<ColumnInfo>, DbError> {
        crate::metadata::columns(self, schema).await
    }

    async fn table_structure(&self, table: &TableRef) -> Result<TableStructure, DbError> {
        crate::metadata::table_structure(self, table).await
    }

    async fn acquire(&self, scope: ConnScope) -> Result<Box<dyn Conn>, DbError> {
        if scope == ConnScope::Current {
            let guard = self.pinned.clone().lock_owned().await;
            if guard.is_some() {
                return MyConn::pinned(guard, self.shared.clone());
            }
        }
        Ok(MyConn::pooled(self.shared.get().await?, self.shared.clone()))
    }

    async fn table_page(&self, table: &TableRef, page: &PageRequest) -> Result<TablePage, DbError> {
        crate::page::table_page(self, table, page).await
    }

    async fn table_count(
        &self,
        table: &TableRef,
        filters: &[FilterRule],
        timeout: Duration,
    ) -> Result<Option<u64>, DbError> {
        crate::page::table_count(self, table, filters, timeout).await
    }

    async fn explain(&self, stmt: &str) -> Result<String, DbError> {
        crate::page::explain(self, stmt).await
    }

    async fn begin(&self, read_only: bool) -> Result<(), DbError> {
        let mut pinned = self.pinned.lock().await;
        if pinned.is_some() {
            return Err(DbError::query("a transaction is already open"));
        }
        let mut conn = self.shared.get().await?;
        let sql = if read_only {
            "START TRANSACTION READ ONLY"
        } else {
            "START TRANSACTION"
        };
        conn.query_drop(sql).await.map_err(map_error)?;
        *pinned = Some(conn);
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
        let _ = self.shared.pool.clone().disconnect().await;
    }
}
