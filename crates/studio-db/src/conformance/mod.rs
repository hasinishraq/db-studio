//! The shared test suite every driver must pass. It creates and drops its own scratch objects.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cancel;
mod execute;
mod metadata;
mod page;
mod readonly;

use crate::{ConnectConfig, Driver, ExecOptions, RowEvent, Session};
use studio_types::{ColumnMeta, Engine, ExecSummary, SchemaRef, TableRef, Value};
use tokio::sync::mpsc;

/// One conformance run's scratch objects.
pub(crate) struct Scratch {
    pub engine: Engine,
    pub schema: String,
    pub table: String,
}

impl Scratch {
    pub fn table_ref(&self) -> TableRef {
        TableRef {
            schema: self.schema.clone(),
            name: self.table.clone(),
        }
    }

    pub fn schema_ref(&self) -> SchemaRef {
        SchemaRef {
            name: self.schema.clone(),
        }
    }

    /// The qualified scratch table.
    pub fn qualified(&self) -> String {
        crate::sql::qualified(self.engine, &self.schema, &self.table)
    }
}

/// Everything a statement produced.
pub(crate) struct Collected {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub batches: usize,
    pub summary: ExecSummary,
}

/// Runs a statement, collecting its stream.
pub(crate) async fn collect(
    session: &dyn Session,
    sql: &str,
    opts: ExecOptions,
) -> Result<Collected, crate::DbError> {
    let (sink, mut inbox) = mpsc::channel(8);
    let reader = tokio::spawn(async move {
        let (mut columns, mut rows, mut batches) = (Vec::new(), Vec::new(), 0);
        let mut next_seq = 0;
        while let Some(event) = inbox.recv().await {
            match event {
                RowEvent::Columns(c) => columns = c,
                RowEvent::Batch(batch) => {
                    assert_eq!(batch.seq, next_seq, "batches must arrive in order");
                    next_seq += 1;
                    batches += 1;
                    rows.extend(batch.rows);
                }
            }
        }
        (columns, rows, batches)
    });
    let summary = session.execute(sql, opts, sink).await;
    let (columns, rows, batches) = reader.await.expect("reader task");
    Ok(Collected {
        columns,
        rows,
        batches,
        summary: summary?,
    })
}

/// Runs a statement that must succeed.
pub(crate) async fn run(session: &dyn Session, sql: &str) {
    if let Err(error) = collect(session, sql, ExecOptions::default()).await {
        panic!("{sql} failed: {error}");
    }
}

/// A portable query producing `0..10000` in order.
pub(crate) fn ten_thousand_rows() -> String {
    let digit = "(SELECT 0 AS n UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL \
                 SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL \
                 SELECT 8 UNION ALL SELECT 9)";
    format!(
        "SELECT a.n + b.n * 10 + c.n * 100 + d.n * 1000 AS v FROM {digit} a, {digit} b, {digit} c, {digit} d \
         ORDER BY 1"
    )
}

/// A query that runs for far longer than any test allows.
pub(crate) fn long_query(engine: Engine) -> &'static str {
    match engine {
        Engine::Postgres => "SELECT pg_sleep(60)",
        Engine::MySql => {
            "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, \
             information_schema.columns c, information_schema.columns d"
        }
        Engine::Sqlite => {
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT COUNT(*) FROM c"
        }
    }
}

/// Runs every check against a live server. Panics on the first failure.
pub async fn run_all(driver: &dyn Driver, cfg: ConnectConfig) {
    let engine = driver.engine();
    let session = driver.connect(&cfg).await.expect("connect");
    let session = session.as_ref();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let schema = match engine {
        Engine::Postgres => format!("studio_conf_{}", &suffix[..12]),
        Engine::MySql => cfg.database.clone(),
        Engine::Sqlite => "main".to_owned(),
    };
    if engine == Engine::Postgres {
        run(session, &format!("CREATE SCHEMA \"{schema}\"")).await;
    }
    let scratch = Scratch {
        engine,
        schema,
        table: format!("conf_{}", &suffix[..12]),
    };
    run(
        session,
        &format!(
            "CREATE TABLE {} (id INTEGER PRIMARY KEY, name VARCHAR(100), score INTEGER)",
            scratch.qualified()
        ),
    )
    .await;

    metadata::check(session, &scratch).await;
    execute::check(session, &scratch).await;
    readonly::check(session, &scratch).await;
    page::check(session, &scratch).await;
    cancel::check(session, &scratch).await;

    run(session, &format!("DROP TABLE {}", scratch.qualified())).await;
    if engine == Engine::Postgres {
        run(session, &format!("DROP SCHEMA \"{}\" CASCADE", scratch.schema)).await;
    }
    session.close().await;
}
