//! PostgreSQL driver: verified TLS, streaming rows, protocol-level cancel, read-only transactions.

mod connect;
mod decode;
mod execute;
mod metadata;
mod page;
mod tls;

pub use connect::PostgresDriver;
