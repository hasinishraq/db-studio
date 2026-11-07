//! The hash-chained audit log.

use crate::db::{enum_text, parse_enum, parse_id, to_json, Store, StoreError};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;
use studio_types::{AuditEntry, AuditQuery, AuditRecord, ProfileId};

/// `prev_hash` of the first record.
const GENESIS: &str = "";

/// `sha256(prev_hash || canonical_json(entry))`, hex. Struct field order makes the JSON canonical.
fn entry_hash(prev_hash: &str, entry: &AuditEntry) -> Result<String, StoreError> {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(to_json(entry)?.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

impl Store {
    /// Appends a record, linking it to the previous one.
    pub async fn audit_append(&self, entry: &AuditEntry) -> Result<i64, StoreError> {
        let _chain = self.audit_lock.lock().await;
        let last = sqlx::query("SELECT entry_hash FROM audit_log ORDER BY id DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        let prev_hash = match last {
            Some(row) => row.try_get::<String, _>("entry_hash")?,
            None => GENESIS.to_owned(),
        };
        let hash = entry_hash(&prev_hash, entry)?;
        let result = sqlx::query(
            "INSERT INTO audit_log (at, profile_id, environment, origin, kind, sql_hash, sql_text,
               rows_affected, outcome, prev_hash, entry_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )
        .bind(entry.at)
        .bind(entry.profile_id.map(|p| p.to_string()))
        .bind(enum_text(&entry.environment)?)
        .bind(enum_text(&entry.origin)?)
        .bind(enum_text(&entry.kind)?)
        .bind(entry.sql_hash.clone())
        .bind(entry.sql_text.clone())
        .bind(entry.rows_affected)
        .bind(entry.outcome.clone())
        .bind(prev_hash)
        .bind(hash)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Lists records matching the filters, newest first.
    pub async fn audit_list(&self, q: &AuditQuery) -> Result<Vec<AuditRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM audit_log
             WHERE (?1 IS NULL OR profile_id = ?1) AND (?2 IS NULL OR origin = ?2) AND (?3 IS NULL OR kind = ?3)
               AND (?4 IS NULL OR at >= ?4) AND (?5 IS NULL OR at <= ?5)
             ORDER BY id DESC LIMIT ?6",
        )
        .bind(q.profile_id.map(|p| p.to_string()))
        .bind(q.origin.as_ref().map(enum_text).transpose()?)
        .bind(q.kind.as_ref().map(enum_text).transpose()?)
        .bind(q.from)
        .bind(q.to)
        .bind(i64::from(q.limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(record_from_row).collect()
    }

    /// Recomputes the chain and returns the id of the first broken record, if any.
    pub async fn verify_chain(&self) -> Result<Option<i64>, StoreError> {
        let rows = sqlx::query("SELECT * FROM audit_log ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        let mut prev = GENESIS.to_owned();
        for row in &rows {
            let id: i64 = row.try_get("id")?;
            let Ok(record) = record_from_row(row) else {
                return Ok(Some(id));
            };
            if record.prev_hash != prev || entry_hash(&prev, &record.entry)? != record.entry_hash {
                return Ok(Some(id));
            }
            prev = record.entry_hash;
        }
        Ok(None)
    }
}

fn record_from_row(row: &SqliteRow) -> Result<AuditRecord, StoreError> {
    let profile: Option<String> = row.try_get("profile_id")?;
    Ok(AuditRecord {
        id: row.try_get("id")?,
        entry: AuditEntry {
            at: row.try_get("at")?,
            profile_id: profile.as_deref().map(parse_id::<ProfileId>).transpose()?,
            environment: parse_enum(&row.try_get::<String, _>("environment")?)?,
            origin: parse_enum(&row.try_get::<String, _>("origin")?)?,
            kind: parse_enum(&row.try_get::<String, _>("kind")?)?,
            sql_hash: row.try_get("sql_hash")?,
            sql_text: row.try_get("sql_text")?,
            rows_affected: row.try_get("rows_affected")?,
            outcome: row.try_get("outcome")?,
        },
        prev_hash: row.try_get("prev_hash")?,
        entry_hash: row.try_get("entry_hash")?,
    })
}
