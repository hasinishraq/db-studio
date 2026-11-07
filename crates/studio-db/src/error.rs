//! Driver errors. Messages must never contain a password or a row value.

/// What can go wrong talking to a database.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DbError {
    /// Could not reach the server.
    #[error("connection failed: {0}")]
    Connect(String),
    /// TLS negotiation or verification failed.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The server rejected the credentials.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// The server rejected a statement.
    #[error("{message}")]
    Query {
        /// SQLSTATE.
        sqlstate: Option<String>,
        /// Server message.
        message: String,
        /// 1-based character position.
        position: Option<u32>,
    },
    /// The statement ran out of time.
    #[error("the statement timed out")]
    Timeout,
    /// The statement was cancelled.
    #[error("the statement was cancelled")]
    Cancelled,
    /// A write was refused by a read-only transaction.
    #[error("the database refused a write in a read-only transaction")]
    ReadOnlyViolation,
    /// The engine cannot do this.
    #[error("not supported: {0}")]
    Unsupported(String),
    /// Anything else.
    #[error("internal error: {0}")]
    Internal(String),
}

impl DbError {
    /// A query error with only a message.
    pub fn query(message: impl Into<String>) -> Self {
        DbError::Query {
            sqlstate: None,
            message: message.into(),
            position: None,
        }
    }
}
