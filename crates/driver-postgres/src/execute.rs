//! Running one statement on one connection.

use crate::connect::{client, map_error};
use crate::decode::decode;
use crate::tls::PgTls;
use deadpool_postgres::Object;
use futures::{pin_mut, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use studio_db::{Canceller, Conn, DbError, ExecOptions, RowEvent, RowSink};
use studio_types::{ColumnMeta, ExecSummary, RowBatch, Value};
use tokio::sync::OwnedMutexGuard;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::Type;
use tokio_postgres::{CancelToken, Client, SimpleQueryMessage};

enum Holder {
    Pooled(Box<Object>),
    Pinned(OwnedMutexGuard<Option<Object>>),
}

/// Cancels the statement running on one connection.
pub(crate) struct PgCanceller {
    token: CancelToken,
    tls: Arc<PgTls>,
    requested: AtomicBool,
}

#[async_trait::async_trait]
impl Canceller for PgCanceller {
    async fn cancel(&self) -> Result<(), DbError> {
        self.requested.store(true, Ordering::SeqCst);
        PgCanceller::send(&self.token, &self.tls).await.map_err(map_error)
    }
}

/// One connection.
pub(crate) struct PgConn {
    holder: Holder,
    canceller: Arc<PgCanceller>,
}

impl PgConn {
    fn boxed(holder: Holder, token: CancelToken, tls: Arc<PgTls>) -> Box<dyn Conn> {
        let canceller = Arc::new(PgCanceller {
            token,
            tls,
            requested: AtomicBool::new(false),
        });
        Box::new(Self { holder, canceller })
    }

    pub(crate) fn pooled(object: Object, tls: Arc<PgTls>) -> Box<dyn Conn> {
        let token = client(&object).cancel_token();
        Self::boxed(Holder::Pooled(Box::new(object)), token, tls)
    }

    pub(crate) fn pinned(
        guard: OwnedMutexGuard<Option<Object>>,
        tls: Arc<PgTls>,
    ) -> Result<Box<dyn Conn>, DbError> {
        let token = guard.as_ref().map(|object| client(object).cancel_token());
        let token = token.ok_or_else(|| DbError::Internal("the pinned connection is gone".to_owned()))?;
        Ok(Self::boxed(Holder::Pinned(guard), token, tls))
    }

    fn client(&self) -> Result<&Client, DbError> {
        match &self.holder {
            Holder::Pooled(object) => Ok(client(object)),
            Holder::Pinned(guard) => guard
                .as_ref()
                .map(client)
                .ok_or_else(|| DbError::Internal("the pinned connection is gone".to_owned())),
        }
    }
}

/// Reports why a statement stopped. A server-side cancel looks the same for a user's Stop and for
/// `statement_timeout`, so the request flag tells them apart.
fn stopped(error: tokio_postgres::Error, canceller: &PgCanceller, timed: bool) -> DbError {
    if error.code() == Some(&SqlState::QUERY_CANCELED) {
        if canceller.requested.load(Ordering::SeqCst) {
            return DbError::Cancelled;
        }
        if timed {
            return DbError::Timeout;
        }
    }
    map_error(error)
}

fn millis(timeout: Duration) -> u128 {
    timeout.as_millis().max(1)
}

async fn send(sink: &RowSink, rows: Vec<Vec<Value>>, seq: &mut u32) -> bool {
    let batch = RowBatch { seq: *seq, rows };
    *seq += 1;
    sink.send(RowEvent::Batch(batch)).await.is_ok()
}

/// Streams one statement. Stops early at the row cap or if the reader goes away.
async fn stream_rows(
    client: &Client,
    stmt: &str,
    types: &[Type],
    opts: ExecOptions,
    sink: &RowSink,
    canceller: &PgCanceller,
    timed: bool,
) -> Result<ExecSummary, DbError> {
    let mut summary = ExecSummary::default();
    let stream = client
        .simple_query_raw(stmt)
        .await
        .map_err(|e| stopped(e, canceller, timed))?;
    pin_mut!(stream);
    let batch_size = opts.batch_size.max(1);
    let mut rows = Vec::with_capacity(batch_size);
    let mut seq = 0;
    let mut abandoned = false;
    while let Some(message) = stream.next().await {
        match message.map_err(|e| stopped(e, canceller, timed))? {
            SimpleQueryMessage::Row(row) => {
                if opts.max_rows.is_some_and(|cap| summary.total_rows >= cap) {
                    summary.truncated = true;
                    break;
                }
                let cells = (0..row.len())
                    .map(|i| decode(row.get(i), types.get(i).unwrap_or(&Type::TEXT)))
                    .collect();
                rows.push(cells);
                summary.total_rows += 1;
                if rows.len() >= batch_size {
                    let full = std::mem::replace(&mut rows, Vec::with_capacity(batch_size));
                    if !send(sink, full, &mut seq).await {
                        abandoned = true;
                        break;
                    }
                }
            }
            SimpleQueryMessage::CommandComplete(count) => summary.rows_affected = count,
            _ => {}
        }
    }
    if summary.truncated || abandoned {
        // The server keeps sending until told otherwise. Stop it and wait until the connection
        // is idle, so this cancel cannot land on the next statement.
        let _ = PgCanceller::send(&canceller.token, &canceller.tls).await;
        while stream.next().await.is_some() {}
    }
    if abandoned {
        return Err(DbError::Cancelled);
    }
    if !rows.is_empty() && !send(sink, rows, &mut seq).await {
        return Err(DbError::Cancelled);
    }
    Ok(summary)
}

#[async_trait::async_trait]
impl Conn for PgConn {
    async fn execute(
        &mut self,
        stmt: &str,
        opts: ExecOptions,
        sink: RowSink,
    ) -> Result<ExecSummary, DbError> {
        let canceller = self.canceller.clone();
        if canceller.requested.load(Ordering::SeqCst) {
            return Err(DbError::Cancelled);
        }
        let client = self.client()?;
        let started = Instant::now();
        // Preparing gives the column types, and the server refuses to prepare more than one command,
        // so nothing can ride along with the statement we classified.
        let prepared = client.prepare(stmt).await.map_err(map_error)?;
        let types: Vec<Type> = prepared.columns().iter().map(|c| c.type_().clone()).collect();
        let columns: Vec<ColumnMeta> = prepared
            .columns()
            .iter()
            .map(|c| ColumnMeta {
                name: c.name().to_owned(),
                type_name: c.type_().name().to_owned(),
                nullable: None,
            })
            .collect();
        if !columns.is_empty() {
            let _ = sink.send(RowEvent::Columns(columns.clone())).await;
        }

        if opts.read_only_tx {
            client.batch_execute("BEGIN READ ONLY").await.map_err(map_error)?;
        }
        let timed = opts.timeout.is_some();
        let set_timeout = match opts.timeout {
            Some(timeout) if opts.read_only_tx => {
                client
                    .batch_execute(&format!("SET LOCAL statement_timeout = {}", millis(timeout)))
                    .await
            }
            Some(timeout) => {
                client
                    .batch_execute(&format!("SET statement_timeout = {}", millis(timeout)))
                    .await
            }
            None => Ok(()),
        };
        let result = match set_timeout {
            Ok(()) => stream_rows(client, stmt, &types, opts, &sink, &canceller, timed).await,
            Err(error) => Err(map_error(error)),
        };

        // Runs on every path, including errors and cancel.
        if opts.read_only_tx {
            client.batch_execute("ROLLBACK").await.map_err(map_error)?;
        } else if timed {
            // Fails harmlessly inside an aborted manual transaction; recycling resets it.
            let _ = client.batch_execute("SET statement_timeout = 0").await;
        }
        let mut summary = result?;
        summary.returns_rows = !columns.is_empty();
        summary.columns = columns;
        summary.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(summary)
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }
}
