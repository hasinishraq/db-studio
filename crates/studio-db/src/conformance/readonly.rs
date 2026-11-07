//! The database itself refuses writes in a read-only transaction.

use super::{collect, Scratch};
use crate::{DbError, ExecOptions, Session};
use studio_types::Value;

pub(crate) async fn check(session: &dyn Session, scratch: &Scratch) {
    let table = scratch.qualified();
    let read_only = ExecOptions {
        read_only_tx: true,
        ..Default::default()
    };

    let sql = format!("INSERT INTO {table} (id, name, score) VALUES (99, 'x', 0)");
    let result = collect(session, &sql, read_only).await;
    assert!(
        matches!(
            result,
            Err(DbError::ReadOnlyViolation) | Err(DbError::Query { .. })
        ),
        "a read-only transaction must reject INSERT, got {:?}",
        result.as_ref().map(|c| c.summary.clone())
    );

    let count = format!("SELECT COUNT(*) FROM {table} WHERE id = 99");
    let out = collect(session, &count, ExecOptions::default())
        .await
        .expect("count");
    assert!(
        matches!(&out.rows[0][0], Value::Int(0)) || out.rows[0][0] == Value::BigInt("0".into()),
        "the refused write must leave nothing behind: {:?}",
        out.rows[0][0]
    );

    // Reads still work, and the connection is usable afterwards.
    let out = collect(session, &format!("SELECT * FROM {table}"), read_only)
        .await
        .expect("read");
    assert!(out.summary.returns_rows);
}
