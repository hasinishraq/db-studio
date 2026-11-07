//! Where audit records go. Defined here so this crate does not depend on the store.

use crate::manager::SessionManager;
use crate::open::OpenSession;
use studio_safety::sha256_hex;
use studio_types::{AuditEntry, Origin, StatementKind};

/// Receives one record per statement that is not a pure read.
#[async_trait::async_trait]
pub trait AuditSink: Send + Sync {
    /// Stores a record. A failure must be handled by the sink; it cannot undo the statement.
    async fn record(&self, entry: AuditEntry);
}

/// Discards records.
pub struct NullAuditSink;

#[async_trait::async_trait]
impl AuditSink for NullAuditSink {
    async fn record(&self, _entry: AuditEntry) {}
}

impl SessionManager {
    /// Records the outcome of one non-read statement.
    pub(crate) async fn audit_statement(
        &self,
        open: &OpenSession,
        origin: Origin,
        kind: StatementKind,
        sql: &str,
        rows_affected: Option<i64>,
        outcome: &str,
    ) {
        self.audit
            .record(AuditEntry {
                at: chrono::Utc::now().timestamp_millis(),
                profile_id: Some(open.profile.id),
                environment: open.profile.environment,
                origin,
                kind,
                sql_hash: sha256_hex(sql),
                sql_text: sql.to_owned(),
                rows_affected,
                outcome: outcome.to_owned(),
            })
            .await;
    }
}
