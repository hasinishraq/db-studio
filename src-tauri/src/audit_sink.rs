//! Stores audit records.

use studio_session::AuditSink;
use studio_store::Store;
use studio_types::AuditEntry;

/// Writes audit records to the app database.
pub struct StoreAuditSink {
    /// The database.
    pub store: Store,
}

#[async_trait::async_trait]
impl AuditSink for StoreAuditSink {
    async fn record(&self, entry: AuditEntry) {
        if let Err(error) = self.store.audit_append(&entry).await {
            // The statement already ran; all that can be done is say so loudly.
            tracing::error!(%error, "could not write an audit record");
        }
    }
}
