//! Paging, counting and EXPLAIN.

use crate::connect::{client, map_error, PgSession};
use crate::decode::decode;
use std::collections::HashMap;
use std::time::Duration;
use studio_db::sql::{order_by, qualified_ref, quote_ident, FilterBuilder};
use studio_db::DbError;
use studio_types::{ColumnMeta, Engine, FilterRule, PageRequest, TablePage, TableRef};
use tokio_postgres::error::SqlState;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{Client, SimpleQueryMessage};

const PG: Engine = Engine::Postgres;

/// The columns of a table with their types, from the server itself.
async fn table_columns(client: &Client, table: &TableRef) -> Result<Vec<(String, Type)>, DbError> {
    let sql = format!("SELECT * FROM {} LIMIT 0", qualified_ref(PG, table));
    let prepared = client.prepare(&sql).await.map_err(map_error)?;
    Ok(prepared
        .columns()
        .iter()
        .map(|c| (c.name().to_owned(), c.type_().clone()))
        .collect())
}

/// Column types for casting a text parameter, quoted so a custom type name is safe.
fn casts(columns: &[(String, Type)]) -> HashMap<String, String> {
    columns
        .iter()
        .map(|(name, ty)| {
            let cast = format!("{}.{}", quote_ident(PG, ty.schema()), quote_ident(PG, ty.name()));
            (name.clone(), cast)
        })
        .collect()
}

fn bound(params: &[String]) -> Vec<&(dyn ToSql + Sync)> {
    params.iter().map(|p| p as &(dyn ToSql + Sync)).collect()
}

/// Reads one page. Columns are selected as text so values keep the server's own form, as in
/// [`crate::execute`], while filter values stay bound parameters.
pub(crate) async fn table_page(
    session: &PgSession,
    table: &TableRef,
    page: &PageRequest,
) -> Result<TablePage, DbError> {
    let object = session.free().await?;
    let client = client(&object);
    let columns = table_columns(client, table).await?;
    let casts = casts(&columns);
    let filter = FilterBuilder::new(PG).with_casts(&casts).build(&page.filters);
    let select: Vec<String> = columns
        .iter()
        .map(|(n, _)| format!("{}::text", quote_ident(PG, n)))
        .collect();
    let select = if select.is_empty() {
        "NULL::text".to_owned()
    } else {
        select.join(", ")
    };
    let sql = format!(
        "SELECT {select} FROM {}{}{} LIMIT {} OFFSET {}",
        qualified_ref(PG, table),
        filter.sql,
        order_by(PG, &page.sort),
        page.limit,
        page.offset
    );
    let rows = client
        .query(&sql, &bound(&filter.params))
        .await
        .map_err(map_error)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut cells = Vec::with_capacity(columns.len());
        for (index, (_, ty)) in columns.iter().enumerate() {
            let text: Option<&str> = row.try_get(index).map_err(map_error)?;
            cells.push(decode(text, ty));
        }
        out.push(cells);
    }
    let columns = columns
        .into_iter()
        .map(|(name, ty)| ColumnMeta {
            name,
            type_name: ty.name().to_owned(),
            nullable: None,
        })
        .collect();
    Ok(TablePage { columns, rows: out })
}

/// Counts rows in a read-only transaction, returning `None` when it takes too long.
pub(crate) async fn table_count(
    session: &PgSession,
    table: &TableRef,
    filters: &[FilterRule],
    timeout: Duration,
) -> Result<Option<u64>, DbError> {
    let object = session.free().await?;
    let client = client(&object);
    let columns = table_columns(client, table).await?;
    let casts = casts(&columns);
    let filter = FilterBuilder::new(PG).with_casts(&casts).build(filters);
    let sql = format!("SELECT COUNT(*) FROM {}{}", qualified_ref(PG, table), filter.sql);
    let begin = format!(
        "BEGIN READ ONLY; SET LOCAL statement_timeout = {}",
        timeout.as_millis().max(1)
    );
    client.batch_execute(&begin).await.map_err(map_error)?;
    let result = client.query_one(&sql, &bound(&filter.params)).await;
    client.batch_execute("ROLLBACK").await.map_err(map_error)?;
    match result {
        Ok(row) => {
            let count: i64 = row.try_get(0).map_err(map_error)?;
            Ok(Some(u64::try_from(count).unwrap_or(0)))
        }
        Err(error) if error.code() == Some(&SqlState::QUERY_CANCELED) => Ok(None),
        Err(error) => Err(map_error(error)),
    }
}

/// Returns the JSON plan. Plain EXPLAIN never executes the statement, and preparing first refuses
/// anything that smuggles in a second command.
pub(crate) async fn explain(session: &PgSession, stmt: &str) -> Result<String, DbError> {
    let object = session.free().await?;
    let client = client(&object);
    let sql = format!("EXPLAIN (FORMAT JSON) {stmt}");
    client.prepare(&sql).await.map_err(map_error)?;
    let messages = client.simple_query(&sql).await.map_err(map_error)?;
    let plan: Vec<&str> = messages
        .iter()
        .filter_map(|m| match m {
            SimpleQueryMessage::Row(row) => row.get(0),
            _ => None,
        })
        .collect();
    Ok(plan.join("\n"))
}
