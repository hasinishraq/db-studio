//! Paging, counting and EXPLAIN.

use crate::connect::{code, map_error, MySession};
use crate::decode::{decode, type_name};
use mysql_async::prelude::Queryable;
use mysql_async::{Column, Params, Row, Value as Wire};
use std::sync::Arc;
use std::time::Duration;
use studio_db::sql::{order_by, qualified_ref, FilterBuilder};
use studio_db::DbError;
use studio_types::{ColumnMeta, Engine, FilterRule, PageRequest, TablePage, TableRef};

const MY: Engine = Engine::MySql;

fn params(values: Vec<String>) -> Params {
    if values.is_empty() {
        Params::Empty
    } else {
        Params::Positional(values.into_iter().map(Wire::from).collect())
    }
}

/// Reads one page. Filters stay bound; `LIMIT` is inlined because not every server accepts it
/// bound, and both numbers are integers by type.
pub(crate) async fn table_page(
    session: &MySession,
    table: &TableRef,
    page: &PageRequest,
) -> Result<TablePage, DbError> {
    let filter = FilterBuilder::new(MY).build(&page.filters);
    let sql = format!(
        "SELECT * FROM {}{}{} LIMIT {} OFFSET {}",
        qualified_ref(MY, table),
        filter.sql,
        order_by(MY, &page.sort),
        page.limit,
        page.offset
    );
    let mut conn = session.shared.get().await?;
    let mut result = conn
        .exec_iter(sql, params(filter.params))
        .await
        .map_err(map_error)?;
    let columns: Arc<[Column]> = result.columns().unwrap_or_else(|| Arc::from(Vec::new()));
    let rows: Vec<Row> = result.collect().await.map_err(map_error)?;
    let rows = rows
        .iter()
        .map(|row| {
            (0..row.len())
                .map(|i| match (row.as_ref(i), columns.get(i)) {
                    (Some(value), Some(column)) => decode(value, column),
                    _ => studio_db::Value::Null,
                })
                .collect()
        })
        .collect();
    let columns = columns
        .iter()
        .map(|c| ColumnMeta {
            name: c.name_str().into_owned(),
            type_name: type_name(c),
            nullable: None,
        })
        .collect();
    Ok(TablePage { columns, rows })
}

/// Counts rows, returning `None` when it takes too long.
pub(crate) async fn table_count(
    session: &MySession,
    table: &TableRef,
    filters: &[FilterRule],
    timeout: Duration,
) -> Result<Option<u64>, DbError> {
    let filter = FilterBuilder::new(MY).build(filters);
    let target = format!("{}{}", qualified_ref(MY, table), filter.sql);
    let sql = if session.shared.mariadb {
        format!(
            "SET STATEMENT max_statement_time = {:.3} FOR SELECT COUNT(*) FROM {target}",
            timeout.as_secs_f64()
        )
    } else {
        format!(
            "SELECT /*+ MAX_EXECUTION_TIME({}) */ COUNT(*) FROM {target}",
            timeout.as_millis().max(1)
        )
    };
    let mut conn = session.shared.get().await?;
    match conn.exec_first::<u64, _, _>(sql, params(filter.params)).await {
        Ok(count) => Ok(count),
        Err(mysql_async::Error::Server(server))
            if matches!(
                server.code,
                code::MYSQL_EXECUTION_TIME_EXCEEDED
                    | code::MARIADB_STATEMENT_TIMEOUT
                    | code::QUERY_INTERRUPTED
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(map_error(error)),
    }
}

/// Returns the JSON plan without executing the statement.
pub(crate) async fn explain(session: &MySession, stmt: &str) -> Result<String, DbError> {
    let mut conn = session.shared.get().await?;
    let sql = format!("EXPLAIN FORMAT=JSON {stmt}");
    let prepared = conn.prep(&sql).await.map_err(map_error)?;
    conn.close(prepared).await.map_err(map_error)?;
    let plan: Option<String> = conn.query_first(sql).await.map_err(map_error)?;
    Ok(plan.unwrap_or_default())
}
