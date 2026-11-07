//! Metadata, paging and plans. Everything here runs on a read-only connection.

use crate::execute::decode;
use crate::session::map_error;
use rusqlite::{params_from_iter, Connection, Row};
use std::time::Duration;
use studio_db::sql::{order_by, qualified_ref, FilterBuilder};
use studio_db::DbError;
use studio_types::{
    ColumnInfo, ColumnMeta, ConstraintInfo, DbObject, Engine, FilterRule, ForeignKeyInfo, IndexInfo,
    ObjectKind, PageRequest, TableMetadata, TablePage, TableRef, TableStructure,
};

const SQ: Engine = Engine::Sqlite;

const COLUMNS_SQL: &str = "
    SELECT m.name, p.name, p.type, p.\"notnull\", p.dflt_value, p.pk, p.cid
    FROM sqlite_schema m JOIN pragma_table_info(m.name) p
    WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite_%'";

fn column_from_row(row: &Row<'_>) -> rusqlite::Result<ColumnInfo> {
    let cid: i64 = row.get(6)?;
    Ok(ColumnInfo {
        table: row.get(0)?,
        name: row.get(1)?,
        data_type: row.get(2)?,
        nullable: row.get::<_, i64>(3)? == 0,
        default_value: row.get(4)?,
        primary_key: row.get::<_, i64>(5)? > 0,
        ordinal: u32::try_from(cid + 1).unwrap_or(0),
    })
}

pub(crate) fn list_objects(conn: &Connection, schema: &str) -> Result<Vec<DbObject>, DbError> {
    let mut statement = conn
        .prepare(
            "SELECT name, type FROM sqlite_schema
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(map_error)?;
    let rows = statement
        .query_map([], |row| {
            let kind: String = row.get(1)?;
            Ok(DbObject {
                name: row.get(0)?,
                schema: schema.to_owned(),
                kind: if kind == "view" {
                    ObjectKind::View
                } else {
                    ObjectKind::Table
                },
            })
        })
        .map_err(map_error)?;
    rows.collect::<Result<_, _>>().map_err(map_error)
}

pub(crate) fn columns(conn: &Connection) -> Result<Vec<ColumnInfo>, DbError> {
    let sql = format!("{COLUMNS_SQL} ORDER BY m.name, p.cid");
    let mut statement = conn.prepare(&sql).map_err(map_error)?;
    let rows = statement.query_map([], column_from_row).map_err(map_error)?;
    rows.collect::<Result<_, _>>().map_err(map_error)
}

fn indexes(conn: &Connection, table: &str) -> Result<Vec<IndexInfo>, DbError> {
    let mut statement = conn
        .prepare(
            "SELECT l.name, l.\"unique\", l.origin, (SELECT sql FROM sqlite_schema s WHERE s.name = l.name)
             FROM pragma_index_list(?1) l ORDER BY l.name",
        )
        .map_err(map_error)?;
    let found: Vec<(String, i64, String, Option<String>)> = statement
        .query_map([table], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(map_error)?
        .collect::<Result<_, _>>()
        .map_err(map_error)?;
    let mut out = Vec::with_capacity(found.len());
    for (name, unique, origin, definition) in found {
        let mut info = conn
            .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
            .map_err(map_error)?;
        let columns: Vec<Option<String>> = info
            .query_map([&name], |r| r.get(0))
            .map_err(map_error)?
            .collect::<Result<_, _>>()
            .map_err(map_error)?;
        out.push(IndexInfo {
            name,
            columns: columns.into_iter().flatten().collect(),
            unique: unique != 0,
            primary: origin == "pk",
            definition,
        });
    }
    Ok(out)
}

fn foreign_keys(conn: &Connection, table: &TableRef) -> Result<Vec<ForeignKeyInfo>, DbError> {
    let mut statement = conn
        .prepare("SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1) ORDER BY id, seq")
        .map_err(map_error)?;
    let rows: Vec<(i64, String, String, Option<String>)> = statement
        .query_map([&table.name], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(map_error)?
        .collect::<Result<_, _>>()
        .map_err(map_error)?;
    let mut keys: Vec<(i64, ForeignKeyInfo)> = Vec::new();
    for (id, target, from, to) in rows {
        if keys.last().is_none_or(|(last, _)| *last != id) {
            keys.push((
                id,
                ForeignKeyInfo {
                    name: format!("fk_{id}"),
                    columns: Vec::new(),
                    referenced_schema: table.schema.clone(),
                    referenced_table: target,
                    referenced_columns: Vec::new(),
                    definition: None,
                },
            ));
        }
        if let Some((_, key)) = keys.last_mut() {
            key.columns.push(from);
            key.referenced_columns.extend(to);
        }
    }
    Ok(keys.into_iter().map(|(_, key)| key).collect())
}

pub(crate) fn table_structure(conn: &Connection, table: &TableRef) -> Result<TableStructure, DbError> {
    let (kind, ddl): (String, Option<String>) = conn
        .query_row(
            "SELECT type, sql FROM sqlite_schema WHERE name = ?1",
            [&table.name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(map_error)?;
    let sql = format!("{COLUMNS_SQL} AND m.name = ?1 ORDER BY p.cid");
    let mut statement = conn.prepare(&sql).map_err(map_error)?;
    let columns = statement
        .query_map([&table.name], column_from_row)
        .map_err(map_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_error)?;
    let row_estimate = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {}", qualified_ref(SQ, table)),
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(TableStructure {
        table: table.clone(),
        kind: if kind == "view" {
            ObjectKind::View
        } else {
            ObjectKind::Table
        },
        columns,
        indexes: indexes(conn, &table.name)?,
        foreign_keys: foreign_keys(conn, table)?,
        constraints: Vec::<ConstraintInfo>::new(),
        metadata: TableMetadata {
            row_estimate,
            ..Default::default()
        },
        ddl,
    })
}

pub(crate) fn table_page(
    conn: &Connection,
    table: &TableRef,
    page: &PageRequest,
) -> Result<TablePage, DbError> {
    let filter = FilterBuilder::new(SQ).build(&page.filters);
    let sql = format!(
        "SELECT * FROM {}{}{} LIMIT {} OFFSET {}",
        qualified_ref(SQ, table),
        filter.sql,
        order_by(SQ, &page.sort),
        page.limit,
        page.offset
    );
    let mut statement = conn.prepare(&sql).map_err(map_error)?;
    let declared: Vec<Option<String>> = statement
        .columns()
        .iter()
        .map(|c| c.decl_type().map(str::to_owned))
        .collect();
    let columns: Vec<ColumnMeta> = statement
        .column_names()
        .iter()
        .zip(&declared)
        .map(|(name, ty)| ColumnMeta {
            name: (*name).to_owned(),
            type_name: ty.clone().unwrap_or_default(),
            nullable: None,
        })
        .collect();
    let mut rows = statement
        .query(params_from_iter(filter.params.iter()))
        .map_err(map_error)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(map_error)? {
        let mut cells = Vec::with_capacity(declared.len());
        for (index, ty) in declared.iter().enumerate() {
            cells.push(decode(row.get_ref(index).map_err(map_error)?, ty.as_deref()));
        }
        out.push(cells);
    }
    Ok(TablePage { columns, rows: out })
}

pub(crate) fn table_count(
    conn: &Connection,
    table: &TableRef,
    filters: &[FilterRule],
    timeout: Duration,
) -> Result<Option<u64>, DbError> {
    let filter = FilterBuilder::new(SQ).build(filters);
    let sql = format!("SELECT COUNT(*) FROM {}{}", qualified_ref(SQ, table), filter.sql);
    let handle = conn.get_interrupt_handle();
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let timer = std::thread::spawn(move || {
        if wait.recv_timeout(timeout).is_err() {
            handle.interrupt();
        }
    });
    let result = conn.query_row(&sql, params_from_iter(filter.params.iter()), |r| {
        r.get::<_, i64>(0)
    });
    let _ = done.send(());
    let _ = timer.join();
    match result.map_err(map_error) {
        Ok(count) => Ok(Some(u64::try_from(count).unwrap_or(0))),
        Err(DbError::Cancelled) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn explain(conn: &Connection, stmt: &str) -> Result<String, DbError> {
    let mut statement = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {stmt}"))
        .map_err(map_error)?;
    let rows = statement
        .query_map([], |row| {
            let (id, parent): (i64, i64) = (row.get(0)?, row.get(1)?);
            let detail: String = row.get(3)?;
            Ok(serde_json::json!({ "id": id, "parent": parent, "detail": detail }))
        })
        .map_err(map_error)?;
    let plan: Vec<serde_json::Value> = rows.collect::<Result<_, _>>().map_err(map_error)?;
    Ok(serde_json::Value::Array(plan).to_string())
}
