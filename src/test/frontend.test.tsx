import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AiEvent, ConnectionProfile, RunEvent, StatementVerdict, Value } from '../bindings';

const calls = vi.hoisted(() => ({
  prepareQuery: vi.fn(),
  approveQuery: vi.fn(),
  runQuery: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: (message: unknown) => void = () => {};
  },
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({
  commands: calls,
  events: {},
}));

import { ApiError, unwrap } from '../api/errors';
import { applyRunEvent, runFlow, type ConfirmRequest } from '../api/run';
import { Markdown } from '../components/ai/AiTab';
import { ReviewCard } from '../components/ai/ReviewCard';
import { ConfirmDialog } from '../components/modals/ConfirmDialog';
import { blankProfile, ConnectionDialog } from '../components/modals/ConnectionDialog';
import { matches } from '../components/shell/Explorer';
import { parsePlan } from '../components/workbench/PlanView';
import { RunLog } from '../components/workbench/RunLog';
import { chooseChart } from '../lib/chart';
import { applyAiEvent, isSafeLink, saveConversation, type QueryPart, type TurnView } from '../lib/chat';
import { cellText, ResultStore } from '../lib/resultStore';
import { rowCount, spanText } from '../lib/sqlText';
import { arrange, BOX_WIDTH, edges, fit, tableHeight, zoomAt, type DiagramTable } from '../lib/diagram';
import { inOrder, moveTo } from '../lib/explorerPrefs';
import { nextZoom, zoomAction } from '../lib/zoom';

function wrap(ui: ReactNode) {
  return render(<QueryClientProvider client={new QueryClient()}>{ui}</QueryClientProvider>);
}

const read: StatementVerdict = {
  span: { start: 0, end: 8 },
  kind: 'Select',
  readOnly: true,
  riskReasons: [],
  reason: null,
};
const write: StatementVerdict = {
  span: { start: 0, end: 13 },
  kind: 'Delete',
  readOnly: false,
  riskReasons: [],
  reason: 'DELETE removes rows.',
};
const drop: StatementVerdict = {
  ...write,
  kind: 'Ddl',
  riskReasons: ['DROP permanently removes the object and its data.'],
};
const target = { session: 's1', connectionName: 'main', environment: 'Development' as const };

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });

beforeEach(() => {
  Object.values(calls).forEach((mock) => mock.mockReset());
  localStorage.clear();
});

describe('api adapter', () => {
  it('returns data on success', async () => {
    await expect(unwrap(ok(5))).resolves.toBe(5);
  });

  it('turns an error result into an ApiError with its code', async () => {
    const failed = Promise.resolve({
      status: 'error' as const,
      error: { code: 'ApprovalRequired' as const, message: 'needs approval', detail: null },
    });
    const error = await unwrap(failed).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe('ApprovalRequired');
    expect((error as ApiError).message).toBe('needs approval');
  });
});

describe('connection dialog', () => {
  const saved: ConnectionProfile = { ...blankProfile(), name: 'prod', hasPassword: true };

  it('never renders a stored password, only that one is saved', () => {
    wrap(<ConnectionDialog initial={saved} onClose={() => {}} />);
    expect(screen.getByText('Saved')).toBeInTheDocument();
    expect(document.querySelector('input[type="password"]')).toBeNull();
  });

  it('offers an empty field to replace the password', () => {
    wrap(<ConnectionDialog initial={saved} onClose={() => {}} />);
    fireEvent.click(screen.getByText('Replace'));
    const field = document.querySelector<HTMLInputElement>('input[type="password"]');
    expect(field?.value).toBe('');
  });

  it('defaults to full TLS verification', () => {
    expect(blankProfile().tlsMode).toBe('VerifyFull');
  });
});

describe('explorer search', () => {
  it('matches case-insensitively and ignores surrounding spaces', () => {
    expect(matches('Orders', ' ord ')).toBe(true);
    expect(matches('Orders', 'users')).toBe(false);
    expect(matches('anything', '')).toBe(true);
  });
});

describe('result store', () => {
  const batch = (seq: number, rows: number) => ({
    seq,
    rows: Array.from({ length: rows }, (_, i): Value[] => [{ t: 'Int', v: seq * rows + i }]),
  });

  it('appends 1000 batches of 500 rows in under 200 ms', () => {
    const batches = Array.from({ length: 1000 }, (_, i) => batch(i, 500));
    const store = new ResultStore();
    const started = performance.now();
    for (const b of batches) applyRunEvent(store, { event: 'rows', index: 0, batch: b });
    expect(performance.now() - started).toBeLessThan(200);
    expect(store.statement(0).rowCount).toBe(500_000);
  });

  it('finds rows across chunks', () => {
    const store = new ResultStore();
    store.statement(0).append(batch(0, 3));
    store.statement(0).append(batch(1, 3));
    expect(store.statement(0).rowAt(4)).toEqual([{ t: 'Int', v: 4 }]);
    expect(store.statement(0).rowAt(6)).toBeUndefined();
  });

  it('applies a whole run', () => {
    const store = new ResultStore();
    const events: RunEvent[] = [
      { event: 'statementStart', index: 0, span: { start: 0, end: 8 } },
      { event: 'columns', index: 0, columns: [{ name: 'n', typeName: 'int4', nullable: null }] },
      { event: 'rows', index: 0, batch: batch(0, 2) },
      {
        event: 'statementDone',
        index: 0,
        outcome: { kind: 'error', error: { code: 'Cancelled', message: 'stopped', detail: null } },
      },
    ];
    events.forEach((event) => applyRunEvent(store, event));
    expect(store.statement(0).columns[0]?.name).toBe('n');
    expect(store.statement(0).status).toBe('cancelled');
  });

  it('shows exact text for values JavaScript cannot hold', () => {
    expect(cellText({ t: 'BigInt', v: '9007199254740993' })).toBe('9007199254740993');
    expect(cellText({ t: 'Temporal', v: '2026-01-01' })).toBe('2026-01-01');
    expect(cellText({ t: 'Null' })).toBe('NULL');
  });
});

describe('run flow', () => {
  const run = (confirm: (r: ConfirmRequest) => Promise<boolean>) =>
    runFlow({ target, sql: 'statement', origin: 'Editor', confirm, runId: 'r1', store: new ResultStore() });

  it('never opens the dialog for reads', async () => {
    calls.prepareQuery.mockReturnValue(ok([read]));
    calls.runQuery.mockReturnValue(ok([]));
    const confirm = vi.fn();
    await run(confirm);
    expect(confirm).not.toHaveBeenCalled();
    expect(calls.approveQuery).not.toHaveBeenCalled();
    expect(calls.runQuery.mock.calls[0]?.[2]).toBeNull();
  });

  it('always opens the dialog for writes, then approves and runs with the token', async () => {
    calls.prepareQuery.mockReturnValue(ok([write]));
    calls.approveQuery.mockReturnValue(ok({ token: 't1', verdicts: [write] }));
    calls.runQuery.mockReturnValue(ok([]));
    const confirm = vi.fn().mockResolvedValue(true);
    await run(confirm);
    expect(confirm).toHaveBeenCalledOnce();
    expect(calls.approveQuery).toHaveBeenCalledWith('s1', 'statement', 'Editor');
    expect(calls.runQuery.mock.calls[0]?.[2]).toBe('t1');
  });

  it('never approves or runs when declined', async () => {
    calls.prepareQuery.mockReturnValue(ok([write]));
    const result = await run(vi.fn().mockResolvedValue(false));
    expect(result.status).toBe('declined');
    expect(calls.approveQuery).not.toHaveBeenCalled();
    expect(calls.runQuery).not.toHaveBeenCalled();
  });

  it('confirms a script when any statement is not a read', async () => {
    calls.prepareQuery.mockReturnValue(ok([read, write]));
    const confirm = vi.fn().mockResolvedValue(false);
    await run(confirm);
    expect(confirm).toHaveBeenCalledOnce();
  });
});

describe('confirm dialog', () => {
  const request = (verdict: StatementVerdict, environment: 'Development' | 'Production' = 'Development') => ({
    sql: 'DELETE FROM t',
    verdicts: [verdict],
    connectionName: 'main',
    environment,
    warnings: [],
  });
  const runButton = () => screen.getByRole('button', { name: 'Run on main' });

  it('shows the exact statement and allows an ordinary change', () => {
    const settle = vi.fn();
    render(<ConfirmDialog request={request(write)} onSettle={settle} />);
    expect(screen.getByText('DELETE FROM t')).toBeInTheDocument();
    fireEvent.click(runButton());
    expect(settle).toHaveBeenCalledWith(true);
  });

  it('requires the checkbox for high risk', () => {
    render(<ConfirmDialog request={request(drop)} onSettle={() => {}} />);
    expect(runButton()).toBeDisabled();
    fireEvent.click(screen.getByLabelText('I understand this cannot be undone'));
    expect(runButton()).toBeEnabled();
  });

  it('requires the exact connection name on production', () => {
    render(<ConfirmDialog request={request(write, 'Production')} onSettle={() => {}} />);
    expect(runButton()).toBeDisabled();
    const field = screen.getByLabelText('Connection name');
    fireEvent.change(field, { target: { value: 'mai' } });
    expect(runButton()).toBeDisabled();
    fireEvent.change(field, { target: { value: 'main' } });
    expect(runButton()).toBeEnabled();
  });

  it('declines without running', () => {
    const settle = vi.fn();
    render(<ConfirmDialog request={request(write)} onSettle={settle} />);
    fireEvent.click(screen.getByRole('button', { name: "Don't run" }));
    expect(settle).toHaveBeenCalledWith(false);
  });
});

describe('chat events', () => {
  const blank: TurnView = { id: 't', userText: 'q', parts: [], finished: false, error: null };

  it('applies a scripted sequence', () => {
    const script: AiEvent[] = [
      { type: 'text', text: 'Checking ' },
      { type: 'text', text: 'now.' },
      { type: 'queryStart', id: 'q1', sql: 'SELECT 1', readOnly: true, reason: null, needsReview: false, riskReasons: [] },
      {
        type: 'queryResult',
        id: 'q1',
        columns: [{ name: 'n', typeName: 'int4', nullable: null }],
        rows: [[{ t: 'Int', v: 1 }]],
        rowsAffected: null,
        truncated: false,
        elapsedMs: 3,
      },
      { type: 'queryStart', id: 'q2', sql: 'DELETE', readOnly: false, reason: 'no', needsReview: true, riskReasons: [] },
      { type: 'queryDeclined', id: 'q2' },
      { type: 'done', error: null },
    ];
    const turn = script.reduce(applyAiEvent, blank);
    expect(turn.parts).toHaveLength(3);
    expect(turn.parts[0]).toEqual({ kind: 'text', text: 'Checking now.' });
    const [, first, second] = turn.parts;
    expect(first?.kind === 'query' && first.query.status).toBe('done');
    expect(second?.kind === 'query' && second.query.status).toBe('declined');
    expect(turn.finished).toBe(true);
  });

  it('keeps no result rows in browser storage', () => {
    const secret = 'alice-private-value';
    const turn = [
      { type: 'queryStart', id: 'q', sql: 'SELECT 1', readOnly: true, reason: null, needsReview: false, riskReasons: [] },
      {
        type: 'queryResult',
        id: 'q',
        columns: [{ name: 'email', typeName: 'text', nullable: null }],
        rows: [[{ t: 'Text', v: secret }]],
        rowsAffected: null,
        truncated: false,
        elapsedMs: 1,
      },
    ].reduce((t, e) => applyAiEvent(t, e as AiEvent), blank);
    saveConversation('c1', [turn]);
    const stored = JSON.stringify({ ...localStorage });
    expect(stored).toContain('SELECT 1');
    expect(stored).not.toContain(secret);
  });
});

describe('review card', () => {
  const query = (riskReasons: string[]): QueryPart => ({
    id: 'q',
    sql: 'DROP TABLE t',
    status: 'review',
    readOnly: false,
    reason: 'This changes the database structure.',
    riskReasons,
    columns: [],
    rows: [],
    rowsAffected: null,
    truncated: false,
    elapsedMs: 0,
    error: null,
  });
  const approve = () => screen.getByRole('button', { name: 'Approve' });

  it('blocks Approve until the checkbox for high risk', () => {
    const decide = vi.fn();
    render(<ReviewCard query={query(['DROP removes data.'])} connectionName="main" environment="Development" onDecide={decide} />);
    expect(approve()).toBeDisabled();
    fireEvent.click(screen.getByLabelText('I understand this cannot be undone'));
    fireEvent.click(approve());
    expect(decide).toHaveBeenCalledWith(true);
  });

  it('requires the connection name on production', () => {
    render(<ReviewCard query={query([])} connectionName="main" environment="Production" onDecide={() => {}} />);
    expect(approve()).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Connection name'), { target: { value: 'main' } });
    expect(approve()).toBeEnabled();
  });

  it('decides only once', () => {
    const decide = vi.fn();
    render(<ReviewCard query={query([])} connectionName="main" environment="Development" onDecide={decide} />);
    fireEvent.click(screen.getByRole('button', { name: 'Decline' }));
    fireEvent.click(screen.getByRole('button', { name: 'Decline' }));
    expect(decide).toHaveBeenCalledOnce();
    expect(decide).toHaveBeenCalledWith(false);
  });
});

describe('links in model replies', () => {
  it('allows only https and mailto', () => {
    expect(isSafeLink('https://example.com')).toBe(true);
    expect(isSafeLink('mailto:a@example.com')).toBe(true);
    for (const bad of ['javascript:alert(1)', 'file:///etc/passwd', 'http://example.com', 'data:text/html,x', '', undefined]) {
      expect(isSafeLink(bad)).toBe(false);
    }
  });

  it('renders unsafe links as plain text', async () => {
    render(<Markdown text="[safe](https://example.com) [bad](file:///etc/passwd)" />);
    await waitFor(() => expect(screen.getByText('safe')).toBeInTheDocument());
    expect(screen.getByText('safe').closest('a')).not.toBeNull();
    expect(screen.getByText('bad').closest('a')).toBeNull();
  });
});

describe('charts', () => {
  const col = (name: string) => ({ name, typeName: 'x', nullable: null });

  it('shows one number as a stat', () => {
    expect(chooseChart([col('count')], [[{ t: 'BigInt', v: '42' }]])).toMatchObject({ kind: 'stat', value: '42' });
  });

  it('shows dates as a line and categories as bars', () => {
    const dated: Value[][] = [
      [{ t: 'Temporal', v: '2026-01-01' }, { t: 'Int', v: 1 }],
      [{ t: 'Temporal', v: '2026-01-02' }, { t: 'Int', v: 2 }],
    ];
    expect(chooseChart([col('d'), col('n')], dated).kind).toBe('line');
    const named: Value[][] = [
      [{ t: 'Text', v: 'a' }, { t: 'Int', v: 1 }],
      [{ t: 'Text', v: 'b' }, { t: 'Int', v: 2 }],
    ];
    expect(chooseChart([col('name'), col('n')], named).kind).toBe('bar');
  });

  it('does not plot an id as if it were a measure', () => {
    const rows: Value[][] = [
      [{ t: 'Int', v: 1 }, { t: 'Text', v: 'a' }],
      [{ t: 'Int', v: 2 }, { t: 'Text', v: 'b' }],
    ];
    expect(chooseChart([col('id'), col('name')], rows).kind).toBe('none');
    expect(chooseChart([col('user_id'), col('name')], rows).kind).toBe('none');
    expect(chooseChart([col('total'), col('name')], rows).kind).toBe('bar');
  });

  it('declines when there is nothing to plot', () => {
    expect(chooseChart([col('a')], [])).toEqual({ kind: 'none' });
  });
});

describe('explain plans', () => {
  it('reads a PostgreSQL plan', () => {
    const text = JSON.stringify([
      { Plan: { 'Node Type': 'Seq Scan', 'Relation Name': 't', 'Total Cost': 12, 'Plan Rows': 3, Plans: [{ 'Node Type': 'Sort' }] } },
    ]);
    const plan = parsePlan(text);
    expect(plan).toMatchObject({ label: 'Seq Scan', relation: 't', cost: 12, rows: 3 });
    expect(plan?.children[0]?.label).toBe('Sort');
  });

  it('reads a MySQL plan', () => {
    const text = JSON.stringify({
      query_block: { cost_info: { query_cost: '5.5' }, table: { table_name: 't', access_type: 'ALL', rows_examined_per_scan: 9 } },
    });
    const plan = parsePlan(text);
    expect(plan?.cost).toBe(5.5);
    expect(plan?.children[0]).toMatchObject({ relation: 't', label: 'ALL', rows: 9 });
  });

  it('reads a SQLite plan', () => {
    const plan = parsePlan(JSON.stringify([{ id: 2, parent: 0, detail: 'SCAN t' }]));
    expect(plan?.children[0]?.label).toBe('SCAN t');
  });

  it('returns nothing for text that is not a plan', () => {
    expect(parsePlan('not json')).toBeNull();
  });
});

describe('zoom shortcuts', () => {
  const press = (key: string, extra: Partial<KeyboardEvent> = {}) =>
    zoomAction({ key, code: '', ctrlKey: true, metaKey: false, altKey: false, ...extra });

  it('works like VS Code, with or without Shift', () => {
    expect(press('=')).toBe('in');
    expect(press('+')).toBe('in');
    expect(press('-')).toBe('out');
    expect(press('_')).toBe('out');
    expect(press('0')).toBe('reset');
    expect(press('', { code: 'NumpadAdd' })).toBe('in');
    expect(press('', { code: 'NumpadSubtract' })).toBe('out');
  });

  it('leaves other keys and plain typing alone', () => {
    expect(press('k')).toBeNull();
    expect(press('-', { ctrlKey: false })).toBeNull();
    expect(press('+', { altKey: true })).toBeNull();
  });

  it('steps through the levels and stops at the ends', () => {
    expect(nextZoom(1, 'in')).toBe(1.1);
    expect(nextZoom(1, 'out')).toBe(0.9);
    expect(nextZoom(2, 'in')).toBe(2);
    expect(nextZoom(0.5, 'out')).toBe(0.5);
    expect(nextZoom(1.75, 'reset')).toBe(1);
  });
});

describe('statement positions', () => {
  it('cuts by bytes, so text before a statement cannot shift it', () => {
    const sql = "SELECT 'বাংলা'; DELETE FROM t WHERE name = 'é'";
    const start = new TextEncoder().encode("SELECT 'বাংলা'; ").length;
    const end = new TextEncoder().encode(sql).length;
    expect(spanText(sql, { start, end })).toBe("DELETE FROM t WHERE name = 'é'");
  });

  it('the review shows the exact statement after non-English text', () => {
    const sql = "SELECT 'বাংলা'; DELETE FROM t";
    const bytes = new TextEncoder().encode(sql).length;
    const verdict = { ...write, span: { start: bytes - 'DELETE FROM t'.length, end: bytes } };
    render(
      <ConfirmDialog
        request={{ sql, verdicts: [verdict], connectionName: 'main', environment: 'Development', warnings: [] }}
        onSettle={() => {}}
      />,
    );
    expect(screen.getByText('DELETE FROM t')).toBeInTheDocument();
  });

  it('says 1 row, not 1 rows', () => {
    expect(rowCount(1)).toBe('1 row');
    expect(rowCount(2)).toBe('2 rows');
  });
});

describe('run log', () => {
  const entry = (id: number, text: string) => ({ id, at: 0, tone: 'info' as const, text, sql: 'SELECT 1' });

  it('shows nothing until something ran', () => {
    render(<RunLog entries={[]} onClear={() => {}} />);
    expect(screen.queryByLabelText('Run log')).toBeNull();
  });

  it('lists what ran and can be cleared', () => {
    const clear = vi.fn();
    render(<RunLog entries={[entry(1, '1 row affected in 6 ms')]} onClear={clear} />);
    expect(screen.getByText('1 row affected in 6 ms')).toBeInTheDocument();
    expect(screen.getByText('SELECT 1')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Clear log' }));
    expect(clear).toHaveBeenCalledOnce();
  });
});

describe('schema diagram', () => {
  const box = (name: string, columns: string[], keys: DiagramTable['keys'] = []): DiagramTable => ({
    name,
    columns: columns.map((c) => ({ name: c, type: 'int', primaryKey: c === 'id' })),
    keys,
  });
  const orders = box('orders', ['id', 'user_id'], [{ columns: ['user_id'], table: 'users', referencedColumns: ['id'] }]);
  const users = box('users', ['id']);

  it('draws every table and every column, however many', () => {
    const many = Array.from({ length: 200 }, (_, i) => box(`t${i}`, Array.from({ length: 40 }, (_, c) => `c${c}`)));
    const positions = arrange(many);
    expect(Object.keys(positions)).toHaveLength(200);
    expect(tableHeight(many[0] as DiagramTable)).toBeGreaterThan(40 * 20);
  });

  it('never stacks two tables on the same spot', () => {
    const positions = arrange([orders, users, box('a', ['id']), box('b', ['id'])]);
    const spots = Object.values(positions).map((p) => `${p.x},${p.y}`);
    expect(new Set(spots).size).toBe(spots.length);
  });

  it('joins the two columns of a foreign key', () => {
    const positions = { orders: { x: 0, y: 0 }, users: { x: 600, y: 100 } };
    const [edge] = edges([orders, users], positions);
    // user_id is the second column of orders; id is the first of users.
    expect(edge?.path.startsWith(`M${BOX_WIDTH},58`)).toBe(true);
    expect(edge?.path.endsWith('600,138')).toBe(true);
  });

  it('skips a key whose table is not on the diagram', () => {
    expect(edges([orders], { orders: { x: 0, y: 0 } })).toEqual([]);
  });

  it('keeps the point under the cursor still when zooming', () => {
    const before = { x: 10, y: 20, k: 1 };
    const after = zoomAt(before, 2, { x: 110, y: 120 });
    expect((110 - after.x) / after.k).toBeCloseTo((110 - before.x) / before.k);
    expect(zoomAt({ x: 0, y: 0, k: 2 }, 10, { x: 0, y: 0 }).k).toBe(2);
  });

  it('fits a large schema without enlarging a small one', () => {
    const positions = { users: { x: 0, y: 0 } };
    expect(fit([users], positions, 1000, 800).k).toBe(1);
    expect(fit([users], { users: { x: 5000, y: 0 } }, 1000, 800).k).toBeLessThan(0.2);
    expect(fit([users], positions, 0, 0).k).toBe(1);
  });
});

describe('connection order', () => {
  const item = (id: string) => ({ id });

  it('keeps the saved order and puts new connections last', () => {
    const items = ['a', 'b', 'c', 'd'].map(item);
    expect(inOrder(items, ['c', 'a', 'gone']).map((i) => i.id)).toEqual(['c', 'a', 'b', 'd']);
  });

  it('moves before a position, counting from the list as it was', () => {
    expect(moveTo(['a', 'b', 'c', 'd'], 'a', 3)).toEqual(['b', 'c', 'a', 'd']);
    expect(moveTo(['a', 'b', 'c', 'd'], 'd', 0)).toEqual(['d', 'a', 'b', 'c']);
    expect(moveTo(['a', 'b', 'c', 'd'], 'b', 4)).toEqual(['a', 'c', 'd', 'b']);
  });

  it('cannot move past either end or lose anything', () => {
    expect(moveTo(['a', 'b'], 'a', -1)).toEqual(['a', 'b']);
    expect(moveTo(['a', 'b'], 'b', 99)).toEqual(['a', 'b']);
    expect(moveTo(['a', 'b'], 'x', 0)).toEqual(['a', 'b']);
  });
});
