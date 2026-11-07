//! SSH: tunnels for sessions, and host key trust.

use super::check_text;
use crate::error::AppResult;
use crate::state::{AppState, ConnectionLost};
use secrecy::SecretString;
use std::sync::Arc;
use studio_session::{Connector, Route, SessionError, SessionManager};
use studio_store::{KnownHost, Store};
use studio_tunnel::{HostKeyStore, PinnedKey, Tunnel, TunnelAuth, TunnelConfig, TunnelError};
use studio_types::{AppError, ConnectionProfile, HostKeyDetail, ProfileId, SessionId, SshAuth};
use tauri::{AppHandle, State};
use tauri_specta::Event;

/// Host key pins kept in the app database.
pub struct StoreHostKeys {
    /// The database.
    pub store: Store,
}

#[async_trait::async_trait]
impl HostKeyStore for StoreHostKeys {
    async fn pinned(&self, host: &str, port: u16) -> Result<Vec<PinnedKey>, TunnelError> {
        let pins = self
            .store
            .known_host(host, port)
            .await
            .map_err(|e| TunnelError::Store(e.to_string()))?;
        Ok(pins
            .into_iter()
            .map(|pin| PinnedKey {
                key_type: pin.key_type,
                fingerprint: pin.fingerprint,
            })
            .collect())
    }
}

/// Maps a tunnel failure, keeping what the UI needs to show the fingerprints.
pub fn tunnel_error(error: TunnelError) -> SessionError {
    let message = error.to_string();
    match error {
        TunnelError::HostKeyUnknown {
            host,
            port,
            key_type,
            fingerprint_sha256,
        } => SessionError::HostKey {
            message,
            changed: false,
            detail: Box::new(HostKeyDetail {
                host,
                port,
                key_type: Some(key_type),
                fingerprint: fingerprint_sha256,
                expected: None,
            }),
        },
        TunnelError::HostKeyChanged {
            host,
            port,
            expected,
            actual,
        } => SessionError::HostKey {
            message,
            changed: true,
            detail: Box::new(HostKeyDetail {
                host,
                port,
                key_type: None,
                fingerprint: actual,
                expected: Some(expected),
            }),
        },
        _ => SessionError::Tunnel(message),
    }
}

/// Opens SSH tunnels for the session manager.
pub struct TunnelConnector {
    /// Where pins live.
    pub store: Store,
    /// For the connection-lost event; absent in tests.
    pub app: Option<AppHandle>,
    /// Filled after construction, because the manager owns this connector.
    pub sessions: std::sync::OnceLock<std::sync::Weak<SessionManager>>,
}

fn auth_for(ssh: &SshAuth, secret: Option<SecretString>) -> Result<TunnelAuth, SessionError> {
    Ok(match ssh {
        SshAuth::Password => TunnelAuth::Password(
            secret.ok_or_else(|| SessionError::Tunnel("no SSH password is saved".to_owned()))?,
        ),
        SshAuth::PrivateKey { path } => TunnelAuth::Key {
            path: path.clone(),
            passphrase: secret,
        },
        SshAuth::Agent => TunnelAuth::Agent,
    })
}

#[async_trait::async_trait]
impl Connector for TunnelConnector {
    async fn open_route(
        &self,
        profile: &ConnectionProfile,
        ssh_secret: Option<SecretString>,
        session: Option<SessionId>,
    ) -> Result<Route, SessionError> {
        let ssh = profile
            .ssh
            .as_ref()
            .ok_or_else(|| SessionError::Tunnel("no SSH settings".to_owned()))?;
        let config = TunnelConfig {
            host: ssh.host.clone(),
            port: ssh.port,
            username: ssh.username.clone(),
            auth: auth_for(&ssh.auth, ssh_secret)?,
            target_host: profile.host.clone(),
            target_port: profile.port,
        };
        let keys = StoreHostKeys {
            store: self.store.clone(),
        };
        let tunnel = Tunnel::open(config, &keys).await.map_err(tunnel_error)?;
        if let (Some(app), Some(id)) = (self.app.clone(), session) {
            let lost = tunnel.lost();
            let manager = self.sessions.get().cloned();
            tokio::spawn(async move {
                lost.cancelled().await;
                if let Some(manager) = manager.and_then(|weak| weak.upgrade()) {
                    manager.forget(id);
                }
                let _ = ConnectionLost { session_id: id }.emit(&app);
            });
        }
        Ok(Route {
            local_port: tunnel.local_port,
            keep_alive: Box::new(Arc::new(tunnel)),
        })
    }
}

/// Pins a new host key, after the user compared the fingerprint.
pub async fn trust(
    store: &Store,
    id: ProfileId,
    key_type: String,
    fingerprint: String,
    replace: bool,
) -> Result<(), AppError> {
    check_text("the fingerprint", &fingerprint, 200)?;
    check_text("the key type", &key_type, 100)?;
    let profile = store.get_profile(id).await.app()?;
    let ssh = profile
        .ssh
        .ok_or_else(|| AppError::invalid("this profile has no SSH settings"))?;
    let key = KnownHost {
        key_type,
        fingerprint,
    };
    if replace {
        store.replace_host_key(&ssh.host, ssh.port, &key).await.app()
    } else {
        store.trust_host_key(&ssh.host, ssh.port, &key).await.app()
    }
}

/// Trusts a host key the user compared. `replace` is the explicit action for a changed key.
#[tauri::command]
#[specta::specta]
pub async fn trust_host_key(
    state: State<'_, AppState>,
    profile_id: ProfileId,
    key_type: String,
    fingerprint: String,
    replace: bool,
) -> Result<(), AppError> {
    trust(&state.store, profile_id, key_type, fingerprint, replace).await
}

#[cfg(test)]
#[path = "ssh_tests.rs"]
mod real_tunnel;

#[cfg(test)]
mod ssh_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::error::ToApp;
    use studio_types::ErrorCode;

    #[test]
    fn an_unknown_key_reaches_the_ui_with_its_fingerprint() {
        let error = tunnel_error(TunnelError::HostKeyUnknown {
            host: "b".into(),
            port: 22,
            key_type: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:abc".into(),
        })
        .to_app();
        assert_eq!(error.code, ErrorCode::HostKeyUnknown);
        let detail = error.host_key.unwrap();
        assert_eq!(detail.fingerprint, "SHA256:abc");
        assert_eq!(detail.key_type.as_deref(), Some("ssh-ed25519"));
    }

    #[test]
    fn a_changed_key_shows_both_fingerprints() {
        let error = tunnel_error(TunnelError::HostKeyChanged {
            host: "b".into(),
            port: 22,
            expected: "SHA256:old".into(),
            actual: "SHA256:new".into(),
        })
        .to_app();
        assert_eq!(error.code, ErrorCode::HostKeyChanged);
        let detail = error.host_key.unwrap();
        assert_eq!(detail.fingerprint, "SHA256:new");
        assert_eq!(detail.expected.as_deref(), Some("SHA256:old"));
    }

    #[test]
    fn password_auth_needs_a_saved_password() {
        assert!(auth_for(&SshAuth::Password, None).is_err());
        assert!(auth_for(&SshAuth::Agent, None).is_ok());
    }
}
