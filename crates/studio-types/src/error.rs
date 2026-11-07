//! The error every command returns.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;

/// Stable, machine-readable error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ErrorCode {
    /// The session does not exist.
    NotConnected,
    /// A write was attempted without a token.
    ApprovalRequired,
    /// The token expired.
    ApprovalExpired,
    /// The token does not match the SQL, session or origin.
    ApprovalMismatch,
    /// The profile refuses writes.
    ReadOnlyProfile,
    /// Timed out.
    Timeout,
    /// Cancelled.
    Cancelled,
    /// The database reported an error.
    Database,
    /// TLS failed.
    Tls,
    /// SSH failed.
    Ssh,
    /// The bastion's host key is not yet trusted.
    HostKeyUnknown,
    /// The bastion's host key changed.
    HostKeyChanged,
    /// A manual transaction has uncommitted work.
    DirtyTransaction,
    /// The secret store failed.
    Secret,
    /// The AI subsystem failed.
    Ai,
    /// Input failed validation.
    InvalidInput,
    /// Anything else.
    Internal,
}

/// Database details for underlining errors in the editor.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DbErrorDetail {
    /// SQLSTATE.
    pub sqlstate: Option<String>,
    /// 1-based character position in the statement.
    pub position: Option<u32>,
    /// Server hint.
    pub hint: Option<String>,
}

/// What the UI needs to confirm or refuse an SSH host key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyDetail {
    /// Bastion host.
    pub host: String,
    /// Bastion port.
    pub port: u16,
    /// Key algorithm, when the key is new.
    pub key_type: Option<String>,
    /// The fingerprint the server presented.
    pub fingerprint: String,
    /// The pinned fingerprint, when the key changed.
    pub expected: Option<String>,
}

/// What every command returns on failure. Never contains a secret or a row value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    /// Stable code.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Database details.
    pub detail: Option<DbErrorDetail>,
    /// SSH host key details.
    #[serde(default)]
    pub host_key: Option<Box<HostKeyDetail>>,
}

impl AppError {
    /// Creates an error with no database detail.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            host_key: None,
        }
    }

    /// Shorthand for [`ErrorCode::InvalidInput`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}
