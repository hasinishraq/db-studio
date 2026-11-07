//! Opening a tunnel.

use crate::forward::serve;
use crate::keys::authenticate;
use crate::known_hosts::{HostKeyStore, KeyChecker};
use secrecy::SecretString;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const KEEP_ALIVE: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What can go wrong. Messages never contain a password or passphrase.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TunnelError {
    /// The bastion's key is not pinned. Nothing was sent to it.
    #[error("the host key of {host}:{port} is not trusted yet ({fingerprint_sha256})")]
    HostKeyUnknown {
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// Algorithm.
        key_type: String,
        /// Presented fingerprint.
        fingerprint_sha256: String,
    },
    /// The bastion's key differs from the pinned one. A hard error.
    #[error("the host key of {host}:{port} changed: expected {expected}, got {actual}")]
    HostKeyChanged {
        /// Host.
        host: String,
        /// Port.
        port: u16,
        /// Pinned fingerprint.
        expected: String,
        /// Presented fingerprint.
        actual: String,
    },
    /// Authentication failed.
    #[error("{0}")]
    Auth(String),
    /// The connection failed.
    #[error("SSH connection failed: {0}")]
    Connect(String),
    /// The pin store failed.
    #[error("{0}")]
    Store(String),
}

/// How to authenticate.
#[derive(Clone)]
pub enum TunnelAuth {
    /// A password.
    Password(SecretString),
    /// A private key file.
    Key {
        /// Path.
        path: String,
        /// Passphrase, if the key is encrypted.
        passphrase: Option<SecretString>,
    },
    /// The running SSH agent.
    Agent,
}

/// Where to connect and forward.
#[derive(Clone)]
pub struct TunnelConfig {
    /// Bastion host.
    pub host: String,
    /// Bastion port.
    pub port: u16,
    /// Bastion user.
    pub username: String,
    /// Credentials.
    pub auth: TunnelAuth,
    /// Database host, as the bastion sees it.
    pub target_host: String,
    /// Database port.
    pub target_port: u16,
}

/// An open tunnel. Dropping it closes the listener and the SSH session.
pub struct Tunnel {
    /// The local port to dial.
    pub local_port: u16,
    shutdown: CancellationToken,
    lost: CancellationToken,
    task: JoinHandle<()>,
}

impl Tunnel {
    /// Opens a tunnel. An unpinned or changed host key fails before any credential is sent.
    pub async fn open(cfg: TunnelConfig, host_keys: &dyn HostKeyStore) -> Result<Self, TunnelError> {
        let pinned = host_keys.pinned(&cfg.host, cfg.port).await?;
        let refusal = Arc::new(Mutex::new(None));
        let checker = KeyChecker {
            host: cfg.host.clone(),
            port: cfg.port,
            pinned,
            refusal: refusal.clone(),
        };
        let config = Arc::new(russh::client::Config {
            keepalive_interval: Some(KEEP_ALIVE),
            ..Default::default()
        });
        let connecting = russh::client::connect(config, (cfg.host.as_str(), cfg.port), checker);
        let connected = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| TunnelError::Connect("timed out".to_owned()))?;
        let mut handle = match connected {
            Ok(handle) => handle,
            Err(error) => {
                let refused = refusal.lock().ok().and_then(|mut slot| slot.take());
                return Err(refused.unwrap_or_else(|| TunnelError::Connect(error.to_string())));
            }
        };
        authenticate(&mut handle, &cfg.username, &cfg.auth).await?;

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| TunnelError::Connect(e.to_string()))?;
        let local_port = listener
            .local_addr()
            .map_err(|e| TunnelError::Connect(e.to_string()))?
            .port();
        let (shutdown, lost) = (CancellationToken::new(), CancellationToken::new());
        let task = tokio::spawn(serve(
            listener,
            Arc::new(handle),
            cfg.target_host,
            cfg.target_port,
            shutdown.clone(),
            lost.clone(),
        ));
        Ok(Self {
            local_port,
            shutdown,
            lost,
            task,
        })
    }

    /// Fires when the SSH session drops by itself.
    pub fn lost(&self) -> CancellationToken {
        self.lost.clone()
    }

    /// Closes the tunnel and waits until the port is free.
    pub async fn close(mut self) {
        self.shutdown.cancel();
        let task = std::mem::replace(&mut self.task, tokio::spawn(async {}));
        let _ = task.await;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}
