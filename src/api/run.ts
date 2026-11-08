import { Channel } from '@tauri-apps/api/core';
import { commands } from '../bindings';
import type { Environment, Origin, RunEvent, SessionId, StatementOutcome, StatementVerdict } from '../bindings';
import { ResultStore } from '../lib/resultStore';
import { api, unwrap } from './index';

/** What the confirm dialog shows. Verdicts come from Rust; the frontend never decides safety. */
export interface ConfirmRequest {
  sql: string;
  verdicts: StatementVerdict[];
  connectionName: string;
  environment: Environment;
  warnings: string[];
}

export type Confirm = (request: ConfirmRequest) => Promise<boolean>;

export interface RunTarget {
  session: SessionId;
  connectionName: string;
  environment: Environment;
}

export type RunResult =
  | { status: 'declined' }
  | { status: 'finished'; outcomes: StatementOutcome[]; verdicts: StatementVerdict[] };

/** Applies one channel message to the store. */
export function applyRunEvent(store: ResultStore, event: RunEvent): void {
  switch (event.event) {
    case 'statementStart':
      store.statement(event.index);
      break;
    case 'columns':
      store.statement(event.index).columns = event.columns;
      break;
    case 'rows':
      store.statement(event.index).append(event.batch);
      break;
    case 'statementDone': {
      const statement = store.statement(event.index);
      statement.outcome = event.outcome;
      if (event.outcome.kind !== 'error') statement.status = 'done';
      else statement.status = event.outcome.error.code === 'Cancelled' ? 'cancelled' : 'error';
      break;
    }
    case 'finished':
      break;
  }
  store.notify();
}

/**
 * prepare, then (for anything that is not a read) confirm and approve, then run.
 * Reads never open the dialog, writes always do, and declining never calls approve.
 */
export async function runFlow(options: {
  target: RunTarget;
  sql: string;
  origin: Origin;
  confirm: Confirm;
  runId: string;
  store: ResultStore;
  warnings?: string[];
}): Promise<RunResult> {
  const { target, sql, origin, confirm, runId, store, warnings = [] } = options;
  const verdicts = await api.prepareQuery(target.session, sql);
  let token: string | null = null;
  if (verdicts.some((verdict) => !verdict.readOnly)) {
    const confirmed = await confirm({
      sql,
      verdicts,
      connectionName: target.connectionName,
      environment: target.environment,
      warnings,
    });
    if (!confirmed) return { status: 'declined' };
    token = (await api.approveQuery(target.session, sql, origin)).token;
  }
  const channel = new Channel<RunEvent>();
  channel.onmessage = (event) => applyRunEvent(store, event);
  const outcomes = await unwrap(commands.runQuery(target.session, sql, token, runId, channel));
  return { status: 'finished', outcomes, verdicts };
}

export type ChangesResult = { status: 'declined' } | { status: 'saved'; saved: number };

/**
 * The grid's save: Rust planned `sql`; the user reviews it, then it runs all or nothing through
 * the grid's own door. Grid tokens do not open `run_query`.
 */
export async function runChanges(options: {
  target: RunTarget;
  sql: string;
  confirm: Confirm;
  runId: string;
  warnings?: string[];
}): Promise<ChangesResult> {
  const { target, sql, confirm, runId, warnings = [] } = options;
  const verdicts = await api.prepareQuery(target.session, sql);
  const confirmed = await confirm({
    sql,
    verdicts,
    connectionName: target.connectionName,
    environment: target.environment,
    warnings,
  });
  if (!confirmed) return { status: 'declined' };
  const { token } = await api.approveQuery(target.session, sql, 'Grid');
  const saved = await api.runRowChanges(target.session, sql, token, runId);
  return { status: 'saved', saved };
}
