//! Commands: thin wrappers that validate input and call plain, testable functions.

pub mod ai;
pub mod audit;
pub mod drafts;
pub mod history;
pub mod profiles;
pub mod query;
pub mod rows;
pub mod schema;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod ssh;
pub mod system;
pub mod transfer;

#[cfg(test)]
mod tests;

use studio_types::AppError;

/// The longest SQL accepted from the editor.
pub const MAX_SQL_BYTES: usize = 1024 * 1024;

/// Refuses SQL that is empty or too long.
pub fn check_sql(sql: &str) -> Result<(), AppError> {
    if sql.trim().is_empty() {
        return Err(AppError::invalid("the SQL is empty"));
    }
    if sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid("the SQL is longer than 1 MB"));
    }
    Ok(())
}

/// Refuses text that is empty or too long.
pub fn check_text(label: &str, value: &str, max: usize) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::invalid(format!("{label} is required")));
    }
    if value.len() > max {
        return Err(AppError::invalid(format!("{label} is too long")));
    }
    Ok(())
}
