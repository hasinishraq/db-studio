//! Secret commands. There is deliberately no command that reads a secret.

use super::check_text;
use crate::error::AppResult;
use crate::state::AppState;
use secrecy::SecretString;
use studio_store::{SecretKey, Store};
use studio_types::{AppError, SecretTarget};
use tauri::State;

fn key_for(target: SecretTarget) -> Result<SecretKey, AppError> {
    Ok(match target {
        SecretTarget::ProfilePassword { profile_id } => SecretKey::ProfilePassword(profile_id),
        SecretTarget::SshSecret { profile_id } => SecretKey::SshSecret(profile_id),
        SecretTarget::AiKey { provider } => {
            check_text("the provider", &provider, 100)?;
            SecretKey::AiKey(provider)
        }
    })
}

/// Stores a secret.
pub async fn set(store: &Store, target: SecretTarget, value: String) -> Result<(), AppError> {
    if value.is_empty() {
        return Err(AppError::invalid("the secret is empty"));
    }
    store
        .secrets()
        .set(&key_for(target)?, SecretString::from(value))
        .await
        .app()
}

/// Removes a secret.
pub async fn clear(store: &Store, target: SecretTarget) -> Result<(), AppError> {
    store.secrets().delete(&key_for(target)?).await.app()
}

/// Stores a password, passphrase or API key.
#[tauri::command]
#[specta::specta]
pub async fn set_secret(
    state: State<'_, AppState>,
    target: SecretTarget,
    value: String,
) -> Result<(), AppError> {
    set(&state.store, target, value).await
}

/// Removes a password, passphrase or API key.
#[tauri::command]
#[specta::specta]
pub async fn clear_secret(state: State<'_, AppState>, target: SecretTarget) -> Result<(), AppError> {
    clear(&state.store, target).await
}
