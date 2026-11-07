//! What a driver needs to connect. Built by the session manager.

use secrecy::SecretString;
use std::time::Duration;
use studio_types::TlsMode;

/// Application name reported to servers.
pub const APPLICATION_NAME: &str = "AI DB Studio";

/// Transport security settings.
#[derive(Clone, Debug, Default)]
pub struct TlsConfig {
    /// Mode. Defaults to full verification.
    pub mode: TlsMode,
    /// Extra CA bundle file.
    pub ca_path: Option<String>,
    /// Client certificate file.
    pub client_cert_path: Option<String>,
    /// Client key file.
    pub client_key_path: Option<String>,
    /// Host name to verify, when it differs from `host` (an SSH tunnel's real database host).
    pub server_name: Option<String>,
}

/// Connection settings. `Debug` shows no secret because `SecretString` redacts itself.
#[derive(Clone, Debug)]
pub struct ConnectConfig {
    /// Host to dial.
    pub host: String,
    /// Port to dial.
    pub port: u16,
    /// Database name, or the file path for SQLite.
    pub database: String,
    /// User name.
    pub username: String,
    /// Password.
    pub password: Option<SecretString>,
    /// Transport security.
    pub tls: TlsConfig,
    /// How long to wait for the connection.
    pub connect_timeout: Duration,
    /// Name reported to the server.
    pub application_name: String,
    /// Ask the engine to default to read-only where it can.
    pub read_only: bool,
}

impl ConnectConfig {
    /// A config with the standard timeout and application name.
    pub fn new(host: impl Into<String>, port: u16, database: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            database: database.into(),
            username: String::new(),
            password: None,
            tls: TlsConfig::default(),
            connect_timeout: Duration::from_secs(8),
            application_name: APPLICATION_NAME.to_owned(),
            read_only: false,
        }
    }
}
