//! Saved queries.

use crate::db::{parse_id, Store, StoreError};
use sqlx::Row;
use studio_types::{Draft, DraftId, ProfileId};

impl Store {
    /// Lists drafts, newest first.
    pub async fn list_drafts(&self) -> Result<Vec<Draft>, StoreError> {
        let rows = sqlx::query("SELECT * FROM drafts ORDER BY updated_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                let profile: Option<String> = row.try_get("profile_id")?;
                Ok(Draft {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    profile_id: profile.as_deref().map(parse_id::<ProfileId>).transpose()?,
                    title: row.try_get("title")?,
                    sql: row.try_get("sql")?,
                    saved_sql: row.try_get("saved_sql")?,
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect()
    }

    /// Inserts or replaces a draft.
    pub async fn save_draft(&self, d: &Draft) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO drafts (id, profile_id, title, sql, saved_sql, updated_at) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET profile_id=?2, title=?3, sql=?4, saved_sql=?5, updated_at=?6",
        )
        .bind(d.id.to_string())
        .bind(d.profile_id.map(|p| p.to_string()))
        .bind(d.title.clone())
        .bind(d.sql.clone())
        .bind(d.saved_sql.clone())
        .bind(d.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes a draft.
    pub async fn delete_draft(&self, id: DraftId) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM drafts WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
