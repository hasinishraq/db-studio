//! App storage: one local SQLite file, OS keychain secrets and a tamper-evident audit log.

pub mod audit;
pub mod conversations;
pub mod db;
pub mod drafts;
pub mod history;
pub mod known_hosts;
pub mod masking;
pub mod profiles;
pub mod secrets;
pub mod settings;

pub use db::{Store, StoreError};
pub use known_hosts::KnownHost;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKey, SecretStore};
