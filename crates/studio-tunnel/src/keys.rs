//! Client authentication.

use crate::client::{TunnelAuth, TunnelError};
use crate::known_hosts::KeyChecker;
use russh::client::Handle;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use secrecy::ExposeSecret;
use std::sync::Arc;

fn denied() -> TunnelError {
    TunnelError::Auth("the SSH server rejected these credentials".to_owned())
}

fn failed(error: russh::Error) -> TunnelError {
    TunnelError::Connect(error.to_string())
}

async fn with_key(
    handle: &mut Handle<KeyChecker>,
    user: &str,
    path: &str,
    passphrase: Option<&str>,
) -> Result<bool, TunnelError> {
    let key = load_secret_key(path, passphrase)
        .map_err(|_| TunnelError::Auth("the key file could not be read or decrypted".to_owned()))?;
    let hash = handle.best_supported_rsa_hash().await.map_err(failed)?.flatten();
    let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
    Ok(handle
        .authenticate_publickey(user, key)
        .await
        .map_err(failed)?
        .success())
}

#[cfg(unix)]
async fn agent_client() -> Result<russh::keys::agent::client::AgentClient<tokio::net::UnixStream>, TunnelError>
{
    russh::keys::agent::client::AgentClient::connect_env()
        .await
        .map_err(|_| TunnelError::Auth("no SSH agent is running".to_owned()))
}

#[cfg(windows)]
async fn agent_client() -> Result<
    russh::keys::agent::client::AgentClient<tokio::net::windows::named_pipe::NamedPipeClient>,
    TunnelError,
> {
    russh::keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map_err(|_| TunnelError::Auth("no SSH agent is running".to_owned()))
}

async fn with_agent(handle: &mut Handle<KeyChecker>, user: &str) -> Result<bool, TunnelError> {
    let mut agent = agent_client().await?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|_| TunnelError::Auth("the SSH agent did not list its keys".to_owned()))?;
    let hash = handle.best_supported_rsa_hash().await.map_err(failed)?.flatten();
    for identity in identities {
        let key = identity.public_key().into_owned();
        if let Ok(result) = handle
            .authenticate_publickey_with(user, key, hash, &mut agent)
            .await
        {
            if result.success() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Authenticates, or explains why it could not.
pub(crate) async fn authenticate(
    handle: &mut Handle<KeyChecker>,
    user: &str,
    auth: &TunnelAuth,
) -> Result<(), TunnelError> {
    let accepted = match auth {
        TunnelAuth::Password(password) => handle
            .authenticate_password(user, password.expose_secret())
            .await
            .map_err(failed)?
            .success(),
        TunnelAuth::Key { path, passphrase } => {
            let passphrase = passphrase.as_ref().map(|p| p.expose_secret());
            with_key(handle, user, path, passphrase).await?
        }
        TunnelAuth::Agent => with_agent(handle, user).await?,
    };
    if accepted {
        Ok(())
    } else {
        Err(denied())
    }
}
