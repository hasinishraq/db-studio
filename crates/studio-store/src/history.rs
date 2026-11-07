//! Run history.

use crate::db::{parse_id, Store, StoreError};
use sqlx::Row;
use studio_types::{HistoryEntry, ProfileId};

impl Store {
    /// Adds an entry, then trims to `limit` newest entries.
    pub async fn add_history(&self, e: &HistoryEntry, limit: u32) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT OR REPLACE INTO history (id, profile_id, sql, started_at, duration_ms, row_count, status)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(e.id.to_string())
        .bind(e.profile_id.map(|p| p.to_string()))
        .bind(e.sql.clone())
        .bind(e.started_at)
        .bind(e.duration_ms)
        .bind(e.row_count)
        .bind(e.status.clone())
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM history WHERE id NOT IN
               (SELECT id FROM history ORDER BY started_at DESC, rowid DESC LIMIT ?1)",
        )
        .bind(i64::from(limit))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists at most `limit` entries, newest first.
    pub async fn list_history(&self, limit: u32) -> Result<Vec<HistoryEntry>, StoreError> {
        let rows = sqlx::query("SELECT * FROM history ORDER BY started_at DESC, rowid DESC LIMIT ?1")
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                let profile: Option<String> = row.try_get("profile_id")?;
                Ok(HistoryEntry {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    profile_id: profile.as_deref().map(parse_id::<ProfileId>).transpose()?,
                    sql: row.try_get("sql")?,
                    started_at: row.try_get("started_at")?,
                    duration_ms: row.try_get("duration_ms")?,
                    row_count: row.try_get("row_count")?,
                    status: row.try_get("status")?,
                })
            })
            .collect()
    }

    /// Removes all history.
    pub async fn clear_history(&self) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM history").execute(&self.pool).await?;
        Ok(())
    }
}
