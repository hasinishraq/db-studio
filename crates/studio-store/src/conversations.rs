//! AI conversations. Turns hold words and statement outcomes, never result rows.

use crate::db::{now_ms, parse_id, parse_json, to_json, Store, StoreError};
use sqlx::Row;
use studio_types::{ConversationId, ConversationSummary, ProfileId, QueryRecord, TurnId, TurnRecord};

impl Store {
    /// Creates the conversation if it does not exist.
    pub async fn ensure_conversation(
        &self,
        id: ConversationId,
        profile: Option<ProfileId>,
        title: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT OR IGNORE INTO conversations (id, profile_id, title, updated_at) VALUES (?1,?2,?3,?4)",
        )
        .bind(id.to_string())
        .bind(profile.map(|p| p.to_string()))
        .bind(title.to_owned())
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists conversations, newest first.
    pub async fn list_conversations(&self) -> Result<Vec<ConversationSummary>, StoreError> {
        let rows = sqlx::query("SELECT id, title, updated_at FROM conversations ORDER BY updated_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                Ok(ConversationSummary {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    title: row.try_get("title")?,
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect()
    }

    /// Renames a conversation.
    pub async fn rename_conversation(&self, id: ConversationId, title: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE conversations SET title = ?2 WHERE id = ?1")
            .bind(id.to_string())
            .bind(title.to_owned())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Appends a turn after the last one and returns its sequence number.
    pub async fn append_turn(
        &self,
        conversation: ConversationId,
        id: TurnId,
        user_text: &str,
        assistant_text: &str,
        queries: &[QueryRecord],
    ) -> Result<i64, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row =
            sqlx::query("SELECT COALESCE(MAX(seq), -1) + 1 AS next FROM turns WHERE conversation_id = ?1")
                .bind(conversation.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let seq: i64 = row.try_get("next")?;
        sqlx::query(
            "INSERT INTO turns (id, conversation_id, seq, user_text, assistant_text, queries_json)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(id.to_string())
        .bind(conversation.to_string())
        .bind(seq)
        .bind(user_text.to_owned())
        .bind(assistant_text.to_owned())
        .bind(to_json(&queries)?)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE conversations SET updated_at = ?2 WHERE id = ?1")
            .bind(conversation.to_string())
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(seq)
    }

    /// Lists a conversation's turns in order.
    pub async fn list_turns(&self, conversation: ConversationId) -> Result<Vec<TurnRecord>, StoreError> {
        let rows = sqlx::query("SELECT * FROM turns WHERE conversation_id = ?1 ORDER BY seq")
            .bind(conversation.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| {
                Ok(TurnRecord {
                    id: parse_id(&row.try_get::<String, _>("id")?)?,
                    seq: row.try_get("seq")?,
                    user_text: row.try_get("user_text")?,
                    assistant_text: row.try_get("assistant_text")?,
                    queries: parse_json(&row.try_get::<String, _>("queries_json")?)?,
                })
            })
            .collect()
    }

    /// Deletes a turn and every later turn. Does nothing when the turn is unknown.
    pub async fn forget_from(&self, conversation: ConversationId, turn: TurnId) -> Result<(), StoreError> {
        sqlx::query(
            "DELETE FROM turns WHERE conversation_id = ?1
               AND seq >= (SELECT seq FROM turns WHERE id = ?2 AND conversation_id = ?1)",
        )
        .bind(conversation.to_string())
        .bind(turn.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes a conversation and its turns.
    pub async fn clear_conversation(&self, conversation: ConversationId) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM conversations WHERE id = ?1")
            .bind(conversation.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Reads the summary of earlier turns.
    pub async fn get_summary(&self, conversation: ConversationId) -> Result<Option<String>, StoreError> {
        let row = sqlx::query("SELECT summary FROM conversations WHERE id = ?1")
            .bind(conversation.to_string())
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => Ok(row.try_get("summary")?),
            None => Ok(None),
        }
    }

    /// Stores a summary without touching turns.
    pub async fn set_summary(&self, conversation: ConversationId, summary: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE conversations SET summary = ?2 WHERE id = ?1")
            .bind(conversation.to_string())
            .bind(summary.to_owned())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Atomically stores a summary and deletes all but the last `keep` turns.
    pub async fn compact(
        &self,
        conversation: ConversationId,
        summary: &str,
        keep: u32,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE conversations SET summary = ?2 WHERE id = ?1")
            .bind(conversation.to_string())
            .bind(summary.to_owned())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM turns WHERE conversation_id = ?1 AND id NOT IN
               (SELECT id FROM turns WHERE conversation_id = ?1 ORDER BY seq DESC LIMIT ?2)",
        )
        .bind(conversation.to_string())
        .bind(i64::from(keep))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
