//! An in-memory driver for tests, so policy can be proven without a database.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{AuditSink, SessionManager};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use studio_db::{
    Canceller, Conn, ConnScope, ConnectConfig, DbError, Driver, ExecOptions, RowEvent, RowSink, Session,
};
use studio_safety::ApprovalBook;
use studio_types::*;
use tokio::sync::Notify;

/// What the fake database saw.
#[derive(Default)]
pub struct FakeLog {
    /// Each statement with the options and scope it ran under.
    pub executed: Vec<(String, ExecOptions, ConnScope)>,
    /// Databases chosen with `use_database`.
    pub used_databases: Vec<String>,
    /// Schemas chosen with `use_schema`.
    pub used_schemas: Vec<String>,
    /// Calls to `begin`.
    pub begins: u32,
    /// Calls to `commit`.
    pub commits: u32,
    /// Calls to `rollback`.
    pub rollbacks: u32,
    /// Calls to `close`.
    pub closes: u32,
    /// Cancels received.
    pub cancels: u32,
    /// Configs passed to `connect`.
    pub configs: Vec<ConnectConfig>,
}

/// Rows a read returns, replacing the default.
pub type CannedRows = Arc<Mutex<Option<(Vec<ColumnMeta>, Vec<Vec<Value>>)>>>;

/// Shared handle to the log.
pub type Log = Arc<Mutex<FakeLog>>;

/// A driver that records everything.
///
/// A statement containing `slow()` blocks until cancelled; one containing `boom` fails; a `SELECT`
/// returns one row holding `canned`. Other statements affect one row, or none when they contain
/// `nomatch`, or two when they contain `twice`.
#[derive(Clone, Default)]
pub struct FakeDriver {
    /// The log.
    pub log: Log,
    /// Rows a read returns, replacing the default.
    pub rows: CannedRows,
}

struct FakeSession {
    log: Log,
    rows: CannedRows,
}

struct FakeConn {
    log: Log,
    rows: CannedRows,
    scope: ConnScope,
    canceller: Arc<FakeCanceller>,
}

struct FakeCanceller {
    log: Log,
    stop: Notify,
    stopped: AtomicBool,
}

#[async_trait::async_trait]
impl Canceller for FakeCanceller {
    async fn cancel(&self) -> Result<(), DbError> {
        self.log.lock().unwrap().cancels += 1;
        self.stopped.store(true, Ordering::SeqCst);
        self.stop.notify_waiters();
        Ok(())
    }
}

#[async_trait::async_trait]
impl Conn for FakeConn {
    async fn execute(
        &mut self,
        stmt: &str,
        opts: ExecOptions,
        sink: RowSink,
    ) -> Result<ExecSummary, DbError> {
        self.log
            .lock()
            .unwrap()
            .executed
            .push((stmt.to_owned(), opts, self.scope));
        if stmt.contains("slow()") {
            while !self.canceller.stopped.load(Ordering::SeqCst) {
                let _ = tokio::time::timeout(Duration::from_millis(5), self.canceller.stop.notified()).await;
            }
            return Err(DbError::Cancelled);
        }
        if stmt.contains("boom") {
            return Err(DbError::Query {
                sqlstate: Some("42000".into()),
                message: "boom failed".into(),
                position: Some(3),
            });
        }
        let upper = stmt.trim_start().to_uppercase();
        let returns_rows =
            upper.starts_with("SELECT") || upper.starts_with("SHOW") || upper.starts_with("EXPLAIN");
        if !returns_rows {
            let rows_affected = if stmt.contains("nomatch") {
                0
            } else if stmt.contains("twice") {
                2
            } else {
                1
            };
            return Ok(ExecSummary {
                rows_affected,
                ..Default::default()
            });
        }
        let canned = self.rows.lock().unwrap().clone();
        let (columns, rows) = canned.unwrap_or_else(|| {
            let column = ColumnMeta {
                name: "n".into(),
                type_name: "int4".into(),
                nullable: None,
            };
            (vec![column], vec![vec![Value::Int(1)]])
        });
        let _ = sink.send(RowEvent::Columns(columns.clone())).await;
        let total = rows.len() as u64;
        let _ = sink.send(RowEvent::Batch(RowBatch { seq: 0, rows })).await;
        Ok(ExecSummary {
            columns,
            returns_rows: true,
            total_rows: total,
            ..Default::default()
        })
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }
}

#[async_trait::async_trait]
impl Session for FakeSession {
    async fn server_info(&self) -> Result<ServerInfo, DbError> {
        Ok(ServerInfo {
            version: "fake 1.0".into(),
            tls: false,
            tls_verified: false,
            certificates: vec![],
        })
    }

    async fn list_databases(&self) -> Result<Vec<String>, DbError> {
        Ok(vec!["app".into(), "other".into()])
    }

    async fn current_schema(&self) -> Result<Option<String>, DbError> {
        Ok(Some("public".into()))
    }

    async fn use_schema(&self, name: &str) -> Result<(), DbError> {
        self.log.lock().unwrap().used_schemas.push(name.to_owned());
        Ok(())
    }

    async fn use_database(&self, name: &str) -> Result<(), DbError> {
        self.log.lock().unwrap().used_databases.push(name.to_owned());
        Ok(())
    }

    async fn list_schemas(&self) -> Result<Vec<SchemaRef>, DbError> {
        Ok(vec![SchemaRef {
            name: "public".into(),
        }])
    }

    async fn list_objects(&self, schema: &SchemaRef) -> Result<Vec<DbObject>, DbError> {
        Ok(vec![DbObject {
            name: "users".into(),
            schema: schema.name.clone(),
            kind: ObjectKind::Table,
        }])
    }

    async fn columns(&self, _schema: &SchemaRef) -> Result<Vec<ColumnInfo>, DbError> {
        let column = |name: &str, ordinal| ColumnInfo {
            table: "users".into(),
            name: name.into(),
            data_type: "text".into(),
            nullable: true,
            default_value: None,
            primary_key: ordinal == 1,
            ordinal,
        };
        Ok(vec![column("id", 1), column("email", 2)])
    }

    async fn table_structure(&self, table: &TableRef) -> Result<TableStructure, DbError> {
        Ok(TableStructure {
            table: table.clone(),
            kind: ObjectKind::Table,
            columns: self
                .columns(&SchemaRef {
                    name: table.schema.clone(),
                })
                .await?,
            indexes: vec![],
            foreign_keys: vec![],
            constraints: vec![],
            metadata: TableMetadata::default(),
            ddl: Some("CREATE TABLE users (id int)".into()),
        })
    }

    async fn acquire(&self, scope: ConnScope) -> Result<Box<dyn Conn>, DbError> {
        Ok(Box::new(FakeConn {
            log: self.log.clone(),
            rows: self.rows.clone(),
            scope,
            canceller: Arc::new(FakeCanceller {
                log: self.log.clone(),
                stop: Notify::new(),
                stopped: AtomicBool::new(false),
            }),
        }))
    }

    async fn table_page(&self, _table: &TableRef, _page: &PageRequest) -> Result<TablePage, DbError> {
        Ok(TablePage {
            columns: vec![],
            rows: vec![],
        })
    }

    async fn table_count(
        &self,
        _table: &TableRef,
        _filters: &[FilterRule],
        _timeout: Duration,
    ) -> Result<Option<u64>, DbError> {
        Ok(Some(0))
    }

    async fn explain(&self, _stmt: &str) -> Result<String, DbError> {
        Ok("[{\"Plan\":{}}]".into())
    }

    async fn begin(&self, _read_only: bool) -> Result<(), DbError> {
        self.log.lock().unwrap().begins += 1;
        Ok(())
    }

    async fn commit(&self) -> Result<(), DbError> {
        self.log.lock().unwrap().commits += 1;
        Ok(())
    }

    async fn rollback(&self) -> Result<(), DbError> {
        self.log.lock().unwrap().rollbacks += 1;
        Ok(())
    }

    async fn close(&self) {
        self.log.lock().unwrap().closes += 1;
    }
}

#[async_trait::async_trait]
impl Driver for FakeDriver {
    fn engine(&self) -> Engine {
        Engine::Postgres
    }

    async fn connect(&self, cfg: &ConnectConfig) -> Result<Box<dyn Session>, DbError> {
        self.log.lock().unwrap().configs.push(cfg.clone());
        Ok(Box::new(FakeSession {
            log: self.log.clone(),
            rows: self.rows.clone(),
        }))
    }
}

/// An audit sink that keeps entries.
#[derive(Clone, Default)]
pub struct MemoryAudit {
    /// Records received.
    pub entries: Arc<Mutex<Vec<AuditEntry>>>,
}

#[async_trait::async_trait]
impl AuditSink for MemoryAudit {
    async fn record(&self, entry: AuditEntry) {
        self.entries.lock().unwrap().push(entry);
    }
}

/// A plain development profile for the fake driver.
pub fn profile() -> ConnectionProfile {
    ConnectionProfile {
        id: ProfileId::new(),
        name: "fake".into(),
        engine: Engine::Postgres,
        host: "localhost".into(),
        port: 5432,
        database: "app".into(),
        username: "me".into(),
        tls_mode: TlsMode::Disable,
        ca_cert_path: None,
        client_cert_path: None,
        client_key_path: None,
        ssh: None,
        environment: Environment::Development,
        read_only: false,
        ai_policy: AiPolicy::Default,
        color: None,
        default_schema: None,
        has_password: false,
    }
}

/// Everything a test needs.
pub struct Harness {
    /// The manager under test.
    pub manager: Arc<SessionManager>,
    /// The approval book it shares.
    pub approvals: Arc<ApprovalBook>,
    /// What the database saw.
    pub driver: FakeDriver,
    /// Audit records.
    pub audit: MemoryAudit,
}

/// Builds a manager over the fake driver.
pub fn harness() -> Harness {
    let driver = FakeDriver::default();
    let audit = MemoryAudit::default();
    let approvals = Arc::new(ApprovalBook::new());
    let fake = || Arc::new(driver.clone()) as Arc<dyn Driver>;
    let drivers: HashMap<Engine, Arc<dyn Driver>> = HashMap::from([
        (Engine::Postgres, fake()),
        (Engine::MySql, fake()),
        (Engine::Sqlite, fake()),
    ]);
    let manager = SessionManager::new(drivers, approvals.clone(), Arc::new(audit.clone()));
    Harness {
        manager: Arc::new(manager),
        approvals,
        driver,
        audit,
    }
}
