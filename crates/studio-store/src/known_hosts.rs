//! Pinned SSH host keys.

use crate::db::{Store, StoreError};
use sqlx::Row;

/// A pinned key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownHost {
    /// Key algorithm.
    pub key_type: String,
    /// SHA-256 fingerprint.
    pub fingerprint: String,
}

impl Store {
    /// Reads the keys pinned for a host.
    pub async fn known_host(&self, host: &str, port: u16) -> Result<Vec<KnownHost>, StoreError> {
        let rows = sqlx::query("SELECT key_type, fingerprint FROM known_hosts WHERE host = ?1 AND port = ?2")
            .bind(host.to_owned())
            .bind(i64::from(port))
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(KnownHost {
                    key_type: r.try_get("key_type")?,
                    fingerprint: r.try_get("fingerprint")?,
                })
            })
            .collect()
    }

    /// Pins a new key. Never changes an existing pin; use [`Store::replace_host_key`] for that.
    pub async fn trust_host_key(&self, host: &str, port: u16, key: &KnownHost) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT OR IGNORE INTO known_hosts (host, port, key_type, fingerprint) VALUES (?1,?2,?3,?4)",
        )
        .bind(host.to_owned())
        .bind(i64::from(port))
        .bind(key.key_type.clone())
        .bind(key.fingerprint.clone())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Replaces every pin for a host. Only an explicit user action may call this.
    pub async fn replace_host_key(&self, host: &str, port: u16, key: &KnownHost) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM known_hosts WHERE host = ?1 AND port = ?2")
            .bind(host.to_owned())
            .bind(i64::from(port))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO known_hosts (host, port, key_type, fingerprint) VALUES (?1,?2,?3,?4)")
            .bind(host.to_owned())
            .bind(i64::from(port))
            .bind(key.key_type.clone())
            .bind(key.fingerprint.clone())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
