//! An open database session and the connections it hands out.

use crate::error::DbError;
use std::sync::Arc;
use std::time::Duration;
use studio_types::{
    ColumnInfo, ColumnMeta, DbObject, ExecSummary, FilterRule, PageRequest, RowBatch, SchemaRef, ServerInfo,
    TablePage, TableRef, TableStructure,
};
use tokio::sync::mpsc;

/// How one statement runs.
#[derive(Clone, Copy, Debug)]
pub struct ExecOptions {
    /// Wrap in a READ ONLY transaction, then always roll back.
    pub read_only_tx: bool,
    /// Give up after this long.
    pub timeout: Option<Duration>,
    /// Stop reading after this many rows.
    pub max_rows: Option<u64>,
    /// Rows per batch.
    pub batch_size: usize,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            read_only_tx: false,
            timeout: None,
            max_rows: None,
            batch_size: 500,
        }
    }
}

/// What a statement streams while it runs.
#[derive(Clone, Debug, PartialEq)]
pub enum RowEvent {
    /// The columns, sent once before any rows.
    Columns(Vec<ColumnMeta>),
    /// A batch of rows.
    Batch(RowBatch),
}

/// Where rows go. Bounded, so a slow reader applies backpressure to the database read.
pub type RowSink = mpsc::Sender<RowEvent>;

/// Cancels whatever one connection is running.
#[async_trait::async_trait]
pub trait Canceller: Send + Sync {
    /// Asks the server to stop. Succeeds when nothing is running.
    async fn cancel(&self) -> Result<(), DbError>;
}

/// Which connection [`Session::acquire`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnScope {
    /// The open manual transaction's connection if there is one, otherwise a free one.
    Current,
    /// Always a separate connection, outside any manual transaction.
    Isolated,
}

/// One connection, held for a whole script so statements share it.
#[async_trait::async_trait]
pub trait Conn: Send {
    /// Runs one statement and streams rows into `sink` in batches.
    async fn execute(&mut self, stmt: &str, opts: ExecOptions, sink: RowSink)
        -> Result<ExecSummary, DbError>;

    /// A handle that can cancel what this connection is running.
    fn canceller(&self) -> Arc<dyn Canceller>;
}

/// An open session on one engine.
#[async_trait::async_trait]
pub trait Session: Send + Sync {
    /// Facts about the server.
    async fn server_info(&self) -> Result<ServerInfo, DbError>;
    /// Lists the databases on the server. Empty when a connection only ever has one.
    async fn list_databases(&self) -> Result<Vec<String>, DbError> {
        Ok(Vec::new())
    }
    /// Makes a database the default for every later connection of this session.
    async fn use_database(&self, _name: &str) -> Result<(), DbError> {
        Err(DbError::Unsupported(
            "this connection cannot change database".to_owned(),
        ))
    }
    /// The schema unqualified names resolve in, where the engine has one.
    async fn current_schema(&self) -> Result<Option<String>, DbError> {
        Ok(None)
    }
    /// Makes a schema the first place unqualified names resolve, for every later connection.
    async fn use_schema(&self, _name: &str) -> Result<(), DbError> {
        Err(DbError::Unsupported(
            "this connection has no schemas to choose".to_owned(),
        ))
    }
    /// Lists schemas.
    async fn list_schemas(&self) -> Result<Vec<SchemaRef>, DbError>;
    /// Lists tables and views in a schema.
    async fn list_objects(&self, schema: &SchemaRef) -> Result<Vec<DbObject>, DbError>;
    /// Lists every column in a schema.
    async fn columns(&self, schema: &SchemaRef) -> Result<Vec<ColumnInfo>, DbError>;
    /// Describes one table or view.
    async fn table_structure(&self, table: &TableRef) -> Result<TableStructure, DbError>;

    /// Hands out a connection.
    async fn acquire(&self, scope: ConnScope) -> Result<Box<dyn Conn>, DbError>;

    /// Reads one page of a table, with bound filter values.
    async fn table_page(&self, table: &TableRef, page: &PageRequest) -> Result<TablePage, DbError>;
    /// Counts rows, returning `None` if `timeout` passes first.
    async fn table_count(
        &self,
        table: &TableRef,
        filters: &[FilterRule],
        timeout: Duration,
    ) -> Result<Option<u64>, DbError>;
    /// Returns the JSON plan of one statement. Never executes it.
    async fn explain(&self, stmt: &str) -> Result<String, DbError>;

    /// Starts a manual transaction, pinning a connection until commit or rollback.
    async fn begin(&self, read_only: bool) -> Result<(), DbError>;
    /// Commits the manual transaction.
    async fn commit(&self) -> Result<(), DbError>;
    /// Rolls back the manual transaction.
    async fn rollback(&self) -> Result<(), DbError>;

    /// Closes every connection.
    async fn close(&self);

    /// Runs one statement on a free connection.
    async fn execute(&self, stmt: &str, opts: ExecOptions, sink: RowSink) -> Result<ExecSummary, DbError> {
        self.acquire(ConnScope::Current)
            .await?
            .execute(stmt, opts, sink)
            .await
    }
}
