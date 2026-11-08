//! Profile commands. Passwords come in here and never go back out.

use super::check_text;
use crate::error::AppResult;
use crate::state::AppState;
use secrecy::SecretString;
use studio_store::{SecretKey, Store};
use studio_types::{AppError, ConnectionProfile, Engine, ProfileId};
use tauri::State;

fn validate(profile: &ConnectionProfile) -> Result<(), AppError> {
    check_text("the name", &profile.name, 200)?;
    if profile.database.len() > 4096 {
        return Err(AppError::invalid("the database is too long"));
    }
    if profile.engine == Engine::Sqlite {
        check_text("the database file", &profile.database, 4096)?;
    } else {
        check_text("the host", &profile.host, 255)?;
        check_text("the user name", &profile.username, 255)?;
        if profile.port == 0 {
            return Err(AppError::invalid("the port is required"));
        }
    }
    if let Some(schema) = &profile.default_schema {
        check_name("the default schema", schema)?;
        if profile.engine != Engine::Postgres {
            return Err(AppError::invalid("only PostgreSQL has schemas inside a database"));
        }
    }
    if let Some(ssh) = &profile.ssh {
        check_text("the SSH host", &ssh.host, 255)?;
        check_text("the SSH user", &ssh.username, 255)?;
        if ssh.port == 0 {
            return Err(AppError::invalid("the SSH port is required"));
        }
    }
    Ok(())
}

/// A database or schema name: not blank, at most 255 bytes, no control characters.
fn check_name(label: &str, name: &str) -> Result<(), AppError> {
    check_text(label, name, 255)?;
    if name.chars().any(char::is_control) {
        return Err(AppError::invalid(format!("{label} holds a control character")));
    }
    Ok(())
}

/// Remembers where a connection opens: a database, and on PostgreSQL a schema in it. `None` for
/// both forgets it, so the connection opens on the server's default. Running sessions are left
/// as they are.
pub async fn remember(
    store: &Store,
    id: ProfileId,
    database: Option<String>,
    schema: Option<String>,
) -> Result<ConnectionProfile, AppError> {
    let mut profile = store.get_profile(id).await.app()?;
    if profile.engine == Engine::Sqlite {
        return Err(AppError::invalid("a SQLite connection has one database"));
    }
    if let Some(name) = &database {
        check_name("the database", name)?;
    }
    if schema.is_some() && database.is_none() {
        return Err(AppError::invalid("a default schema needs its database"));
    }
    profile.database = database.unwrap_or_default();
    profile.default_schema = schema;
    validate(&profile)?;
    store.upsert_profile(&profile).await.app()?;
    store.get_profile(id).await.app()
}

/// Lists profiles.
pub async fn list(store: &Store) -> Result<Vec<ConnectionProfile>, AppError> {
    store.list_profiles().await.app()
}

/// Saves a profile, and its password when one is given. Returns it with fresh secret flags.
pub async fn save(
    store: &Store,
    profile: ConnectionProfile,
    password: Option<String>,
) -> Result<ConnectionProfile, AppError> {
    validate(&profile)?;
    store.upsert_profile(&profile).await.app()?;
    if let Some(password) = password {
        let key = SecretKey::ProfilePassword(profile.id);
        store
            .secrets()
            .set(&key, SecretString::from(password))
            .await
            .app()?;
    }
    store.get_profile(profile.id).await.app()
}

/// Lists saved connection profiles.
#[tauri::command]
#[specta::specta]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ConnectionProfile>, AppError> {
    list(&state.store).await
}

/// Saves a profile. The password is write-only.
#[tauri::command]
#[specta::specta]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    password: Option<String>,
) -> Result<ConnectionProfile, AppError> {
    save(&state.store, profile, password).await
}

/// Remembers, or forgets, the database and schema a connection opens on.
#[tauri::command]
#[specta::specta]
pub async fn remember_default(
    state: State<'_, AppState>,
    profile_id: ProfileId,
    database: Option<String>,
    schema: Option<String>,
) -> Result<ConnectionProfile, AppError> {
    remember(&state.store, profile_id, database, schema).await
}

/// Deletes a profile and its secrets.
#[tauri::command]
#[specta::specta]
pub async fn delete_profile(state: State<'_, AppState>, id: ProfileId) -> Result<(), AppError> {
    state.store.delete_profile(id).await.app()
}
