//! Streaming, row caps and type round trips.

use super::{collect, run, ten_thousand_rows, Scratch};
use crate::{ExecOptions, Session};
use studio_types::{Engine, Value};

pub(crate) async fn check(session: &dyn Session, scratch: &Scratch) {
    streams_in_order(session).await;
    max_rows_truncates(session).await;
    commands_report_rows_affected(session, scratch).await;
    types_round_trip(session, scratch).await;
    errors_are_query_errors(session).await;
}

fn as_i64(value: &Value) -> i64 {
    match value {
        Value::Int(v) => i64::from(*v),
        Value::BigInt(v) | Value::Decimal(v) => v.parse().expect("integer text"),
        other => panic!("expected an integer, got {other:?}"),
    }
}

async fn streams_in_order(session: &dyn Session) {
    let opts = ExecOptions {
        batch_size: 500,
        ..Default::default()
    };
    let out = collect(session, &ten_thousand_rows(), opts)
        .await
        .expect("stream");
    assert_eq!(out.rows.len(), 10_000);
    assert_eq!(out.batches, 20, "10 000 rows must arrive as 20 batches of 500");
    assert_eq!(out.summary.total_rows, 10_000);
    assert!(!out.summary.truncated);
    assert_eq!(out.columns.len(), 1);
    assert_eq!(out.columns[0].name, "v");
    for (i, row) in out.rows.iter().enumerate() {
        assert_eq!(as_i64(&row[0]), i as i64, "row {i} out of order");
    }
}

async fn max_rows_truncates(session: &dyn Session) {
    let opts = ExecOptions {
        max_rows: Some(1200),
        batch_size: 500,
        ..Default::default()
    };
    let out = collect(session, &ten_thousand_rows(), opts)
        .await
        .expect("capped");
    assert_eq!(out.rows.len(), 1200);
    assert_eq!(out.summary.total_rows, 1200);
    assert!(out.summary.truncated, "a capped result must report truncation");

    let opts = ExecOptions {
        max_rows: Some(10_000),
        ..Default::default()
    };
    let out = collect(session, &ten_thousand_rows(), opts).await.expect("exact");
    assert_eq!(out.rows.len(), 10_000);
    assert!(!out.summary.truncated, "exactly the cap is not truncated");
}

async fn commands_report_rows_affected(session: &dyn Session, scratch: &Scratch) {
    let table = scratch.qualified();
    let sql = format!("INSERT INTO {table} (id, name, score) VALUES (1, 'a', 10), (2, 'b', 20)");
    let out = collect(session, &sql, ExecOptions::default())
        .await
        .expect("insert");
    assert!(!out.summary.returns_rows);
    assert_eq!(out.summary.rows_affected, 2);
    run(session, &format!("DELETE FROM {table}")).await;
}

async fn types_round_trip(session: &dyn Session, scratch: &Scratch) {
    let name = format!("{}_types", scratch.table);
    let table = crate::sql::qualified(scratch.engine, &scratch.schema, &name);
    let (ddl, insert) = match scratch.engine {
        Engine::Postgres => (
            "(i INTEGER, b BIGINT, n NUMERIC(12,3), t TEXT, f BOOLEAN, j JSONB, d DATE, ts TIMESTAMP, \
             y BYTEA, u UUID, z TEXT)",
            "(2147483647, 9007199254740993, 1234.500, 'héllo', TRUE, '{\"a\":1}', '2026-11-07', \
             '2026-11-07 12:30:00', '\\x01ff', '6f1c0c8e-3a7e-4b0e-9d6a-3f0f7a2c1b11', NULL)",
        ),
        Engine::MySql => (
            "(i INT, b BIGINT, n DECIMAL(12,3), t TEXT, f BOOLEAN, j JSON, d DATE, ts DATETIME, \
             y BLOB, u CHAR(36), z TEXT)",
            "(2147483647, 9007199254740993, 1234.500, 'héllo', TRUE, '{\"a\":1}', '2026-11-07', \
             '2026-11-07 12:30:00', x'01ff', '6f1c0c8e-3a7e-4b0e-9d6a-3f0f7a2c1b11', NULL)",
        ),
        Engine::Sqlite => (
            "(i INTEGER, b BIGINT, n NUMERIC, t TEXT, f BOOLEAN, j JSON, d DATE, ts DATETIME, \
             y BLOB, u TEXT, z TEXT)",
            "(2147483647, 9007199254740993, 1234.5, 'héllo', 1, '{\"a\":1}', '2026-11-07', \
             '2026-11-07 12:30:00', x'01ff', '6f1c0c8e-3a7e-4b0e-9d6a-3f0f7a2c1b11', NULL)",
        ),
    };
    run(session, &format!("CREATE TABLE {table} {ddl}")).await;
    run(session, &format!("INSERT INTO {table} VALUES {insert}")).await;
    let out = collect(session, &format!("SELECT * FROM {table}"), ExecOptions::default())
        .await
        .expect("types");
    let row = &out.rows[0];

    assert_eq!(as_i64(&row[0]), 2_147_483_647);
    assert_eq!(
        row[1],
        Value::BigInt("9007199254740993".into()),
        "bigint must stay exact"
    );
    match &row[2] {
        Value::Decimal(v) => assert!(v.starts_with("1234.5"), "{v}"),
        Value::Float(v) => assert!(scratch.engine == Engine::Sqlite && (*v - 1234.5).abs() < 1e-9),
        other => panic!("numeric decoded as {other:?}"),
    }
    assert_eq!(row[3], Value::Text("héllo".into()));
    match &row[4] {
        Value::Bool(v) => assert!(*v),
        other => assert_eq!(as_i64(other), 1),
    }
    match &row[5] {
        Value::Json(v) | Value::Text(v) => assert!(v.contains("\"a\""), "{v}"),
        other => panic!("json decoded as {other:?}"),
    }
    for (index, want) in [(6, "2026-11-07"), (7, "2026-11-07")] {
        match &row[index] {
            Value::Temporal(v) | Value::Text(v) => assert!(v.starts_with(want), "{v}"),
            other => panic!("temporal decoded as {other:?}"),
        }
    }
    let Value::Bytes { len, preview_hex } = &row[8] else {
        panic!("blob decoded as {:?}", row[8])
    };
    assert_eq!((*len, preview_hex.as_str()), (2, "01ff"));
    match &row[9] {
        Value::Text(v) => assert_eq!(v, "6f1c0c8e-3a7e-4b0e-9d6a-3f0f7a2c1b11"),
        other => panic!("uuid decoded as {other:?}"),
    }
    assert_eq!(row[10], Value::Null);
    run(session, &format!("DROP TABLE {table}")).await;
}

async fn errors_are_query_errors(session: &dyn Session) {
    let result = collect(
        session,
        "SELECT * FROM studio_no_such_table",
        ExecOptions::default(),
    )
    .await;
    assert!(
        matches!(result, Err(crate::DbError::Query { .. })),
        "got {:?}",
        result.err()
    );
}
