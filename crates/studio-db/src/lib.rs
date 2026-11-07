//! The engine-neutral database interface every driver implements.

pub mod config;
pub mod driver;
pub mod error;
pub mod session;
pub mod sql;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use config::{ConnectConfig, TlsConfig};
pub use driver::Driver;
pub use error::DbError;
pub use session::{Canceller, Conn, ConnScope, ExecOptions, RowEvent, RowSink, Session};
pub use studio_types::{Dialect, Engine, ExecSummary, RowBatch, ServerInfo, Value};
