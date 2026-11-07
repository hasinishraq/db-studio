//! Catalog queries.

use crate::connect::{client, map_error, PgSession};
use sha2::{Digest, Sha256};
use studio_db::sql::qualified_ref;
use studio_db::DbError;
use studio_types::{
    CertificateInfo, ColumnInfo, ConstraintInfo, DbObject, Engine, ForeignKeyInfo, IndexInfo, ObjectKind,
    SchemaRef, TableMetadata, TableRef, TableStructure,
};
use tokio_postgres::Client;
use x509_parser::prelude::{FromDer, X509Certificate};

const COLUMNS_SQL: &str = "
    SELECT c.relname::text AS table_name, a.attname::text AS name,
           pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type, NOT a.attnotnull AS nullable,
           pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS default_value,
           COALESCE(a.attnum = ANY(i.indkey), false) AS primary_key, a.attnum::int4 AS ordinal
    FROM pg_catalog.pg_attribute a
    JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
    LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    LEFT JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary
    WHERE n.nspname = $1 AND a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r','p','v','m','f')";

fn column_from_row(row: &tokio_postgres::Row) -> Result<ColumnInfo, DbError> {
    let ordinal: i32 = row.try_get("ordinal").map_err(map_error)?;
    Ok(ColumnInfo {
        table: row.try_get("table_name").map_err(map_error)?,
        name: row.try_get("name").map_err(map_error)?,
        data_type: row.try_get("data_type").map_err(map_error)?,
        nullable: row.try_get("nullable").map_err(map_error)?,
        default_value: row.try_get("default_value").map_err(map_error)?,
        primary_key: row.try_get("primary_key").map_err(map_error)?,
        ordinal: u32::try_from(ordinal).unwrap_or(0),
    })
}

pub(crate) async fn list_databases(session: &PgSession) -> Result<Vec<String>, DbError> {
    let object = session.free().await?;
    let rows = client(&object)
        .query(
            "SELECT datname::text FROM pg_catalog.pg_database
             WHERE datallowconn AND NOT datistemplate AND has_database_privilege(datname, 'CONNECT')
             ORDER BY 1",
            &[],
        )
        .await
        .map_err(map_error)?;
    rows.iter().map(|r| r.try_get(0).map_err(map_error)).collect()
}

pub(crate) async fn list_schemas(session: &PgSession) -> Result<Vec<SchemaRef>, DbError> {
    let object = session.free().await?;
    let rows = client(&object)
        .query(
            "SELECT nspname::text FROM pg_catalog.pg_namespace
             WHERE nspname !~ '^pg_' AND nspname <> 'information_schema' ORDER BY 1",
            &[],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            Ok(SchemaRef {
                name: r.try_get(0).map_err(map_error)?,
            })
        })
        .collect()
}

fn object_kind(relkind: &str) -> ObjectKind {
    match relkind {
        "v" => ObjectKind::View,
        "m" => ObjectKind::MaterializedView,
        _ => ObjectKind::Table,
    }
}

pub(crate) async fn list_objects(session: &PgSession, schema: &SchemaRef) -> Result<Vec<DbObject>, DbError> {
    let object = session.free().await?;
    let rows = client(&object)
        .query(
            "SELECT c.relname::text, c.relkind::text FROM pg_catalog.pg_class c
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m','f') ORDER BY 1",
            &[&schema.name],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            let kind: String = r.try_get(1).map_err(map_error)?;
            Ok(DbObject {
                name: r.try_get(0).map_err(map_error)?,
                schema: schema.name.clone(),
                kind: object_kind(&kind),
            })
        })
        .collect()
}

pub(crate) async fn columns(session: &PgSession, schema: &SchemaRef) -> Result<Vec<ColumnInfo>, DbError> {
    let object = session.free().await?;
    let sql = format!("{COLUMNS_SQL} ORDER BY c.relname, a.attnum");
    let rows = client(&object)
        .query(&sql, &[&schema.name])
        .await
        .map_err(map_error)?;
    rows.iter().map(column_from_row).collect()
}

async fn indexes(client: &Client, table: &TableRef) -> Result<Vec<IndexInfo>, DbError> {
    let rows = client
        .query(
            "SELECT ic.relname::text, i.indisunique, i.indisprimary, pg_catalog.pg_get_indexdef(i.indexrelid),
                    ARRAY(SELECT a.attname::text FROM unnest(i.indkey) WITH ORDINALITY k(attnum, ord)
                          JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum
                          ORDER BY k.ord)
             FROM pg_catalog.pg_index i
             JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2 ORDER BY 1",
            &[&table.schema, &table.name],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            Ok(IndexInfo {
                name: r.try_get(0).map_err(map_error)?,
                unique: r.try_get(1).map_err(map_error)?,
                primary: r.try_get(2).map_err(map_error)?,
                definition: r.try_get(3).map_err(map_error)?,
                columns: r.try_get(4).map_err(map_error)?,
            })
        })
        .collect()
}

async fn foreign_keys(client: &Client, table: &TableRef) -> Result<Vec<ForeignKeyInfo>, DbError> {
    let rows = client
        .query(
            "SELECT k.conname::text, fn.nspname::text, fc.relname::text,
                    ARRAY(SELECT a.attname::text FROM unnest(k.conkey) WITH ORDINALITY x(n, ord)
                          JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = x.n
                          ORDER BY x.ord),
                    ARRAY(SELECT a.attname::text FROM unnest(k.confkey) WITH ORDINALITY x(n, ord)
                          JOIN pg_catalog.pg_attribute a ON a.attrelid = k.confrelid AND a.attnum = x.n
                          ORDER BY x.ord),
                    pg_catalog.pg_get_constraintdef(k.oid, true)
             FROM pg_catalog.pg_constraint k
             JOIN pg_catalog.pg_class c ON c.oid = k.conrelid
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_catalog.pg_class fc ON fc.oid = k.confrelid
             JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace
             WHERE k.contype = 'f' AND n.nspname = $1 AND c.relname = $2 ORDER BY 1",
            &[&table.schema, &table.name],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            Ok(ForeignKeyInfo {
                name: r.try_get(0).map_err(map_error)?,
                referenced_schema: r.try_get(1).map_err(map_error)?,
                referenced_table: r.try_get(2).map_err(map_error)?,
                columns: r.try_get(3).map_err(map_error)?,
                referenced_columns: r.try_get(4).map_err(map_error)?,
                definition: r.try_get(5).map_err(map_error)?,
            })
        })
        .collect()
}

fn constraint_kind(contype: &str) -> &'static str {
    match contype {
        "c" => "CHECK",
        "u" => "UNIQUE",
        "p" => "PRIMARY KEY",
        "x" => "EXCLUDE",
        _ => "OTHER",
    }
}

async fn constraints(client: &Client, table: &TableRef) -> Result<Vec<ConstraintInfo>, DbError> {
    let rows = client
        .query(
            "SELECT k.conname::text, k.contype::text, pg_catalog.pg_get_constraintdef(k.oid)
             FROM pg_catalog.pg_constraint k
             JOIN pg_catalog.pg_class c ON c.oid = k.conrelid
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE k.contype <> 'f' AND n.nspname = $1 AND c.relname = $2 ORDER BY 1",
            &[&table.schema, &table.name],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            let kind: String = r.try_get(1).map_err(map_error)?;
            Ok(ConstraintInfo {
                name: r.try_get(0).map_err(map_error)?,
                kind: constraint_kind(&kind).to_owned(),
                definition: r.try_get(2).map_err(map_error)?,
            })
        })
        .collect()
}

/// Rebuilds approximate DDL for a table; PostgreSQL has no `SHOW CREATE TABLE`.
fn table_ddl(table: &TableRef, columns: &[ColumnInfo], constraints: &[ConstraintInfo]) -> String {
    let mut lines: Vec<String> = columns
        .iter()
        .map(|c| {
            let mut line = format!(
                "  {} {}",
                studio_db::sql::quote_ident(Engine::Postgres, &c.name),
                c.data_type
            );
            if let Some(default) = &c.default_value {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    lines.extend(constraints.iter().filter_map(|c| {
        c.definition.as_ref().map(|d| {
            format!(
                "  CONSTRAINT {} {d}",
                studio_db::sql::quote_ident(Engine::Postgres, &c.name)
            )
        })
    }));
    format!(
        "CREATE TABLE {} (\n{}\n);",
        qualified_ref(Engine::Postgres, table),
        lines.join(",\n")
    )
}

pub(crate) async fn table_structure(
    session: &PgSession,
    table: &TableRef,
) -> Result<TableStructure, DbError> {
    let object = session.free().await?;
    let client = client(&object);
    let row = client
        .query_opt(
            "SELECT c.relkind::text, pg_catalog.pg_get_userbyid(c.relowner)::text,
                    pg_catalog.pg_total_relation_size(c.oid), c.reltuples::int8,
                    pg_catalog.obj_description(c.oid, 'pg_class'),
                    CASE WHEN c.relkind IN ('v','m') THEN pg_catalog.pg_get_viewdef(c.oid, true) END
             FROM pg_catalog.pg_class c
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
            &[&table.schema, &table.name],
        )
        .await
        .map_err(map_error)?
        .ok_or_else(|| DbError::query("the table does not exist"))?;
    let relkind: String = row.try_get(0).map_err(map_error)?;
    let view_def: Option<String> = row.try_get(5).map_err(map_error)?;
    let estimate: i64 = row.try_get(3).map_err(map_error)?;
    let metadata = TableMetadata {
        owner: row.try_get(1).map_err(map_error)?,
        size_bytes: row.try_get(2).map_err(map_error)?,
        row_estimate: (estimate >= 0).then_some(estimate),
        comment: row.try_get(4).map_err(map_error)?,
    };

    let sql = format!("{COLUMNS_SQL} AND c.relname = $2 ORDER BY a.attnum");
    let rows = client
        .query(&sql, &[&table.schema, &table.name])
        .await
        .map_err(map_error)?;
    let columns = rows.iter().map(column_from_row).collect::<Result<Vec<_>, _>>()?;
    let constraints = constraints(client, table).await?;
    let kind = object_kind(&relkind);
    let ddl = match view_def {
        Some(definition) => {
            let keyword = if kind == ObjectKind::View {
                "VIEW"
            } else {
                "MATERIALIZED VIEW"
            };
            format!(
                "CREATE {keyword} {} AS\n{definition}",
                qualified_ref(Engine::Postgres, table)
            )
        }
        None => table_ddl(table, &columns, &constraints),
    };
    Ok(TableStructure {
        table: table.clone(),
        kind,
        indexes: indexes(client, table).await?,
        foreign_keys: foreign_keys(client, table).await?,
        columns,
        constraints,
        metadata,
        ddl: Some(ddl),
    })
}

/// Summarises one DER certificate for display.
pub(crate) fn describe_certificate(der: &[u8]) -> Option<CertificateInfo> {
    let (_, certificate) = X509Certificate::from_der(der).ok()?;
    Some(CertificateInfo {
        subject: certificate.subject().to_string(),
        issuer: certificate.issuer().to_string(),
        not_before: certificate.validity().not_before.to_string(),
        not_after: certificate.validity().not_after.to_string(),
        fingerprint_sha256: hex::encode(Sha256::digest(der)),
    })
}
