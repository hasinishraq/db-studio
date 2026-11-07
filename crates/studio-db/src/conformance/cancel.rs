//! Timeouts fire, and cancel stops a long query quickly.

use super::{long_query, Scratch};
use crate::{ConnScope, DbError, ExecOptions, Session};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub(crate) async fn check(session: &dyn Session, scratch: &Scratch) {
    timeout_fires(session, scratch).await;
    cancel_stops_within_two_seconds(session, scratch).await;
    session_survives(session).await;
}

async fn timeout_fires(session: &dyn Session, scratch: &Scratch) {
    let opts = ExecOptions {
        timeout: Some(Duration::from_millis(300)),
        ..Default::default()
    };
    let (sink, mut inbox) = mpsc::channel(8);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    let started = Instant::now();
    let result = session.execute(long_query(scratch.engine), opts, sink).await;
    drain.await.expect("drain");
    assert!(
        matches!(result, Err(DbError::Timeout)),
        "expected Timeout, got {result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "timeout took {:?}",
        started.elapsed()
    );
}

async fn cancel_stops_within_two_seconds(session: &dyn Session, scratch: &Scratch) {
    let mut conn = session.acquire(ConnScope::Isolated).await.expect("acquire");
    let canceller = conn.canceller();
    let (sink, mut inbox) = mpsc::channel(8);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let asked = Instant::now();
        canceller.cancel().await.expect("cancel");
        asked
    });
    let result = conn
        .execute(long_query(scratch.engine), ExecOptions::default(), sink)
        .await;
    let finished = Instant::now();
    let asked = cancel.await.expect("cancel task");
    drain.await.expect("drain");
    assert!(
        matches!(result, Err(DbError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );
    assert!(
        finished.duration_since(asked) < Duration::from_secs(2),
        "cancel was too slow"
    );
}

async fn session_survives(session: &dyn Session) {
    let out = super::collect(session, "SELECT 1", ExecOptions::default())
        .await
        .expect("after cancel");
    assert_eq!(
        out.rows.len(),
        1,
        "the session must stay usable after cancel and timeout"
    );
}
