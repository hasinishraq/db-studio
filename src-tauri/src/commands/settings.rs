//! Preferences.

use crate::error::AppResult;
use crate::state::AppState;
use studio_types::{AppError, Settings};
use tauri::State;

/// Refuses settings that are out of bounds.
pub fn check(settings: &Settings) -> Result<(), AppError> {
    if settings.history_limit == 0 || settings.history_limit > 100_000 {
        return Err(AppError::invalid(
            "the history limit must be between 1 and 100000",
        ));
    }
    let endpoint = settings.ai_endpoint.to_ascii_lowercase();
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        return Err(AppError::invalid(
            "the AI endpoint must start with http:// or https://",
        ));
    }
    if settings.ai_endpoint.len() > 2048 || settings.ai_model.len() > 500 {
        return Err(AppError::invalid("the AI settings are too long"));
    }
    Ok(())
}

/// Reads the preferences.
#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, AppError> {
    state.store.get_settings().await.app()
}

/// Writes the preferences.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<(), AppError> {
    check(&settings)?;
    state.store.set_settings(&settings).await.app()
}
