//! Connection profiles.

use crate::db::{enum_text, now_ms, parse_enum, parse_id, parse_json, to_json, Store, StoreError};
use crate::secrets::SecretKey;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;
use studio_types::{ConnectionProfile, ProfileId, SshConfig};

impl Store {
    /// Lists profiles by name, with `has_password` and `has_secret` filled from the secret store.
    pub async fn list_profiles(&self) -> Result<Vec<ConnectionProfile>, StoreError> {
        let rows = sqlx::query("SELECT * FROM profiles ORDER BY name COLLATE NOCASE")
            .fetch_all(&self.pool)
            .await?;
        let mut profiles = Vec::with_capacity(rows.len());
        for row in &rows {
            profiles.push(self.with_secret_flags(profile_from_row(row)?).await?);
        }
        Ok(profiles)
    }

    /// Reads one profile.
    pub async fn get_profile(&self, id: ProfileId) -> Result<ConnectionProfile, StoreError> {
        let row = sqlx::query("SELECT * FROM profiles WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| StoreError::NotFound("profile".to_owned()))?;
        self.with_secret_flags(profile_from_row(&row)?).await
    }

    /// Inserts or replaces a profile. The `has_*` flags are ignored; the secret store decides them.
    pub async fn upsert_profile(&self, p: &ConnectionProfile) -> Result<(), StoreError> {
        let ssh = p.ssh.as_ref().map(to_json).transpose()?;
        let now = now_ms();
        sqlx::query(
            "INSERT INTO profiles (id, name, engine, host, port, database, username, tls_mode, ca_cert_path,
               client_cert_path, client_key_path, ssh_json, environment, read_only, ai_policy, color,
               created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17)
             ON CONFLICT(id) DO UPDATE SET name=?2, engine=?3, host=?4, port=?5, database=?6, username=?7,
               tls_mode=?8, ca_cert_path=?9, client_cert_path=?10, client_key_path=?11, ssh_json=?12,
               environment=?13, read_only=?14, ai_policy=?15, color=?16, updated_at=?17",
        )
        .bind(p.id.to_string())
        .bind(p.name.clone())
        .bind(enum_text(&p.engine)?)
        .bind(p.host.clone())
        .bind(i64::from(p.port))
        .bind(p.database.clone())
        .bind(p.username.clone())
        .bind(enum_text(&p.tls_mode)?)
        .bind(p.ca_cert_path.clone())
        .bind(p.client_cert_path.clone())
        .bind(p.client_key_path.clone())
        .bind(ssh)
        .bind(enum_text(&p.environment)?)
        .bind(p.read_only)
        .bind(enum_text(&p.ai_policy)?)
        .bind(p.color.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes a profile, its secrets and its masking rules.
    pub async fn delete_profile(&self, id: ProfileId) -> Result<(), StoreError> {
        self.secrets.delete(&SecretKey::ProfilePassword(id)).await?;
        self.secrets.delete(&SecretKey::SshSecret(id)).await?;
        sqlx::query("DELETE FROM masking_rules WHERE profile_id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM profiles WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn with_secret_flags(&self, mut p: ConnectionProfile) -> Result<ConnectionProfile, StoreError> {
        p.has_password = self
            .secrets
            .get(&SecretKey::ProfilePassword(p.id))
            .await?
            .is_some();
        let has_ssh = match p.ssh {
            Some(_) => self.secrets.get(&SecretKey::SshSecret(p.id)).await?.is_some(),
            None => false,
        };
        if let Some(ssh) = p.ssh.as_mut() {
            ssh.has_secret = has_ssh;
        }
        Ok(p)
    }
}

fn profile_from_row(row: &SqliteRow) -> Result<ConnectionProfile, StoreError> {
    let ssh: Option<String> = row.try_get("ssh_json")?;
    let port: i64 = row.try_get("port")?;
    Ok(ConnectionProfile {
        id: parse_id(&row.try_get::<String, _>("id")?)?,
        name: row.try_get("name")?,
        engine: parse_enum(&row.try_get::<String, _>("engine")?)?,
        host: row.try_get("host")?,
        port: u16::try_from(port).map_err(|_| StoreError::Corrupt("port out of range".to_owned()))?,
        database: row.try_get("database")?,
        username: row.try_get("username")?,
        tls_mode: parse_enum(&row.try_get::<String, _>("tls_mode")?)?,
        ca_cert_path: row.try_get("ca_cert_path")?,
        client_cert_path: row.try_get("client_cert_path")?,
        client_key_path: row.try_get("client_key_path")?,
        ssh: ssh.as_deref().map(parse_json::<SshConfig>).transpose()?,
        environment: parse_enum(&row.try_get::<String, _>("environment")?)?,
        read_only: row.try_get("read_only")?,
        ai_policy: parse_enum(&row.try_get::<String, _>("ai_policy")?)?,
        color: row.try_get("color")?,
        has_password: false,
    })
}
