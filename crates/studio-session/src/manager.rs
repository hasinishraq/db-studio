//! The session manager.

use crate::audit::AuditSink;
use crate::open::{Connector, OpenSession};
use crate::run::RunHandle;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use studio_db::{DbError, Driver};
use studio_safety::{verdicts, ApprovalBook, SafetyError};
use studio_types::{
    ColumnInfo, DbObject, Engine, FilterRule, PageRequest, RunId, SchemaRef, SessionId, StatementVerdict,
    TablePage, TableRef, TableStructure,
};

/// How long EXPLAIN may take.
const EXPLAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Why something was refused or failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// The session does not exist.
    #[error("not connected")]
    NotConnected,
    /// A non-read statement arrived without a token.
    #[error("this statement needs approval")]
    ApprovalRequired,
    /// The token expired.
    #[error("the approval expired")]
    ApprovalExpired,
    /// The token does not fit this SQL, session or door.
    #[error("the approval does not match this statement")]
    ApprovalMismatch,
    /// The profile refuses writes.
    #[error("this connection is read-only")]
    ReadOnlyProfile,
    /// The statement may not run here.
    #[error("{0}")]
    Refused(String),
    /// Uncommitted work would be lost.
    #[error("this session has uncommitted changes")]
    DirtyTransaction,
    /// No driver for the engine.
    #[error("no driver for this engine")]
    NoDriver,
    /// An SSH tunnel failed.
    #[error("{0}")]
    Tunnel(String),
    /// The SSH host key is new or changed.
    #[error("{message}")]
    HostKey {
        /// Human-readable text.
        message: String,
        /// Whether the key changed, rather than being new.
        changed: bool,
        /// What the UI needs.
        detail: Box<studio_types::HostKeyDetail>,
    },
    /// The driver failed.
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<SafetyError> for SessionError {
    fn from(error: SafetyError) -> Self {
        match error {
            SafetyError::NoApproval => SessionError::ApprovalRequired,
            SafetyError::Expired => SessionError::ApprovalExpired,
            SafetyError::Mismatch => SessionError::ApprovalMismatch,
        }
    }
}

/// Owns every open connection and every running query.
pub struct SessionManager {
    pub(crate) drivers: HashMap<Engine, Arc<dyn Driver>>,
    pub(crate) sessions: DashMap<SessionId, Arc<OpenSession>>,
    pub(crate) runs: DashMap<RunId, Arc<RunHandle>>,
    pub(crate) approvals: Arc<ApprovalBook>,
    pub(crate) audit: Arc<dyn AuditSink>,
    pub(crate) connector: Option<Arc<dyn Connector>>,
}

impl SessionManager {
    /// Creates a manager with no tunnel support.
    pub fn new(
        drivers: HashMap<Engine, Arc<dyn Driver>>,
        approvals: Arc<ApprovalBook>,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        Self {
            drivers,
            sessions: DashMap::new(),
            runs: DashMap::new(),
            approvals,
            audit,
            connector: None,
        }
    }

    /// Adds the tunnel opener used by profiles with an SSH bastion.
    pub fn with_connector(mut self, connector: Arc<dyn Connector>) -> Self {
        self.connector = Some(connector);
        self
    }

    /// An open session.
    pub fn get(&self, id: SessionId) -> Result<Arc<OpenSession>, SessionError> {
        self.sessions
            .get(&id)
            .map(|s| s.clone())
            .ok_or(SessionError::NotConnected)
    }

    /// Splits and classifies a script for the UI. Only `studio-safety` decides.
    pub fn prepare(&self, id: SessionId, sql: &str) -> Result<Vec<StatementVerdict>, SessionError> {
        Ok(verdicts(sql, self.get(id)?.engine()))
    }

    /// Lists schemas.
    pub async fn list_schemas(&self, id: SessionId) -> Result<Vec<SchemaRef>, SessionError> {
        Ok(self.get(id)?.session.list_schemas().await?)
    }

    /// Lists tables and views.
    pub async fn list_objects(
        &self,
        id: SessionId,
        schema: &SchemaRef,
    ) -> Result<Vec<DbObject>, SessionError> {
        Ok(self.get(id)?.session.list_objects(schema).await?)
    }

    /// Lists every column of a schema.
    pub async fn columns(&self, id: SessionId, schema: &SchemaRef) -> Result<Vec<ColumnInfo>, SessionError> {
        Ok(self.get(id)?.session.columns(schema).await?)
    }

    /// Describes a table.
    pub async fn table_structure(
        &self,
        id: SessionId,
        table: &TableRef,
    ) -> Result<TableStructure, SessionError> {
        Ok(self.get(id)?.session.table_structure(table).await?)
    }

    /// Reads a page of a table.
    pub async fn table_page(
        &self,
        id: SessionId,
        table: &TableRef,
        page: &PageRequest,
    ) -> Result<TablePage, SessionError> {
        Ok(self.get(id)?.session.table_page(table, page).await?)
    }

    /// Counts rows, returning `None` on timeout.
    pub async fn table_count(
        &self,
        id: SessionId,
        table: &TableRef,
        filters: &[FilterRule],
    ) -> Result<Option<u64>, SessionError> {
        Ok(self
            .get(id)?
            .session
            .table_count(table, filters, Duration::from_secs(5))
            .await?)
    }

    /// Returns the plan of exactly one statement.
    pub async fn explain(&self, id: SessionId, sql: &str) -> Result<String, SessionError> {
        let open = self.get(id)?;
        if studio_safety::classify_script(sql, open.engine()).len() != 1 {
            return Err(SessionError::Refused(
                "EXPLAIN takes exactly one statement.".to_owned(),
            ));
        }
        match tokio::time::timeout(EXPLAIN_TIMEOUT, open.session.explain(sql)).await {
            Ok(result) => Ok(result?),
            Err(_) => Err(DbError::Timeout.into()),
        }
    }
}
