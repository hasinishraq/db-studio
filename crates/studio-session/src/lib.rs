//! The single place where SQL meets a database: sessions, policy, approvals, runs and transactions.

mod audit;
mod manager;
mod open;
mod policy;
mod run;
mod tx;

#[cfg(feature = "testing")]
pub mod testing;

pub use audit::{AuditSink, NullAuditSink};
pub use manager::{SessionError, SessionManager};
pub use open::{Connector, OpenSession, Route};
pub use policy::SessionPolicy;
pub use run::{AiQueryResult, RunOptions, MAX_GRID_CHANGES};
