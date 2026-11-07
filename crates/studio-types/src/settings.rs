//! User preferences.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Colour theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Theme {
    /// Follow the OS.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

/// Which provider protocol to speak.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AiProviderKind {
    /// Ollama.
    #[default]
    Ollama,
    /// OpenAI or a compatible endpoint.
    OpenAi,
}

/// All preferences. Unknown or missing fields fall back to defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Colour theme.
    pub theme: Theme,
    /// Maximum history entries kept.
    pub history_limit: u32,
    /// AI provider.
    pub ai_provider: AiProviderKind,
    /// Provider base URL.
    pub ai_endpoint: String,
    /// Model name.
    pub ai_model: String,
    /// Let the model read result samples.
    pub ai_share_rows: bool,
    /// Run safe AI reads without asking.
    pub auto_run_reads: bool,
    /// Encrypt the app database.
    pub encrypt_store: bool,
    /// Hide masked columns in the grid too.
    pub mask_in_grid: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            history_limit: 1000,
            ai_provider: AiProviderKind::Ollama,
            ai_endpoint: "http://localhost:11434".to_owned(),
            ai_model: "llama3.1".to_owned(),
            ai_share_rows: false,
            auto_run_reads: true,
            encrypt_store: false,
            mask_in_grid: false,
        }
    }
}
