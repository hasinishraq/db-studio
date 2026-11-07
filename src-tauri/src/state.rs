//! Shared application state and app-wide events.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use studio_safety::ApprovalBook;
use studio_session::SessionManager;
use studio_store::Store;
use studio_types::SessionId;
use tauri_specta::Event;

/// Everything commands share.
pub struct AppState {
    /// App database and secrets.
    pub store: Store,
    /// Open connections and runs.
    pub sessions: Arc<SessionManager>,
    /// Pending write approvals. Only `approve_query` and `ai_review` may call `issue`.
    pub approvals: Arc<ApprovalBook>,
    /// AI turns.
    pub ai: Arc<studio_ai::Engine>,
    /// CSV files the user picked, by opaque id, so the webview never holds a path.
    pub import_files: dashmap::DashMap<String, std::path::PathBuf>,
    /// Keeps the log file writer alive.
    pub _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// A session was lost, for example because its SSH tunnel dropped.
#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionLost {
    /// The session.
    pub session_id: SessionId,
}

/// The OS credential store is unavailable, so secrets last only until the app quits.
#[derive(Clone, Debug, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreWarning {
    /// What went wrong.
    pub message: String,
}
