//! Schema metadata DTOs.

use serde::{Deserialize, Serialize};
use specta::Type;

/// A schema; a database on MySQL; `main` on SQLite.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SchemaRef {
    /// Name.
    pub name: String,
}

/// A table-like object's identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableRef {
    /// Schema.
    pub schema: String,
    /// Name.
    pub name: String,
}

/// Kind of table-like object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ObjectKind {
    /// A table.
    Table,
    /// A view.
    View,
    /// A materialized view.
    MaterializedView,
}

/// A table-like object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DbObject {
    /// Name.
    pub name: String,
    /// Schema.
    pub schema: String,
    /// Kind.
    pub kind: ObjectKind,
}

/// A column of a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    /// Owning table.
    pub table: String,
    /// Column name.
    pub name: String,
    /// Type as the engine names it.
    pub data_type: String,
    /// Whether NULL is allowed.
    pub nullable: bool,
    /// Default expression.
    pub default_value: Option<String>,
    /// Whether it is part of the primary key.
    pub primary_key: bool,
    /// Position, from 1.
    pub ordinal: u32,
}

/// An index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    /// Name.
    pub name: String,
    /// Columns in order.
    pub columns: Vec<String>,
    /// Whether it is unique.
    pub unique: bool,
    /// Whether it backs the primary key.
    pub primary: bool,
    /// Definition, when available.
    pub definition: Option<String>,
}

/// A foreign key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyInfo {
    /// Constraint name.
    pub name: String,
    /// Local columns.
    pub columns: Vec<String>,
    /// Referenced schema.
    pub referenced_schema: String,
    /// Referenced table.
    pub referenced_table: String,
    /// Referenced columns.
    pub referenced_columns: Vec<String>,
}

/// A table constraint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintInfo {
    /// Name.
    pub name: String,
    /// Kind, such as CHECK or UNIQUE.
    pub kind: String,
    /// Definition, when available.
    pub definition: Option<String>,
}

/// Size and ownership facts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableMetadata {
    /// Owner.
    pub owner: Option<String>,
    /// Size in bytes.
    pub size_bytes: Option<i64>,
    /// Estimated rows.
    pub row_estimate: Option<i64>,
    /// Comment.
    pub comment: Option<String>,
}

/// Everything the structure view shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableStructure {
    /// The object.
    pub table: TableRef,
    /// Kind.
    pub kind: ObjectKind,
    /// Columns.
    pub columns: Vec<ColumnInfo>,
    /// Indexes.
    pub indexes: Vec<IndexInfo>,
    /// Foreign keys.
    pub foreign_keys: Vec<ForeignKeyInfo>,
    /// Other constraints.
    pub constraints: Vec<ConstraintInfo>,
    /// Size and ownership.
    pub metadata: TableMetadata,
    /// DDL text.
    pub ddl: Option<String>,
}
