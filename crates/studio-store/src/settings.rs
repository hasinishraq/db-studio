//! Typed preferences.

use crate::db::{parse_json, to_json, Store, StoreError};
use sqlx::Row;
use studio_types::Settings;

const KEY: &str = "settings";

impl Store {
    /// Reads the settings, or the defaults when none are stored.
    pub async fn get_settings(&self) -> Result<Settings, StoreError> {
        let row = sqlx::query("SELECT value_json FROM settings WHERE key = ?1")
            .bind(KEY)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => parse_json(&row.try_get::<String, _>("value_json")?),
            None => Ok(Settings::default()),
        }
    }

    /// Writes the settings.
    pub async fn set_settings(&self, settings: &Settings) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = ?2",
        )
        .bind(KEY)
        .bind(to_json(settings)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
