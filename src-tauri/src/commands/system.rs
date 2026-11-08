//! Diagnostics and the first-launch import from the Electron version.

use crate::error::AppResult;
use crate::state::AppState;
use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use studio_store::Store;
use studio_types::{
    AiPolicy, AppError, ConnectionProfile, Diagnostics, Draft, DraftId, Engine, Environment,
    LegacyImportSummary, ProfileId, TlsMode,
};
use tauri::{AppHandle, Manager, State};

const LOG_LINES: usize = 200;

/// Words whose lines are dropped from diagnostics.
const SENSITIVE: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "passphrase",
];

/// Keeps the last lines, removing anything that could hold a secret.
pub fn clean_log_tail(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(LOG_LINES);
    lines[start..]
        .iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            !SENSITIVE.iter().any(|word| lower.contains(word))
        })
        .map(|line| (*line).to_owned())
        .collect()
}

fn latest_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("studio.log"))
        .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok())
        .map(|entry| entry.path())
}

/// Collects facts for a bug report.
#[tauri::command]
#[specta::specta]
pub async fn diagnostics(app: AppHandle) -> Result<Diagnostics, AppError> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::invalid(e.to_string()))?;
    let text = latest_log(&log_dir)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    Ok(Diagnostics {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        log_tail: clean_log_tail(&text),
    })
}

fn string(value: &Json, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key)?.as_str().map(str::to_owned))
}

fn number(value: &Json, keys: &[&str]) -> Option<u16> {
    keys.iter().find_map(|key| {
        let field = value.get(*key)?;
        field
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .or_else(|| field.as_str()?.parse().ok())
    })
}

fn legacy_engine(value: &Json) -> Option<Engine> {
    match string(value, &["engine", "type", "dialect", "driver"])?
        .to_lowercase()
        .as_str()
    {
        "postgres" | "postgresql" | "pg" => Some(Engine::Postgres),
        "mysql" | "mariadb" => Some(Engine::MySql),
        "sqlite" | "sqlite3" => Some(Engine::Sqlite),
        _ => None,
    }
}

/// Reads one old profile. Secrets were encrypted with Electron's safeStorage and cannot be read here.
fn legacy_profile(value: &Json, notes: &mut Vec<String>) -> Option<ConnectionProfile> {
    let engine = legacy_engine(value)?;
    let name = string(value, &["name", "label", "title"])?;
    let tls = string(value, &["sslMode", "tlsMode", "ssl"])
        .unwrap_or_default()
        .to_lowercase();
    let tls_mode = match tls.as_str() {
        "disable" | "off" | "false" => TlsMode::Disable,
        "verify-full" | "verifyfull" => TlsMode::VerifyFull,
        "verify-ca" | "verifyca" => TlsMode::VerifyCa,
        _ => {
            notes.push(format!(
                "{name}: TLS was not verified before. Consider Verify full."
            ));
            TlsMode::Require
        }
    };
    Some(ConnectionProfile {
        id: ProfileId::new(),
        name,
        engine,
        host: string(value, &["host", "hostname"]).unwrap_or_default(),
        port: number(value, &["port"]).unwrap_or_else(|| engine.default_port()),
        database: string(value, &["database", "db", "file", "path"]).unwrap_or_default(),
        username: string(value, &["username", "user"]).unwrap_or_default(),
        tls_mode,
        ca_cert_path: None,
        client_cert_path: None,
        client_key_path: None,
        ssh: None,
        environment: Environment::Development,
        read_only: value.get("readOnly").and_then(Json::as_bool).unwrap_or(false),
        ai_policy: AiPolicy::Default,
        color: string(value, &["color"]),
        default_schema: None,
        has_password: false,
    })
}

fn items<'a>(root: &'a Json, keys: &[&str]) -> Vec<&'a Json> {
    keys.iter()
        .find_map(|key| root.get(*key)?.as_array())
        .map(|list| list.iter().collect())
        .unwrap_or_default()
}

/// Imports old data into the store, keeping a backup of the file.
pub async fn import_legacy_file(store: &Store, path: &Path) -> Result<LegacyImportSummary, AppError> {
    let invalid = |e: &dyn std::fmt::Display| AppError::invalid(format!("the old data is unreadable: {e}"));
    let text = std::fs::read_to_string(path).map_err(|e| invalid(&e))?;
    let root: Json = serde_json::from_str(&text).map_err(|e| invalid(&e))?;
    std::fs::copy(path, path.with_extension("json.backup")).map_err(|e| invalid(&e))?;

    let mut summary = LegacyImportSummary {
        found: true,
        ..Default::default()
    };
    for value in items(&root, &["profiles", "connections"]) {
        let mut notes = Vec::new();
        if let Some(profile) = legacy_profile(value, &mut notes) {
            store.upsert_profile(&profile).await.app()?;
            summary.profiles += 1;
            summary.notes.append(&mut notes);
        }
    }
    let now = chrono::Utc::now().timestamp_millis();
    for value in items(&root, &["drafts", "savedQueries"]) {
        let Some(sql) = string(value, &["sql", "query", "text"]) else {
            continue;
        };
        let title = string(value, &["title", "name"]).unwrap_or_else(|| "Imported query".to_owned());
        let draft = Draft {
            id: DraftId::new(),
            profile_id: None,
            title,
            saved_sql: sql.clone(),
            sql,
            updated_at: now,
        };
        store.save_draft(&draft).await.app()?;
        summary.drafts += 1;
    }
    if summary.profiles > 0 {
        summary
            .notes
            .push("Passwords could not be carried over. Please enter them again.".to_owned());
    }
    Ok(summary)
}

fn legacy_candidates(app: &AppHandle) -> Vec<PathBuf> {
    let Ok(config) = app.path().config_dir() else {
        return Vec::new();
    };
    ["AI DB Studio", "ai-db-studio"]
        .iter()
        .map(|name| config.join(name).join("studio-storage.json"))
        .collect()
}

/// Imports the Electron version's data once, on first launch.
#[tauri::command]
#[specta::specta]
pub async fn import_legacy(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LegacyImportSummary, AppError> {
    if !state.store.list_profiles().await.app()?.is_empty() {
        return Ok(LegacyImportSummary::default());
    }
    for path in legacy_candidates(&app) {
        if path.exists() {
            return import_legacy_file(&state.store, &path).await;
        }
    }
    Ok(LegacyImportSummary::default())
}

#[cfg(test)]
mod system_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::sync::Arc;
    use studio_store::MemorySecretStore;

    #[test]
    fn diagnostics_drop_lines_that_could_hold_a_secret() {
        let log = "INFO started\nWARN Password rejected for x\nINFO api_key=abc\nINFO done";
        assert_eq!(clean_log_tail(log), ["INFO started", "INFO done"]);
    }

    #[test]
    fn diagnostics_keep_the_last_lines_only() {
        let log: String = (0..500).map(|i| format!("line {i}\n")).collect();
        let tail = clean_log_tail(&log);
        assert_eq!(tail.len(), LOG_LINES);
        assert_eq!(tail.last().unwrap(), "line 499");
    }

    #[tokio::test]
    async fn legacy_profiles_import_without_passwords_and_with_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path(), Arc::new(MemorySecretStore::new()))
            .await
            .unwrap();
        let path = dir.path().join("studio-storage.json");
        let data = serde_json::json!({
            "connections": [
                { "name": "old", "type": "postgresql", "host": "db", "port": 5432, "database": "app",
                  "username": "me", "sslMode": "prefer", "password": "encrypted-blob" },
                { "name": "skip", "type": "oracle" }
            ],
            "drafts": [ { "title": "q", "sql": "SELECT 1" } ]
        });
        std::fs::write(&path, data.to_string()).unwrap();
        let summary = import_legacy_file(&store, &path).await.unwrap();
        assert_eq!((summary.profiles, summary.drafts), (1, 1));
        let profile = &store.list_profiles().await.unwrap()[0];
        assert_eq!(
            profile.tls_mode,
            TlsMode::Require,
            "PREFER becomes Require with a warning"
        );
        assert!(!profile.has_password);
        assert!(summary.notes.iter().any(|n| n.contains("Verify full")));
        assert!(path.with_extension("json.backup").exists());
    }
}
