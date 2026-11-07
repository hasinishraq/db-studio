//! Metadata from `information_schema` and `SHOW`. Every identifier is escaped.

use crate::connect::{map_error, MySession};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};
use std::collections::BTreeMap;
use studio_db::sql::qualified_ref;
use studio_db::DbError;
use studio_types::{
    ColumnInfo, ConstraintInfo, DbObject, Engine, ForeignKeyInfo, IndexInfo, ObjectKind, SchemaRef,
    TableMetadata, TableRef, TableStructure,
};

const HIDDEN: [&str; 4] = ["mysql", "sys", "performance_schema", "information_schema"];

const COLUMNS_SQL: &str = "
    SELECT table_name, column_name, column_type, is_nullable, column_default, column_key, ordinal_position
    FROM information_schema.columns WHERE table_schema = ?";

fn text(row: &Row, index: usize) -> Result<String, DbError> {
    optional(row, index)?.ok_or_else(|| DbError::Internal("a metadata value is missing".to_owned()))
}

fn optional(row: &Row, index: usize) -> Result<Option<String>, DbError> {
    match row.as_ref(index) {
        None | Some(mysql_async::Value::NULL) => Ok(None),
        Some(mysql_async::Value::Bytes(bytes)) => Ok(Some(String::from_utf8_lossy(bytes).into_owned())),
        Some(other) => Ok(Some(other.as_sql(true).trim_matches('\'').to_owned())),
    }
}

fn number(row: &Row, index: usize) -> Result<Option<i64>, DbError> {
    Ok(optional(row, index)?.and_then(|value| value.parse().ok()))
}

fn column_from_row(row: &Row) -> Result<ColumnInfo, DbError> {
    Ok(ColumnInfo {
        table: text(row, 0)?,
        name: text(row, 1)?,
        data_type: text(row, 2)?,
        nullable: text(row, 3)? == "YES",
        default_value: optional(row, 4)?,
        primary_key: text(row, 5)? == "PRI",
        ordinal: number(row, 6)?.and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
    })
}

pub(crate) async fn list_schemas(session: &MySession) -> Result<Vec<SchemaRef>, DbError> {
    let mut conn = session.shared.get().await?;
    let rows: Vec<Row> = conn.query("SHOW DATABASES").await.map_err(map_error)?;
    let mut schemas = Vec::with_capacity(rows.len());
    for row in &rows {
        let name = text(row, 0)?;
        if !HIDDEN.contains(&name.to_lowercase().as_str()) {
            schemas.push(SchemaRef { name });
        }
    }
    Ok(schemas)
}

pub(crate) async fn list_objects(session: &MySession, schema: &SchemaRef) -> Result<Vec<DbObject>, DbError> {
    let mut conn = session.shared.get().await?;
    let rows: Vec<Row> = conn
        .exec(
            "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = ? ORDER BY 1",
            (schema.name.as_str(),),
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|row| {
            let kind = text(row, 1)?;
            Ok(DbObject {
                name: text(row, 0)?,
                schema: schema.name.clone(),
                kind: if kind.contains("VIEW") {
                    ObjectKind::View
                } else {
                    ObjectKind::Table
                },
            })
        })
        .collect()
}

pub(crate) async fn columns(session: &MySession, schema: &SchemaRef) -> Result<Vec<ColumnInfo>, DbError> {
    let mut conn = session.shared.get().await?;
    let sql = format!("{COLUMNS_SQL} ORDER BY table_name, ordinal_position");
    let rows: Vec<Row> = conn.exec(sql, (schema.name.as_str(),)).await.map_err(map_error)?;
    rows.iter().map(column_from_row).collect()
}

async fn indexes(conn: &mut Conn, table: &TableRef) -> Result<Vec<IndexInfo>, DbError> {
    let sql = format!("SHOW INDEX FROM {}", qualified_ref(Engine::MySql, table));
    let rows: Vec<Row> = conn.query(sql).await.map_err(map_error)?;
    let mut by_name: BTreeMap<String, IndexInfo> = BTreeMap::new();
    for row in &rows {
        let name = text(row, 2)?;
        let entry = by_name.entry(name.clone()).or_insert_with(|| IndexInfo {
            primary: name == "PRIMARY",
            name,
            columns: Vec::new(),
            unique: false,
            definition: None,
        });
        entry.unique = number(row, 1)? == Some(0);
        if let Some(column) = optional(row, 4)? {
            entry.columns.push(column);
        }
    }
    Ok(by_name.into_values().collect())
}

async fn foreign_keys(conn: &mut Conn, table: &TableRef) -> Result<Vec<ForeignKeyInfo>, DbError> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT constraint_name, column_name, referenced_table_schema, referenced_table_name,
                    referenced_column_name
             FROM information_schema.key_column_usage
             WHERE table_schema = ? AND table_name = ? AND referenced_table_name IS NOT NULL
             ORDER BY constraint_name, ordinal_position",
            (table.schema.as_str(), table.name.as_str()),
        )
        .await
        .map_err(map_error)?;
    let mut by_name: BTreeMap<String, ForeignKeyInfo> = BTreeMap::new();
    for row in &rows {
        let name = text(row, 0)?;
        let referenced_schema = text(row, 2)?;
        let referenced_table = text(row, 3)?;
        let entry = by_name.entry(name.clone()).or_insert(ForeignKeyInfo {
            name,
            columns: Vec::new(),
            referenced_schema,
            referenced_table,
            referenced_columns: Vec::new(),
            definition: None,
        });
        entry.columns.push(text(row, 1)?);
        entry.referenced_columns.push(text(row, 4)?);
    }
    Ok(by_name.into_values().collect())
}

async fn constraints(conn: &mut Conn, table: &TableRef) -> Result<Vec<ConstraintInfo>, DbError> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT tc.constraint_name, tc.constraint_type, cc.check_clause
             FROM information_schema.table_constraints tc
             LEFT JOIN information_schema.check_constraints cc
               ON cc.constraint_schema = tc.constraint_schema AND cc.constraint_name = tc.constraint_name
             WHERE tc.table_schema = ? AND tc.table_name = ? AND tc.constraint_type <> 'FOREIGN KEY'
             ORDER BY 1",
            (table.schema.as_str(), table.name.as_str()),
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|row| {
            Ok(ConstraintInfo {
                name: text(row, 0)?,
                kind: text(row, 1)?,
                definition: optional(row, 2)?,
            })
        })
        .collect()
}

pub(crate) async fn table_structure(
    session: &MySession,
    table: &TableRef,
) -> Result<TableStructure, DbError> {
    let mut conn = session.shared.get().await?;
    let facts: Option<Row> = conn
        .exec_first(
            "SELECT table_type, data_length + index_length, table_rows, table_comment
             FROM information_schema.tables WHERE table_schema = ? AND table_name = ?",
            (table.schema.as_str(), table.name.as_str()),
        )
        .await
        .map_err(map_error)?;
    let facts = facts.ok_or_else(|| DbError::query("the table does not exist"))?;
    let kind = if text(&facts, 0)?.contains("VIEW") {
        ObjectKind::View
    } else {
        ObjectKind::Table
    };
    let metadata = TableMetadata {
        owner: None,
        size_bytes: number(&facts, 1)?,
        row_estimate: number(&facts, 2)?,
        comment: optional(&facts, 3)?.filter(|c| !c.is_empty()),
    };

    let sql = format!("{COLUMNS_SQL} AND table_name = ? ORDER BY ordinal_position");
    let rows: Vec<Row> = conn
        .exec(sql, (table.schema.as_str(), table.name.as_str()))
        .await
        .map_err(map_error)?;
    let columns = rows.iter().map(column_from_row).collect::<Result<Vec<_>, _>>()?;

    let keyword = if kind == ObjectKind::View { "VIEW" } else { "TABLE" };
    let show = format!("SHOW CREATE {keyword} {}", qualified_ref(Engine::MySql, table));
    let ddl: Option<Row> = conn.query_first(show).await.map_err(map_error)?;
    let ddl = ddl.map(|row| text(&row, 1)).transpose()?;

    Ok(TableStructure {
        table: table.clone(),
        kind,
        columns,
        indexes: indexes(&mut conn, table).await?,
        foreign_keys: foreign_keys(&mut conn, table).await?,
        constraints: constraints(&mut conn, table).await?,
        metadata,
        ddl,
    })
}
