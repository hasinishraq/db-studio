//! Per-profile masking rules.

use crate::db::{Store, StoreError};
use sqlx::Row;
use studio_types::{MaskingRule, ProfileId};

impl Store {
    /// Lists a profile's rules.
    pub async fn list_masking_rules(&self, profile: ProfileId) -> Result<Vec<MaskingRule>, StoreError> {
        let rows = sqlx::query(
            "SELECT schema_name, table_name, column_name FROM masking_rules WHERE profile_id = ?1
             ORDER BY schema_name, table_name, column_name",
        )
        .bind(profile.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(MaskingRule {
                    schema_name: r.try_get("schema_name")?,
                    table_name: r.try_get("table_name")?,
                    column_name: r.try_get("column_name")?,
                })
            })
            .collect()
    }

    /// Replaces a profile's rules.
    pub async fn set_masking_rules(
        &self,
        profile: ProfileId,
        rules: &[MaskingRule],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM masking_rules WHERE profile_id = ?1")
            .bind(profile.to_string())
            .execute(&mut *tx)
            .await?;
        for rule in rules {
            sqlx::query(
                "INSERT OR IGNORE INTO masking_rules (profile_id, schema_name, table_name, column_name)
                 VALUES (?1,?2,?3,?4)",
            )
            .bind(profile.to_string())
            .bind(rule.schema_name.clone())
            .bind(rule.table_name.clone())
            .bind(rule.column_name.clone())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
