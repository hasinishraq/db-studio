//! Startup: paths, logging, TLS, storage, secrets and sessions.

use crate::audit_sink::StoreAuditSink;
use crate::commands::ssh::TunnelConnector;
use crate::logging;
use crate::state::{AppState, SecretStoreWarning};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use studio_db::Driver;
use studio_safety::ApprovalBook;
use studio_session::SessionManager;
use studio_store::{KeyringSecretStore, MemorySecretStore, SecretStore, Store};
use studio_types::Engine;
use tauri::{App, Manager};
use tauri_specta::Event;

/// Uses the OS credential store, or memory when it does not work. Returns any warning.
async fn choose_secret_store() -> (Arc<dyn SecretStore>, Option<String>) {
    let attempt = match KeyringSecretStore::new() {
        Ok(store) => store.probe().await.map(|()| store),
        Err(error) => Err(error),
    };
    match attempt {
        Ok(store) => (Arc::new(store), None),
        Err(error) => {
            tracing::warn!(%error, "falling back to in-memory secrets");
            let message = format!("Passwords will be kept only until you quit: {error}");
            (Arc::new(MemorySecretStore::new()), Some(message))
        }
    }
}

/// Writes a crash file. It records only where the panic happened: a panic message could quote SQL
/// or a value, and this file is meant to be shared in bug reports.
fn install_crash_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string);
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let text = format!("DB Studio {} panicked at {place}\n", env!("CARGO_PKG_VERSION"));
        let _ = std::fs::write(dir.join(format!("crash-{stamp}.txt")), text);
        previous(info);
    }));
}

/// The drivers this build ships.
pub fn drivers() -> HashMap<Engine, Arc<dyn Driver>> {
    HashMap::from([
        (
            Engine::Postgres,
            Arc::new(driver_postgres::PostgresDriver) as Arc<dyn Driver>,
        ),
        (
            Engine::MySql,
            Arc::new(driver_mysql::MySqlDriver) as Arc<dyn Driver>,
        ),
        (
            Engine::Sqlite,
            Arc::new(driver_sqlite::SqliteDriver) as Arc<dyn Driver>,
        ),
    ])
}

/// Where `studio.db` lives. Debug builds can be pointed elsewhere, so end-to-end runs leave a
/// developer's own connections alone. Release builds always use the platform folder.
fn data_dir(app: &App) -> tauri::Result<PathBuf> {
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("STUDIO_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    app.path().app_data_dir()
}

/// Builds and registers [`AppState`].
pub fn init(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = data_dir(app)?;
    let log_dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let log_guard = logging::init(&log_dir);
    install_crash_hook(log_dir.clone());

    // mysql_async uses the process-wide provider. Failing means one is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (secrets, warning) = tauri::async_runtime::block_on(choose_secret_store());
    let store = tauri::async_runtime::block_on(Store::open(&data_dir, secrets))?;

    let approvals = Arc::new(ApprovalBook::new());
    let audit = Arc::new(StoreAuditSink { store: store.clone() });
    let connector = Arc::new(TunnelConnector {
        store: store.clone(),
        app: Some(app.handle().clone()),
        sessions: OnceLock::new(),
    });
    let sessions =
        Arc::new(SessionManager::new(drivers(), approvals.clone(), audit).with_connector(connector.clone()));
    let _ = connector.sessions.set(Arc::downgrade(&sessions));
    let ai = Arc::new(studio_ai::Engine::new(
        sessions.clone(),
        store.clone(),
        Arc::new(studio_ai::AgentHub::default()),
    ));
    app.manage(AppState {
        store,
        sessions,
        approvals,
        ai,
        import_files: dashmap::DashMap::new(),
        _log_guard: log_guard,
    });

    if let Some(message) = warning {
        let _ = SecretStoreWarning { message }.emit(app.handle());
    }
    Ok(())
}
