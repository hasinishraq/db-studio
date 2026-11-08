//! User preferences.

use crate::ExportFormat;
use serde::{Deserialize, Serialize};
use specta::Type;

/// How tall result rows are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum RowDensity {
    /// More rows on screen.
    #[default]
    Compact,
    /// Easier to read.
    Roomy,
}

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
    /// Editor font size in pixels.
    pub editor_font_size: u32,
    /// Spaces per tab in the editor.
    pub editor_tab_size: u32,
    /// Wrap long lines in the editor.
    pub editor_word_wrap: bool,
    /// Show line numbers.
    pub editor_line_numbers: bool,
    /// Show the editor's overview map.
    pub editor_minimap: bool,
    /// Suggest names while typing.
    pub editor_suggestions: bool,
    /// Give up on a statement after this many seconds; 0 means never.
    pub statement_timeout_secs: u32,
    /// Stop reading a result after this many rows; 0 means no limit.
    pub row_limit: u32,
    /// What an empty cell shows.
    pub null_text: String,
    /// Result row height.
    pub row_density: RowDensity,
    /// Open a query tab when a connection opens.
    pub open_query_on_connect: bool,
    /// The format the Export button uses.
    pub export_format: ExportFormat,
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
            editor_font_size: 13,
            editor_tab_size: 2,
            editor_word_wrap: false,
            editor_line_numbers: true,
            editor_minimap: false,
            editor_suggestions: true,
            statement_timeout_secs: 0,
            row_limit: 0,
            null_text: "NULL".to_owned(),
            row_density: RowDensity::Compact,
            open_query_on_connect: false,
            export_format: ExportFormat::Csv,
        }
    }
}
