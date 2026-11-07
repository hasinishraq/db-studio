//! Maps library errors to the stable `AppError`.

use studio_db::DbError;
use studio_session::SessionError;
use studio_store::StoreError;
use studio_types::{AppError, DbErrorDetail, ErrorCode};

/// Conversion to [`AppError`]. A trait because both types live in other crates.
pub trait ToApp {
    /// The error the frontend sees.
    fn to_app(self) -> AppError;
}

impl ToApp for DbError {
    fn to_app(self) -> AppError {
        let code = match &self {
            DbError::Tls(_) => ErrorCode::Tls,
            DbError::Timeout => ErrorCode::Timeout,
            DbError::Cancelled => ErrorCode::Cancelled,
            DbError::ReadOnlyViolation => ErrorCode::ReadOnlyProfile,
            DbError::Internal(_) => ErrorCode::Internal,
            DbError::Connect(_) | DbError::Auth(_) | DbError::Query { .. } | DbError::Unsupported(_) => {
                ErrorCode::Database
            }
        };
        let mut error = AppError::new(code, self.to_string());
        if let DbError::Query {
            sqlstate, position, ..
        } = self
        {
            error.detail = Some(DbErrorDetail {
                sqlstate,
                position,
                hint: None,
            });
        }
        error
    }
}

impl ToApp for SessionError {
    fn to_app(self) -> AppError {
        let code = match &self {
            SessionError::NotConnected => ErrorCode::NotConnected,
            SessionError::ApprovalRequired => ErrorCode::ApprovalRequired,
            SessionError::ApprovalExpired => ErrorCode::ApprovalExpired,
            SessionError::ApprovalMismatch => ErrorCode::ApprovalMismatch,
            SessionError::ReadOnlyProfile => ErrorCode::ReadOnlyProfile,
            SessionError::DirtyTransaction => ErrorCode::DirtyTransaction,
            SessionError::Refused(_) => ErrorCode::InvalidInput,
            SessionError::NoDriver => ErrorCode::Internal,
            SessionError::Tunnel(_) => ErrorCode::Ssh,
            SessionError::HostKey {
                message,
                changed,
                detail,
            } => {
                let code = if *changed {
                    ErrorCode::HostKeyChanged
                } else {
                    ErrorCode::HostKeyUnknown
                };
                let mut error = AppError::new(code, message.clone());
                error.host_key = Some(detail.clone());
                return error;
            }
            SessionError::Db(inner) => return inner.clone().to_app(),
        };
        AppError::new(code, self.to_string())
    }
}

impl ToApp for StoreError {
    fn to_app(self) -> AppError {
        let code = match &self {
            StoreError::Secret(_) => ErrorCode::Secret,
            StoreError::NotFound(_) => ErrorCode::InvalidInput,
            _ => ErrorCode::Internal,
        };
        AppError::new(code, self.to_string())
    }
}

/// Shorthand for `.map_err(ToApp::to_app)`.
pub trait AppResult<T> {
    /// Converts the error side.
    fn app(self) -> Result<T, AppError>;
}

impl<T, E: ToApp> AppResult<T> for Result<T, E> {
    fn app(self) -> Result<T, AppError> {
        self.map_err(ToApp::to_app)
    }
}
