//! Logging: a daily file, plus stderr in debug builds. Never SQL parameters, rows or secrets.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Starts logging into `dir`. Returns the guard that flushes the file on exit.
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let appender = tracing_appender::rolling::daily(dir, "studio.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tokio_postgres=warn"));
    let file = fmt::layer().with_writer(writer).with_ansi(false);
    let console = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(filter)
        .with(file)
        .with(console)
        .try_init()
        .ok()?;
    Some(guard)
}
