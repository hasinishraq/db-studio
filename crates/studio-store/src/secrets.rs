//! Secrets. They live in the OS credential store and leave this crate only as `SecretString`.

use crate::db::StoreError;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Mutex;
use studio_types::ProfileId;
use tokio::sync::oneshot;

const SERVICE: &str = "com.aidbstudio.app";

/// Names one stored secret.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SecretKey {
    /// A profile's database password.
    ProfilePassword(ProfileId),
    /// A profile's SSH password or key passphrase.
    SshSecret(ProfileId),
    /// An AI provider's API key.
    AiKey(String),
    /// The app database encryption key.
    StoreKey,
}

impl SecretKey {
    /// The credential entry name.
    pub fn entry_name(&self) -> String {
        match self {
            SecretKey::ProfilePassword(id) => format!("profile:{id}:password"),
            SecretKey::SshSecret(id) => format!("profile:{id}:ssh"),
            SecretKey::AiKey(provider) => format!("ai:{provider}"),
            SecretKey::StoreKey => "store:key".to_owned(),
        }
    }
}

/// Where secrets are kept.
#[async_trait::async_trait]
pub trait SecretStore: Send + Sync {
    /// Reads a secret, or `None` when absent.
    async fn get(&self, key: &SecretKey) -> Result<Option<SecretString>, StoreError>;
    /// Writes a secret.
    async fn set(&self, key: &SecretKey, value: SecretString) -> Result<(), StoreError>;
    /// Removes a secret. Removing an absent secret succeeds.
    async fn delete(&self, key: &SecretKey) -> Result<(), StoreError>;
}

/// Keeps secrets for this process only. Used when no OS store exists, and in tests.
#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<SecretKey, SecretString>>,
}

impl MemorySecretStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn locked(&self) -> Result<std::sync::MutexGuard<'_, HashMap<SecretKey, SecretString>>, StoreError> {
        self.values
            .lock()
            .map_err(|_| StoreError::Secret("memory store poisoned".to_owned()))
    }
}

#[async_trait::async_trait]
impl SecretStore for MemorySecretStore {
    async fn get(&self, key: &SecretKey) -> Result<Option<SecretString>, StoreError> {
        Ok(self.locked()?.get(key).cloned())
    }

    async fn set(&self, key: &SecretKey, value: SecretString) -> Result<(), StoreError> {
        self.locked()?.insert(key.clone(), value);
        Ok(())
    }

    async fn delete(&self, key: &SecretKey) -> Result<(), StoreError> {
        self.locked()?.remove(key);
        Ok(())
    }
}

enum Job {
    Get(String, oneshot::Sender<Result<Option<SecretString>, StoreError>>),
    Set(String, SecretString, oneshot::Sender<Result<(), StoreError>>),
    Delete(String, oneshot::Sender<Result<(), StoreError>>),
}

/// The OS credential store. Every call runs on one worker thread, because some OS stores
/// misbehave under concurrent access.
pub struct KeyringSecretStore {
    jobs: mpsc::Sender<Job>,
}

impl KeyringSecretStore {
    /// Starts the worker thread.
    pub fn new() -> Result<Self, StoreError> {
        let (jobs, inbox) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("keyring".to_owned())
            .spawn(move || run_worker(inbox))?;
        Ok(Self { jobs })
    }

    /// Writes, reads and removes a probe entry to learn whether the OS store really works.
    pub async fn probe(&self) -> Result<(), StoreError> {
        let key = SecretKey::AiKey("__probe__".to_owned());
        self.set(&key, SecretString::from("probe")).await?;
        let found = self.get(&key).await?;
        self.delete(&key).await?;
        match found {
            Some(_) => Ok(()),
            None => Err(StoreError::Secret(
                "the credential store did not keep the entry".to_owned(),
            )),
        }
    }

    fn send(&self, job: Job) -> Result<(), StoreError> {
        self.jobs.send(job).map_err(|_| worker_gone())
    }
}

fn worker_gone() -> StoreError {
    StoreError::Secret("keyring worker stopped".to_owned())
}

fn entry(name: &str) -> Result<keyring::Entry, StoreError> {
    keyring::Entry::new(SERVICE, name).map_err(keyring_error)
}

/// Only the error kind is kept; keyring errors can embed entry details.
fn keyring_error(error: keyring::Error) -> StoreError {
    let kind = match error {
        keyring::Error::NoStorageAccess(_) => "credential store is unavailable",
        keyring::Error::PlatformFailure(_) => "credential store failed",
        keyring::Error::TooLong(..) => "secret is too long",
        keyring::Error::Invalid(..) => "invalid credential entry",
        _ => "credential store error",
    };
    StoreError::Secret(kind.to_owned())
}

fn run_worker(inbox: mpsc::Receiver<Job>) {
    while let Ok(job) = inbox.recv() {
        match job {
            Job::Get(name, reply) => {
                let result = entry(&name).and_then(|e| match e.get_password() {
                    Ok(value) => Ok(Some(SecretString::from(value))),
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(other) => Err(keyring_error(other)),
                });
                let _ = reply.send(result);
            }
            Job::Set(name, value, reply) => {
                let result =
                    entry(&name).and_then(|e| e.set_password(value.expose_secret()).map_err(keyring_error));
                let _ = reply.send(result);
            }
            Job::Delete(name, reply) => {
                let result = entry(&name).and_then(|e| match e.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(other) => Err(keyring_error(other)),
                });
                let _ = reply.send(result);
            }
        }
    }
}

#[async_trait::async_trait]
impl SecretStore for KeyringSecretStore {
    async fn get(&self, key: &SecretKey) -> Result<Option<SecretString>, StoreError> {
        let (reply, answer) = oneshot::channel();
        self.send(Job::Get(key.entry_name(), reply))?;
        answer.await.map_err(|_| worker_gone())?
    }

    async fn set(&self, key: &SecretKey, value: SecretString) -> Result<(), StoreError> {
        let (reply, answer) = oneshot::channel();
        self.send(Job::Set(key.entry_name(), value, reply))?;
        answer.await.map_err(|_| worker_gone())?
    }

    async fn delete(&self, key: &SecretKey) -> Result<(), StoreError> {
        let (reply, answer) = oneshot::channel();
        self.send(Job::Delete(key.entry_name(), reply))?;
        answer.await.map_err(|_| worker_gone())?
    }
}
