//! Host key pinning.

use crate::client::TunnelError;
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate};
use std::sync::{Arc, Mutex};

/// A trusted key for a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedKey {
    /// Algorithm.
    pub key_type: String,
    /// `SHA256:...` fingerprint.
    pub fingerprint: String,
}

/// Where pins are kept. Implemented over the app database.
#[async_trait::async_trait]
pub trait HostKeyStore: Send + Sync {
    /// The keys pinned for a host.
    async fn pinned(&self, host: &str, port: u16) -> Result<Vec<PinnedKey>, TunnelError>;
}

/// The SSH client's handler: refuses any key that is not pinned.
pub(crate) struct KeyChecker {
    pub host: String,
    pub port: u16,
    pub pinned: Vec<PinnedKey>,
    /// Why the key was refused, since the library reports only a generic failure.
    pub refusal: Arc<Mutex<Option<TunnelError>>>,
}

impl KeyChecker {
    fn verdict(&self, key: &PublicKey) -> Result<(), TunnelError> {
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let key_type = key.algorithm().to_string();
        if self.pinned.iter().any(|p| p.fingerprint == fingerprint) {
            return Ok(());
        }
        match self.pinned.first() {
            Some(expected) => Err(TunnelError::HostKeyChanged {
                host: self.host.clone(),
                port: self.port,
                expected: expected.fingerprint.clone(),
                actual: fingerprint,
            }),
            None => Err(TunnelError::HostKeyUnknown {
                host: self.host.clone(),
                port: self.port,
                key_type,
                fingerprint_sha256: fingerprint,
            }),
        }
    }
}

impl russh::client::Handler for KeyChecker {
    type Error = russh::Error;

    async fn check_server_key(&mut self, presented: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let verdict = match presented {
            PublicKeyOrCertificate::PublicKey { key, .. } => self.verdict(key),
            // Pins cover plain keys. A certificate is refused rather than trusted on another basis.
            PublicKeyOrCertificate::Certificate(_) => Err(TunnelError::Connect(
                "the server presented a host certificate, which is not supported".to_owned(),
            )),
        };
        match verdict {
            Ok(()) => Ok(true),
            Err(error) => {
                if let Ok(mut slot) = self.refusal.lock() {
                    *slot = Some(error);
                }
                Ok(false)
            }
        }
    }
}
