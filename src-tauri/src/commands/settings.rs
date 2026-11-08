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
    if !(8..=32).contains(&settings.editor_font_size) {
        return Err(AppError::invalid("the editor font size must be between 8 and 32"));
    }
    if !(1..=8).contains(&settings.editor_tab_size) {
        return Err(AppError::invalid("the tab size must be between 1 and 8"));
    }
    if settings.statement_timeout_secs > 86_400 {
        return Err(AppError::invalid("the statement timeout can be at most a day"));
    }
    if settings.row_limit > 100_000_000 {
        return Err(AppError::invalid("the row limit can be at most 100,000,000"));
    }
    if settings.null_text.chars().count() > 16 || settings.null_text.chars().any(char::is_control) {
        return Err(AppError::invalid("NULL may show as at most 16 plain characters"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_pass() {
        assert!(check(&Settings::default()).is_ok());
    }

    #[test]
    fn values_out_of_range_are_refused() {
        let base = Settings::default;
        for bad in [
            Settings {
                editor_font_size: 7,
                ..base()
            },
            Settings {
                editor_font_size: 33,
                ..base()
            },
            Settings {
                editor_tab_size: 0,
                ..base()
            },
            Settings {
                statement_timeout_secs: 86_401,
                ..base()
            },
            Settings {
                row_limit: 100_000_001,
                ..base()
            },
            Settings {
                null_text: "x".repeat(17),
                ..base()
            },
            Settings {
                null_text: "a\nb".into(),
                ..base()
            },
        ] {
            assert!(check(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn old_saved_settings_still_load() {
        let old: Settings = serde_json::from_str(r#"{"theme":"Dark","historyLimit":50}"#).unwrap_or_default();
        assert_eq!(old.history_limit, 50);
        assert_eq!(old.editor_font_size, 13);
        assert_eq!(old.null_text, "NULL");
    }
}
