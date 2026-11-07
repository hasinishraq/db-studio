//! AI assistant DTOs.

use crate::ids::{ConversationId, QueryId, SessionId, TurnId};
use crate::query::ColumnMeta;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Messages on an AI turn's channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AiEvent {
    /// A piece of the reply.
    Text {
        /// The text.
        text: String,
    },
    /// The model asked to run a statement.
    QueryStart {
        /// Query id.
        id: QueryId,
        /// Exact SQL.
        sql: String,
        /// Whether studio-safety allows it unattended.
        read_only: bool,
        /// Why it is not allowed unattended.
        reason: Option<String>,
        /// Whether it waits for a human.
        needs_review: bool,
        /// High-risk reasons.
        risk_reasons: Vec<String>,
    },
    /// A human approved it.
    QueryApproved {
        /// Query id.
        id: QueryId,
    },
    /// A human declined it, or the turn ended.
    QueryDeclined {
        /// Query id.
        id: QueryId,
    },
    /// It was cancelled while running.
    QueryCancelled {
        /// Query id.
        id: QueryId,
    },
    /// It finished. Rows go to the UI only.
    QueryResult {
        /// Query id.
        id: QueryId,
        /// Columns.
        columns: Vec<ColumnMeta>,
        /// Rows.
        rows: Vec<Vec<Value>>,
        /// Rows affected, for commands.
        rows_affected: Option<u64>,
        /// Whether the cap stopped reading.
        truncated: bool,
        /// Elapsed milliseconds.
        elapsed_ms: u64,
    },
    /// It failed.
    QueryError {
        /// Query id.
        id: QueryId,
        /// Message.
        message: String,
    },
    /// The turn ended.
    Done {
        /// Set when the turn ended on an error.
        error: Option<String>,
    },
}

/// Starts an AI turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSendRequest {
    /// Conversation.
    pub conversation_id: ConversationId,
    /// Id for the new turn, chosen by the caller so it can cancel at once.
    pub turn_id: TurnId,
    /// Target session; `None` means no statement can run.
    pub session_id: Option<SessionId>,
    /// The user's words.
    pub text: String,
    /// Run safe reads without asking.
    pub auto_run: bool,
    /// Let the model read result samples.
    pub share_rows: bool,
}

/// A column whose values are hidden from the AI and optionally the grid.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MaskingRule {
    /// Schema.
    pub schema_name: String,
    /// Table.
    pub table_name: String,
    /// Column.
    pub column_name: String,
}

/// A stored conversation, for the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    /// Id.
    pub id: ConversationId,
    /// Title.
    pub title: String,
    /// Last update, milliseconds since the epoch.
    pub updated_at: i64,
}

/// What memory keeps about a statement. Never rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueryRecord {
    /// Exact SQL.
    pub sql: String,
    /// `ok`, `declined`, `cancelled` or `error`.
    pub outcome: String,
    /// Rows returned or affected.
    pub row_count: Option<i64>,
}

/// A stored turn. Never contains rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    /// Turn id.
    pub id: TurnId,
    /// Position in the conversation.
    pub seq: i64,
    /// The user's words.
    pub user_text: String,
    /// The assistant's words.
    pub assistant_text: String,
    /// Statements and outcomes.
    pub queries: Vec<QueryRecord>,
}
