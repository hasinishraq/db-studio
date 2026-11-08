//! Connecting, the session type and error mapping.

use crate::execute::{PgCanceller, PgConn};
use crate::tls::{self, CapturedChain, PgTls};
use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use secrecy::ExposeSecret;
use std::net::IpAddr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use studio_db::{Conn, ConnScope, ConnectConfig, DbError, Driver, Session};
use studio_types::{
    ColumnInfo, DbObject, Engine, FilterRule, PageRequest, SchemaRef, ServerInfo, TablePage, TableRef,
    TableStructure, TlsMode,
};
use tokio::sync::Mutex;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, Config, NoTls};

const POOL_SIZE: usize = 4;

/// PostgreSQL.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresDriver;

/// Maps a driver error. Messages from the server and library carry no password.
pub(crate) fn map_error(error: tokio_postgres::Error) -> DbError {
    if let Some(db) = error.as_db_error() {
        let code = db.code();
        if *code == SqlState::INVALID_PASSWORD || *code == SqlState::INVALID_AUTHORIZATION_SPECIFICATION {
            return DbError::Auth(db.message().to_owned());
        }
        if *code == SqlState::READ_ONLY_SQL_TRANSACTION {
            return DbError::ReadOnlyViolation;
        }
        let position = match db.position() {
            Some(tokio_postgres::error::ErrorPosition::Original(position)) => Some(*position),
            _ => None,
        };
        return DbError::Query {
            sqlstate: Some(code.code().to_owned()),
            message: db.message().to_owned(),
            position,
        };
    }
    let text = full_message(&error);
    let lower = text.to_lowercase();
    if lower.contains("certificate") || lower.contains("tls") || lower.contains("ssl") {
        DbError::Tls(text)
    } else {
        DbError::Connect(text)
    }
}

fn full_message(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

pub(crate) fn pool_error(error: deadpool_postgres::PoolError) -> DbError {
    match error {
        deadpool_postgres::PoolError::Backend(inner) => map_error(inner),
        other => DbError::Connect(full_message(&other)),
    }
}

/// The client inside a pool object.
pub(crate) fn client(object: &Object) -> &Client {
    object
}

/// The database to open. PostgreSQL always needs one, and `postgres` exists on nearly every server.
fn database_name(cfg: &ConnectConfig) -> &str {
    if cfg.database.is_empty() {
        "postgres"
    } else {
        &cfg.database
    }
}

fn build_config(cfg: &ConnectConfig) -> Config {
    let mut config = Config::new();
    // With a tunnel we dial 127.0.0.1 but verify the real name, which `host` carries.
    match (&cfg.tls.server_name, cfg.host.parse::<IpAddr>()) {
        (Some(name), Ok(address)) => {
            config.host(name.as_str()).hostaddr(address);
        }
        _ => {
            config.host(cfg.host.as_str());
        }
    }
    config
        .port(cfg.port)
        .dbname(database_name(cfg))
        .user(cfg.username.as_str())
        .application_name(cfg.application_name.as_str())
        .connect_timeout(cfg.connect_timeout)
        .keepalives(true)
        .ssl_mode(if cfg.tls.mode == TlsMode::Disable {
            tokio_postgres::config::SslMode::Disable
        } else {
            tokio_postgres::config::SslMode::Require
        });
    if let Some(password) = &cfg.password {
        config.password(password.expose_secret());
    }
    if cfg.read_only {
        config.options("-c default_transaction_read_only=on");
    }
    config
}

fn build_pool(config: Config, tls: &PgTls) -> Result<Pool, DbError> {
    // Whatever a statement left behind on a connection must not reach the next user of it.
    let recycling = ManagerConfig {
        recycling_method: RecyclingMethod::Custom("ROLLBACK; RESET ALL".to_owned()),
    };
    let manager = match tls {
        PgTls::Plain => Manager::from_config(config, NoTls, recycling),
        PgTls::Rustls(connector) => Manager::from_config(config, connector.clone(), recycling),
    };
    Pool::builder(manager)
        .max_size(POOL_SIZE)
        .build()
        .map_err(|e| DbError::Internal(e.to_string()))
}

#[async_trait::async_trait]
impl Driver for PostgresDriver {
    fn engine(&self) -> Engine {
        Engine::Postgres
    }

    async fn connect(&self, cfg: &ConnectConfig) -> Result<Box<dyn Session>, DbError> {
        let captured: CapturedChain = Arc::new(StdMutex::new(Vec::new()));
        let tls = Arc::new(tls::build(&cfg.tls, &captured)?);
        let pool = build_pool(build_config(cfg), &tls)?;
        // `connect_timeout` covers only the TCP step. A server, or a tunnel to one, that accepts and then
        // stays silent must not leave the UI on "Connecting" forever.
        let handshake = async {
            let object = pool.get().await.map_err(pool_error)?;
            let row = client(&object)
                .query_one("SELECT version()", &[])
                .await
                .map_err(map_error)?;
            row.try_get::<_, String>(0).map_err(map_error)
        };
        let version = tokio::time::timeout(cfg.connect_timeout, handshake)
            .await
            .map_err(|_| DbError::Connect("the server did not answer in time".to_owned()))??;
        Ok(Box::new(PgSession {
            pool,
            tls,
            mode: cfg.tls.mode,
            version,
            captured,
            pinned: Arc::new(Mutex::new(None)),
            default_schema: std::sync::RwLock::new(None),
        }))
    }
}

/// An open PostgreSQL session.
pub(crate) struct PgSession {
    pub(crate) pool: Pool,
    tls: Arc<PgTls>,
    mode: TlsMode,
    version: String,
    captured: CapturedChain,
    /// The connection holding a manual transaction.
    pinned: Arc<Mutex<Option<Object>>>,
    /// The chosen schema. Pooled connections keep their settings, so each is pointed at it when taken.
    default_schema: std::sync::RwLock<Option<String>>,
}

/// Puts the chosen schema first, keeping `public` for anything installed there.
fn search_path(schema: &str) -> String {
    let first = studio_db::sql::quote_ident(Engine::Postgres, schema);
    if schema == "public" {
        format!("SET search_path TO {first}")
    } else {
        format!("SET search_path TO {first}, public")
    }
}

impl PgSession {
    /// A free pooled connection, for metadata and paging.
    pub(crate) async fn free(&self) -> Result<Object, DbError> {
        let object = self.pool.get().await.map_err(pool_error)?;
        let chosen = self.default_schema.read().ok().and_then(|name| name.clone());
        if let Some(schema) = chosen {
            client(&object)
                .batch_execute(&search_path(&schema))
                .await
                .map_err(map_error)?;
        }
        Ok(object)
    }

    async fn finish(&self, sql: &str) -> Result<(), DbError> {
        let object = self.pinned.lock().await.take();
        let object = object.ok_or_else(|| DbError::query("there is no open transaction"))?;
        client(&object).batch_execute(sql).await.map_err(map_error)
    }
}

#[async_trait::async_trait]
impl Session for PgSession {
    async fn server_info(&self) -> Result<ServerInfo, DbError> {
        let certificates = match self.captured.lock() {
            Ok(chain) => chain
                .iter()
                .filter_map(|der| crate::metadata::describe_certificate(der))
                .collect(),
            Err(_) => Vec::new(),
        };
        Ok(ServerInfo {
            version: self.version.clone(),
            tls: self.mode != TlsMode::Disable,
            tls_verified: matches!(self.mode, TlsMode::VerifyCa | TlsMode::VerifyFull),
            certificates,
        })
    }

    async fn list_databases(&self) -> Result<Vec<String>, DbError> {
        crate::metadata::list_databases(self).await
    }

    async fn current_schema(&self) -> Result<Option<String>, DbError> {
        let object = self.free().await?;
        let row = client(&object)
            .query_one("SELECT current_schema()::text", &[])
            .await
            .map_err(map_error)?;
        row.try_get(0).map_err(map_error)
    }

    async fn use_schema(&self, name: &str) -> Result<(), DbError> {
        let object = self.pool.get().await.map_err(pool_error)?;
        client(&object)
            .batch_execute(&search_path(name))
            .await
            .map_err(map_error)?;
        if let Ok(mut slot) = self.default_schema.write() {
            *slot = Some(name.to_owned());
        }
        Ok(())
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
                return PgConn::pinned(guard, self.tls.clone());
            }
        }
        Ok(PgConn::pooled(self.free().await?, self.tls.clone()))
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
        let object = self.free().await?;
        let sql = if read_only { "BEGIN READ ONLY" } else { "BEGIN" };
        client(&object).batch_execute(sql).await.map_err(map_error)?;
        *pinned = Some(object);
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
        self.pool.close();
    }
}

impl PgCanceller {
    /// Sends the protocol-level cancel request over the same kind of transport.
    pub(crate) async fn send(
        token: &tokio_postgres::CancelToken,
        tls: &PgTls,
    ) -> Result<(), tokio_postgres::Error> {
        match tls {
            PgTls::Plain => token.cancel_query(NoTls).await,
            PgTls::Rustls(connector) => token.cancel_query(connector.clone()).await,
        }
    }
}
