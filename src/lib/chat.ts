import type { AiEvent, ChartSpec, ColumnMeta, Value } from '../bindings';

export type QueryStatus = 'review' | 'running' | 'done' | 'declined' | 'cancelled' | 'error';

/** A statement in the conversation. Rows live here, in memory only. */
export interface QueryPart {
  id: string;
  sql: string;
  status: QueryStatus;
  readOnly: boolean;
  reason: string | null;
  riskReasons: string[];
  columns: ColumnMeta[];
  rows: Value[][];
  rowsAffected: number | null;
  truncated: boolean;
  elapsedMs: number;
  error: string | null;
  /** The chart the assistant asked for, checked by Rust. Column names only. */
  chart: ChartSpec | null;
}

export type Part = { kind: 'text'; text: string } | { kind: 'query'; query: QueryPart };

/** One assistant reply being built from events. */
export interface TurnView {
  id: string;
  userText: string;
  parts: Part[];
  finished: boolean;
  error: string | null;
  /** A note between turns, such as a change of database. Set on notes only. */
  note?: string;
}

function updateQuery(turn: TurnView, id: string, change: (query: QueryPart) => QueryPart): TurnView {
  return {
    ...turn,
    parts: turn.parts.map((part) =>
      part.kind === 'query' && part.query.id === id ? { kind: 'query', query: change(part.query) } : part,
    ),
  };
}

/** Applies one event to the turn it belongs to. */
export function applyAiEvent(turn: TurnView, event: AiEvent): TurnView {
  switch (event.type) {
    case 'text': {
      const last = turn.parts[turn.parts.length - 1];
      if (last?.kind === 'text') {
        return { ...turn, parts: [...turn.parts.slice(0, -1), { kind: 'text', text: last.text + event.text }] };
      }
      return { ...turn, parts: [...turn.parts, { kind: 'text', text: event.text }] };
    }
    case 'queryStart':
      return {
        ...turn,
        parts: [
          ...turn.parts,
          {
            kind: 'query',
            query: {
              id: event.id,
              sql: event.sql,
              status: event.needsReview ? 'review' : 'running',
              readOnly: event.readOnly,
              reason: event.reason,
              riskReasons: event.riskReasons,
              columns: [],
              rows: [],
              rowsAffected: null,
              truncated: false,
              elapsedMs: 0,
              error: null,
              chart: null,
            },
          },
        ],
      };
    case 'queryApproved':
      return updateQuery(turn, event.id, (query) => ({ ...query, status: 'running' }));
    case 'queryDeclined':
      return updateQuery(turn, event.id, (query) => ({ ...query, status: 'declined' }));
    case 'queryCancelled':
      return updateQuery(turn, event.id, (query) => ({ ...query, status: 'cancelled' }));
    case 'queryResult':
      return updateQuery(turn, event.id, (query) => ({
        ...query,
        status: 'done',
        columns: event.columns,
        rows: event.rows,
        rowsAffected: event.rowsAffected,
        truncated: event.truncated,
        elapsedMs: event.elapsedMs,
        chart: event.chart,
      }));
    case 'queryError':
      return updateQuery(turn, event.id, (query) => ({ ...query, status: 'error', error: event.message }));
    case 'done':
      return { ...turn, finished: true, error: event.error };
  }
}

const STORAGE_KEY = 'studio.conversations.v1';

/** A turn as kept in browser storage: words and statements, never rows. */
export function withoutRows(turn: TurnView): TurnView {
  return {
    ...turn,
    parts: turn.parts.map((part) =>
      part.kind === 'query' ? { kind: 'query', query: { ...part.query, rows: [], columns: [] } } : part,
    ),
  };
}

export function saveConversation(id: string, turns: TurnView[]): void {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') as Record<string, TurnView[]>;
    saved[id] = turns.map(withoutRows);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(saved));
  } catch {
    // Storage is a convenience; memory in Rust is the source of truth.
  }
}

export function loadConversation(id: string): TurnView[] {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') as Record<string, TurnView[]>;
    return saved[id] ?? [];
  } catch {
    return [];
  }
}

/** Only https and mailto links may open. */
export function isSafeLink(href: string | undefined): boolean {
  if (!href) return false;
  try {
    const protocol = new URL(href).protocol;
    return protocol === 'https:' || protocol === 'mailto:';
  } catch {
    return false;
  }
}
