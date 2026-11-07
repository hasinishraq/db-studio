//! Connecting and disconnecting.

use crate::error::AppResult;
use crate::state::AppState;
use secrecy::SecretString;
use studio_session::SessionManager;
use studio_store::{SecretKey, Store};
use studio_types::{AppError, ConnectionProfile, ProfileId, ServerInfo, SessionId, SessionInfo};
use tauri::State;

/// Reads a profile's secrets. They go from the keychain straight to the session manager.
async fn secrets_for(
    store: &Store,
    id: ProfileId,
) -> Result<(Option<SecretString>, Option<SecretString>), AppError> {
    let password = store.secrets().get(&SecretKey::ProfilePassword(id)).await.app()?;
    let ssh = store.secrets().get(&SecretKey::SshSecret(id)).await.app()?;
    Ok((password, ssh))
}

/// Opens a session for a saved profile.
pub async fn open(store: &Store, sessions: &SessionManager, id: ProfileId) -> Result<SessionInfo, AppError> {
    let profile = store.get_profile(id).await.app()?;
    let (password, ssh) = secrets_for(store, id).await?;
    sessions.connect(profile, password, ssh).await.app()
}

/// Tests a possibly unsaved profile. A new password wins over the stored one.
pub async fn test(
    store: &Store,
    sessions: &SessionManager,
    profile: ConnectionProfile,
    password: Option<String>,
) -> Result<ServerInfo, AppError> {
    let (stored, ssh) = secrets_for(store, profile.id).await?;
    let password = password.map(SecretString::from).or(stored);
    sessions.test_connection(&profile, password, ssh).await.app()
}

/// Connects, reports the server version and TLS status, and disconnects.
#[tauri::command]
#[specta::specta]
pub async fn test_connection(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    password: Option<String>,
) -> Result<ServerInfo, AppError> {
    test(&state.store, &state.sessions, profile, password).await
}

/// Opens a session.
#[tauri::command]
#[specta::specta]
pub async fn connect(state: State<'_, AppState>, profile_id: ProfileId) -> Result<SessionInfo, AppError> {
    open(&state.store, &state.sessions, profile_id).await
}

/// Closes a session. `force` discards uncommitted work.
#[tauri::command]
#[specta::specta]
pub async fn disconnect(
    state: State<'_, AppState>,
    session_id: SessionId,
    force: bool,
) -> Result<(), AppError> {
    state.sessions.disconnect(session_id, force).await.app()
}
