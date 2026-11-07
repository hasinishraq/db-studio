//! The per-engine entry point.

use crate::config::ConnectConfig;
use crate::error::DbError;
use crate::session::Session;
use studio_types::{Dialect, Engine};

/// One database engine.
#[async_trait::async_trait]
pub trait Driver: Send + Sync + 'static {
    /// Which engine this is.
    fn engine(&self) -> Engine;

    /// The SQL dialect, used by `studio-safety`.
    fn dialect(&self) -> Dialect {
        self.engine()
    }

    /// Opens a session.
    async fn connect(&self, cfg: &ConnectConfig) -> Result<Box<dyn Session>, DbError>;
}
