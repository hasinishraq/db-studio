//! Connection profiles and session information.

use crate::ids::{ProfileId, SessionId};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A supported database engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum Engine {
    /// PostgreSQL.
    Postgres,
    /// MySQL or MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
}

impl Engine {
    /// The default TCP port, or 0 for file-based engines.
    pub fn default_port(self) -> u16 {
        match self {
            Engine::Postgres => 5432,
            Engine::MySql => 3306,
            Engine::Sqlite => 0,
        }
    }
}

/// The SQL dialect used for parsing. One per engine.
pub type Dialect = Engine;

/// Transport security, using PostgreSQL's naming for every engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TlsMode {
    /// Plain TCP.
    Disable,
    /// Encrypt without verifying the certificate.
    Require,
    /// Verify the chain but not the host name.
    VerifyCa,
    /// Verify chain and host name. The default.
    #[default]
    VerifyFull,
}

/// Where a profile points. Drives the colour band and write guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Environment {
    /// Local or development database.
    #[default]
    Development,
    /// Staging database.
    Staging,
    /// Production database.
    Production,
}

/// What the AI assistant may do on a profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AiPolicy {
    /// Follow the global settings.
    #[default]
    Default,
    /// Only local model endpoints are allowed.
    LocalOnly,
    /// Result rows are never shared with the model.
    NoRowSharing,
    /// The assistant is off for this profile.
    Disabled,
}

/// How to authenticate to an SSH bastion. Holds no secret material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SshAuth {
    /// Password kept in the OS keychain.
    Password,
    /// Private key file; any passphrase is kept in the OS keychain.
    PrivateKey {
        /// Path of the key file.
        path: String,
    },
    /// Keys offered by the running SSH agent.
    Agent,
}

/// An SSH bastion. `has_secret` is set by the store; the secret itself never appears here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    /// Bastion host.
    pub host: String,
    /// Bastion port.
    pub port: u16,
    /// Bastion user.
    pub username: String,
    /// Authentication method.
    pub auth: SshAuth,
    /// Whether a password or passphrase is stored.
    #[serde(default)]
    pub has_secret: bool,
}

/// A saved connection. There is deliberately no password field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    /// Stable id.
    pub id: ProfileId,
    /// Display name.
    pub name: String,
    /// Engine.
    pub engine: Engine,
    /// Host name; empty for SQLite.
    pub host: String,
    /// Port; 0 for SQLite.
    pub port: u16,
    /// Database name, or the file path for SQLite.
    pub database: String,
    /// User name.
    pub username: String,
    /// Transport security.
    pub tls_mode: TlsMode,
    /// Optional CA bundle file.
    pub ca_cert_path: Option<String>,
    /// Optional client certificate file.
    pub client_cert_path: Option<String>,
    /// Optional client key file.
    pub client_key_path: Option<String>,
    /// Optional SSH bastion.
    pub ssh: Option<SshConfig>,
    /// Environment.
    pub environment: Environment,
    /// Refuse every non-read statement, even with an approval.
    pub read_only: bool,
    /// AI policy.
    pub ai_policy: AiPolicy,
    /// Optional accent colour.
    pub color: Option<String>,
    /// PostgreSQL: the schema to use on connect, in `database`. Ignored if it no longer exists.
    #[serde(default)]
    pub default_schema: Option<String>,
    /// Whether a password is stored. Set by the store.
    #[serde(default)]
    pub has_password: bool,
}

/// One certificate presented by a server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CertificateInfo {
    /// Subject distinguished name.
    pub subject: String,
    /// Issuer distinguished name.
    pub issuer: String,
    /// Start of validity.
    pub not_before: String,
    /// End of validity.
    pub not_after: String,
    /// SHA-256 fingerprint, hex.
    pub fingerprint_sha256: String,
}

/// Facts about a connected server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// Version string reported by the server.
    pub version: String,
    /// Whether the connection is encrypted.
    pub tls: bool,
    /// Whether the server certificate was verified.
    pub tls_verified: bool,
    /// Certificates presented, leaf first, when TLS is on.
    #[serde(default)]
    pub certificates: Vec<CertificateInfo>,
}

/// An open session as the UI sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// Session id.
    pub session_id: SessionId,
    /// The profile it was opened from.
    pub profile_id: ProfileId,
    /// Engine.
    pub engine: Engine,
    /// Server facts.
    pub server: ServerInfo,
    /// The database unqualified names resolve in, if one is chosen.
    pub database: Option<String>,
    /// PostgreSQL: the schema unqualified names resolve in first.
    pub schema: Option<String>,
}

/// Which stored secret an IPC call refers to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SecretTarget {
    /// A profile's database password.
    ProfilePassword {
        /// The profile.
        profile_id: ProfileId,
    },
    /// A profile's SSH password or key passphrase.
    SshSecret {
        /// The profile.
        profile_id: ProfileId,
    },
    /// An AI provider's API key.
    AiKey {
        /// Provider name.
        provider: String,
    },
}
