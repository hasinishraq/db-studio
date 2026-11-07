//! Results, verdicts, filters and other run-time DTOs.

use crate::error::AppError;
use crate::ids::{DraftId, ProfileId, RunId};
use crate::profile::Environment;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use specta::Type;

/// A result column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    /// Column name.
    pub name: String,
    /// Database type name.
    pub type_name: String,
    /// Nullability, when the engine reports it.
    pub nullable: Option<bool>,
}

/// A batch of rows from one statement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RowBatch {
    /// Sequence number within the statement, from 0.
    pub seq: u32,
    /// Rows, each with one value per column.
    pub rows: Vec<Vec<Value>>,
}

/// The result of one statement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StatementOutcome {
    /// The statement returned rows.
    Rows {
        /// Columns.
        columns: Vec<ColumnMeta>,
        /// Rows delivered.
        total_rows: u64,
        /// Whether the row cap stopped reading.
        truncated: bool,
        /// Elapsed milliseconds.
        elapsed_ms: u64,
    },
    /// The statement returned no rows.
    Command {
        /// Rows affected.
        rows_affected: u64,
        /// Elapsed milliseconds.
        elapsed_ms: u64,
    },
    /// The statement failed.
    Error {
        /// The failure.
        error: AppError,
    },
}

/// What a driver reports after executing one statement.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExecSummary {
    /// Columns; empty for commands.
    pub columns: Vec<ColumnMeta>,
    /// Whether the statement produces a row set.
    pub returns_rows: bool,
    /// Rows delivered to the sink.
    pub total_rows: u64,
    /// Rows affected, for commands.
    pub rows_affected: u64,
    /// Whether the row cap stopped reading.
    pub truncated: bool,
    /// Elapsed milliseconds.
    pub elapsed_ms: u64,
}

/// Byte offsets of a statement in the script.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    /// Start byte offset.
    pub start: u32,
    /// End byte offset, exclusive.
    pub end: u32,
}

/// Broad statement category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StatementKind {
    /// SELECT and friends.
    Select,
    /// SHOW or DESCRIBE.
    Show,
    /// EXPLAIN.
    Explain,
    /// INSERT.
    Insert,
    /// UPDATE.
    Update,
    /// DELETE.
    Delete,
    /// MERGE.
    Merge,
    /// CREATE, ALTER, DROP, TRUNCATE, RENAME.
    Ddl,
    /// GRANT, REVOKE, SET ROLE.
    Dcl,
    /// BEGIN, COMMIT, ROLLBACK, SAVEPOINT.
    Tcl,
    /// Anything else.
    Other,
}

/// A statement's verdict as shown to the UI. Produced only by `studio-safety`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StatementVerdict {
    /// Where the statement is in the script.
    pub span: Span,
    /// Category.
    pub kind: StatementKind,
    /// Whether it may run without a human on the user path.
    pub read_only: bool,
    /// Plain-language reasons this is high risk; empty when normal.
    pub risk_reasons: Vec<String>,
    /// Why it is not a plain read, if it is not.
    pub reason: Option<String>,
}

/// A single-use approval. Opaque to the frontend.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ApprovalToken(pub String);

impl std::fmt::Debug for ApprovalToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalToken(<redacted>)")
    }
}

/// The reply to `approve_query`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalGrant {
    /// The single-use token.
    pub token: ApprovalToken,
    /// Verdicts for the approved script.
    pub verdicts: Vec<StatementVerdict>,
}

/// Messages on a run's channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RunEvent {
    /// A statement began.
    StatementStart {
        /// Index in the script.
        index: u32,
        /// Where it is.
        span: Span,
    },
    /// A statement's columns are known; sent before its first rows.
    Columns {
        /// Index in the script.
        index: u32,
        /// Columns.
        columns: Vec<ColumnMeta>,
    },
    /// A batch of rows.
    Rows {
        /// Index in the script.
        index: u32,
        /// The batch.
        batch: RowBatch,
    },
    /// A statement ended.
    StatementDone {
        /// Index in the script.
        index: u32,
        /// How it ended.
        outcome: StatementOutcome,
    },
    /// The run ended.
    Finished,
}

/// Filter operators for table data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum FilterOperator {
    /// Equal.
    Eq,
    /// Not equal.
    NotEq,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Contains the text.
    Contains,
    /// Starts with the text.
    StartsWith,
    /// Ends with the text.
    EndsWith,
    /// Is NULL.
    IsNull,
    /// Is not NULL.
    IsNotNull,
}

/// One filter on a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FilterRule {
    /// Column name.
    pub column: String,
    /// Operator.
    pub operator: FilterOperator,
    /// Operand; ignored by the NULL operators.
    pub value: Option<String>,
}

/// One sort key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    /// Column name.
    pub column: String,
    /// Descending when true.
    pub descending: bool,
}

/// A page request for a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageRequest {
    /// Page size.
    pub limit: u32,
    /// Rows to skip.
    pub offset: u64,
    /// Sort keys.
    pub sort: Vec<SortSpec>,
    /// Filters, combined with AND.
    pub filters: Vec<FilterRule>,
}

/// One page of table data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    /// Columns.
    pub columns: Vec<ColumnMeta>,
    /// Rows.
    pub rows: Vec<Vec<Value>>,
}

/// Commit behaviour of a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TxMode {
    /// Each statement commits by itself.
    #[default]
    Auto,
    /// Statements share an open transaction until commit or rollback.
    Manual,
}

/// Transaction state shown to the UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TxState {
    /// Mode.
    pub mode: TxMode,
    /// Whether uncommitted writes exist.
    pub dirty: bool,
}

/// Where an approved statement came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Origin {
    /// Typed in the SQL editor.
    Editor,
    /// Generated by a grid edit.
    Grid,
    /// Generated by the import wizard.
    Import,
    /// Proposed by the AI.
    Ai,
}

/// One run in the history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// The run.
    pub id: RunId,
    /// The profile.
    pub profile_id: Option<ProfileId>,
    /// SQL text.
    pub sql: String,
    /// Start time, milliseconds since the epoch.
    pub started_at: i64,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Rows returned or affected.
    pub row_count: i64,
    /// `ok`, `error` or `cancelled`.
    pub status: String,
}

/// A saved query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// Draft id.
    pub id: DraftId,
    /// The profile.
    pub profile_id: Option<ProfileId>,
    /// Title.
    pub title: String,
    /// Current text.
    pub sql: String,
    /// Text at the last explicit save.
    pub saved_sql: String,
    /// Last update, milliseconds since the epoch.
    pub updated_at: i64,
}

/// An audit record about to be written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// Time, milliseconds since the epoch.
    pub at: i64,
    /// The profile.
    pub profile_id: Option<ProfileId>,
    /// Environment.
    pub environment: Environment,
    /// Who asked for it.
    pub origin: Origin,
    /// Statement category.
    pub kind: StatementKind,
    /// SHA-256 of the SQL, hex.
    pub sql_hash: String,
    /// SQL text.
    pub sql_text: String,
    /// Rows affected, when known.
    pub rows_affected: Option<i64>,
    /// `ok`, `error` or `cancelled`.
    pub outcome: String,
}

/// A stored audit record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    /// Row id.
    pub id: i64,
    /// The entry.
    pub entry: AuditEntry,
    /// Hash of the previous record.
    pub prev_hash: String,
    /// Hash of this record.
    pub entry_hash: String,
}

/// Filters for the audit viewer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    /// Only this profile.
    pub profile_id: Option<ProfileId>,
    /// Only this origin.
    pub origin: Option<Origin>,
    /// Only this kind.
    pub kind: Option<StatementKind>,
    /// Not before this time.
    pub from: Option<i64>,
    /// Not after this time.
    pub to: Option<i64>,
    /// Page size.
    pub limit: u32,
}

/// One column value for a planned row change. `None` means SQL NULL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CellValue {
    /// Column name.
    pub column: String,
    /// Text form of the value; `None` is NULL.
    pub value: Option<String>,
}

/// A grid edit, turned into SQL by Rust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RowChange {
    /// Insert one row.
    Insert {
        /// Values.
        values: Vec<CellValue>,
    },
    /// Update one row.
    Update {
        /// Identifying columns.
        key: Vec<CellValue>,
        /// New values.
        values: Vec<CellValue>,
    },
    /// Delete rows.
    Delete {
        /// One identifying set per row.
        keys: Vec<Vec<CellValue>>,
    },
}

/// SQL planned by Rust for the normal review flow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSql {
    /// The statement.
    pub sql: String,
    /// Warnings for the confirm dialog.
    pub warnings: Vec<String>,
}

/// A file format for export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ExportFormat {
    /// Comma-separated values.
    Csv,
    /// One JSON object per line.
    JsonLines,
    /// An Excel workbook.
    Xlsx,
    /// SQL INSERT statements.
    SqlInserts,
}

/// One new column for `plan_create_table`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewColumn {
    /// Name.
    pub name: String,
    /// Type, as the engine names it.
    pub data_type: String,
    /// Whether NULL is allowed.
    pub nullable: bool,
    /// Whether it is part of the primary key.
    pub primary_key: bool,
    /// Default value, as a literal.
    pub default_value: Option<String>,
}

/// Maps one CSV column to one table column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportMapping {
    /// Position in the CSV, from 0.
    pub csv_column: u32,
    /// Target column.
    pub column: String,
}

/// A CSV the user picked, with a preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Opaque handle; the webview never holds the path.
    pub file_id: String,
    /// File name for display.
    pub file_name: String,
    /// Header row.
    pub headers: Vec<String>,
    /// First rows.
    pub rows: Vec<Vec<String>>,
}

/// What the user reviews before an import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    /// The text the approval is bound to: first statement plus a hash of the file.
    pub review_sql: String,
    /// Rows the file holds.
    pub rows: u64,
}

/// Facts for a bug report. Holds no secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// App version.
    pub version: String,
    /// Operating system.
    pub os: String,
    /// Last log lines, with secrets removed.
    pub log_tail: Vec<String>,
}

/// What a legacy import did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportSummary {
    /// Whether old data was found.
    pub found: bool,
    /// Profiles imported.
    pub profiles: u32,
    /// Drafts imported.
    pub drafts: u32,
    /// History entries imported.
    pub history: u32,
    /// Notes for the user.
    pub notes: Vec<String>,
}
