import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  tablePage: vi.fn(),
  tableCount: vi.fn(),
  tableStructure: vi.fn(),
  gridEditInfo: vi.fn(),
  planRowChange: vi.fn(),
  planRowChanges: vi.fn(),
  runRowChanges: vi.fn(),
  prepareQuery: vi.fn(),
  approveQuery: vi.fn(),
  runQuery: vi.fn(),
  confirm: vi.fn(),
  setUnsaved: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: (message: unknown) => void = () => {};
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../bindings', () => ({ commands: mocks, events: {} }));
vi.mock('../context/Studio', () => ({
  newId: () => 'id',
  useStudio: () => ({
    sessions: { p1: { sessionId: 's1' } },
    profiles: [{ id: 'p1', readOnly: false }],
    targetFor: () => ({ session: 's1', connectionName: 'main', environment: 'Development' }),
    confirm: mocks.confirm,
    setUnsaved: mocks.setUnsaved,
  }),
}));

import { TableTab } from '../components/workbench/TableTab';

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });

const columns = [
  { name: 'id', typeName: 'int4', nullable: null },
  { name: 'name', typeName: 'text', nullable: null },
  { name: 'secret', typeName: 'text', nullable: null },
];
const planned = `UPDATE "public"."t" SET "name" = 'Bea' WHERE "id" = '1' AND "name" = 'Ann'`;
const verdict = { span: { start: 0, end: planned.length }, kind: 'Update', readOnly: false, riskReasons: [], reason: null };

const twoRows = [
  [{ t: 'Int', v: 1 }, { t: 'Text', v: 'Ann' }, { t: 'Text', v: '***' }],
  [{ t: 'Int', v: 2 }, { t: 'Text', v: 'Bob' }, { t: 'Text', v: '***' }],
];

function setup(primaryKey: string[] = ['id'], rows: unknown[][] = twoRows) {
  mocks.tablePage.mockReturnValue(ok({ columns, rows }));
  mocks.tableCount.mockReturnValue(ok(200));
  mocks.tableStructure.mockReturnValue(ok(null));
  mocks.gridEditInfo.mockReturnValue(
    ok({ primaryKey, locked: [{ name: 'secret', reason: 'Masked' }], maxChanges: 1000 }),
  );
  mocks.planRowChanges.mockReturnValue(ok({ sql: planned, warnings: ['all or nothing'] }));
  mocks.prepareQuery.mockReturnValue(ok([verdict]));
  const tab = { id: 't', kind: 'table' as const, title: 't', profileId: 'p1', table: { schema: 'public', name: 't' } };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <TableTab tab={tab} />
    </QueryClientProvider>,
  );
}

async function editName(to: string) {
  await waitFor(() => expect(mocks.gridEditInfo).toHaveBeenCalled());
  const cell = await screen.findByText('Ann');
  await waitFor(() => expect(screen.queryByText(/no primary key/)).toBeNull());
  fireEvent.doubleClick(cell);
  const input = await screen.findByLabelText('Cell value');
  fireEvent.change(input, { target: { value: to } });
  fireEvent.keyDown(input, { key: 'Enter' });
}

beforeEach(() => {
  vi.clearAllMocks();
});

it('keeps a cell edit pending, then sends the change set to Rust for review', async () => {
  setup();
  await editName('Bea');
  const bar = await screen.findByRole('status', { name: 'Unsaved changes' });
  expect(bar).toHaveTextContent('1 changed');
  expect(mocks.planRowChanges).not.toHaveBeenCalled();
  expect(mocks.setUnsaved).toHaveBeenLastCalledWith('t', true);

  mocks.confirm.mockResolvedValue(false);
  fireEvent.click(within(bar).getByRole('button', { name: 'Review and save' }));
  await waitFor(() => expect(mocks.confirm).toHaveBeenCalledOnce());

  expect(mocks.planRowChanges.mock.calls[0]?.[2]).toEqual({
    updates: [
      {
        key: [{ column: 'id', value: '1' }],
        before: [{ column: 'name', value: 'Ann' }],
        after: [{ column: 'name', value: 'Bea' }],
      },
    ],
    inserts: [],
    deletes: [],
  });
  const request = mocks.confirm.mock.calls[0]?.[0];
  expect(request.sql).toBe(planned);
  expect(request.warnings).toEqual(['all or nothing']);
  // Declined: nothing approved or run, and the edits are still pending.
  expect(mocks.approveQuery).not.toHaveBeenCalled();
  expect(mocks.runRowChanges).not.toHaveBeenCalled();
  expect(await screen.findByText('Change not made: confirmation declined')).toBeInTheDocument();
  expect(screen.getByRole('status', { name: 'Unsaved changes' })).toBeInTheDocument();
});

it('a confirmed save is approved for the grid and runs through the grid door only', async () => {
  setup();
  await editName('Bea');
  mocks.confirm.mockResolvedValue(true);
  mocks.approveQuery.mockReturnValue(ok({ token: 'tok', verdicts: [verdict] }));
  mocks.runRowChanges.mockReturnValue(ok(1));
  fireEvent.click(await screen.findByRole('button', { name: 'Review and save' }));

  await waitFor(() => expect(mocks.runRowChanges).toHaveBeenCalledOnce());
  expect(mocks.approveQuery).toHaveBeenCalledWith('s1', planned, 'Grid');
  expect(mocks.runRowChanges).toHaveBeenCalledWith('s1', planned, 'tok', 'id');
  expect(mocks.runQuery).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByRole('status', { name: 'Unsaved changes' })).toBeNull());
  expect(screen.getByLabelText('Run log')).toHaveTextContent('Saved 1 change');
});

it('a masked column cannot be edited', async () => {
  setup();
  await waitFor(() => expect(mocks.gridEditInfo).toHaveBeenCalled());
  const [masked] = await screen.findAllByText('***');
  await waitFor(() => expect(masked).toHaveAttribute('title', 'Masked, so it cannot be edited'));
  fireEvent.doubleClick(masked as HTMLElement);
  expect(screen.queryByLabelText('Cell value')).toBeNull();
});

it('marks deletes and new rows without running anything', async () => {
  setup();
  await screen.findByText('Bob');
  await waitFor(() => expect(mocks.gridEditInfo).toHaveBeenCalled());
  fireEvent.click(screen.getByLabelText('Select row 2'));
  fireEvent.click(screen.getByRole('button', { name: /Delete 1 rows/ }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Insert row' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Insert row' }));
  const bar = await screen.findByRole('status', { name: 'Unsaved changes' });
  expect(bar).toHaveTextContent('1 new · 1 deleted');
  expect(screen.getByText('Bob').closest('tr')).toHaveClass('line-through');
  expect(mocks.planRowChanges).not.toHaveBeenCalled();
  expect(mocks.planRowChange).not.toHaveBeenCalled();
});

it('asks before paging away from unsaved edits', async () => {
  const full = Array.from({ length: 100 }, (_, i) => [
    { t: 'Int', v: i },
    { t: 'Text', v: i === 0 ? 'Ann' : `n${i}` },
    { t: 'Null' },
  ]);
  setup(['id'], full);
  await editName('Bea');
  const pages = mocks.tablePage.mock.calls.length;
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(await screen.findByText('Discard unsaved changes?')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Keep editing' }));
  expect(mocks.tablePage.mock.calls.length).toBe(pages);
  expect(screen.getByRole('status', { name: 'Unsaved changes' })).toBeInTheDocument();
});

it('a table without a primary key keeps the form and says why', async () => {
  setup([]);
  expect(await screen.findByText(/no primary key/)).toBeInTheDocument();
  const cell = await screen.findByText('Ann');
  fireEvent.doubleClick(cell);
  expect(screen.queryByLabelText('Cell value')).toBeNull();
  expect(await screen.findByRole('dialog', { name: 'Edit row' })).toBeInTheDocument();
});

it('shows applied filters as chips that can be removed one by one', async () => {
  setup();
  await screen.findByText('Ann');
  fireEvent.click(screen.getByRole('button', { name: /^Filter/ }));
  const dialog = await screen.findByRole('dialog', { name: 'Filter rows' });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Add a filter' }));
  fireEvent.change(within(dialog).getByLabelText('Value'), { target: { value: '7' } });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Apply filters' }));

  const chips = await screen.findByLabelText('Filters');
  expect(chips).toHaveTextContent('id');
  expect(chips).toHaveTextContent('equals');
  await waitFor(() =>
    expect(mocks.tablePage).toHaveBeenLastCalledWith(
      's1',
      { schema: 'public', name: 't' },
      expect.objectContaining({ filters: [{ column: 'id', operator: 'Eq', value: '7' }] }),
    ),
  );

  fireEvent.click(within(chips).getByRole('button', { name: 'Remove filter on id' }));
  await waitFor(() => expect(screen.queryByLabelText('Filters')).toBeNull());
  await waitFor(() =>
    expect(mocks.tablePage).toHaveBeenLastCalledWith('s1', { schema: 'public', name: 't' }, expect.objectContaining({ filters: [] })),
  );
});

it('lays out the structure as tables, marking the key column', async () => {
  // Queued before setup, whose default for the structure is empty.
  mocks.tableStructure.mockReturnValueOnce(
    ok({
      metadata: { owner: 'app', sizeBytes: 2_500_000, rowEstimate: 1200 },
      columns: [
        { name: 'id', dataType: 'integer', nullable: false, defaultValue: null, primaryKey: true },
        { name: 'name', dataType: 'text', nullable: true, defaultValue: null, primaryKey: false },
      ],
      indexes: [{ name: 't_pkey', columns: ['id'], unique: true }],
      foreignKeys: [],
      constraints: [],
      ddl: 'CREATE TABLE t (id integer primary key)',
    }),
  );
  setup();
  await screen.findByText('Ann');
  fireEvent.click(screen.getByRole('button', { name: 'Structure' }));
  const columnsBlock = await screen.findByRole('region', { name: 'Columns' });
  expect(within(columnsBlock).getByLabelText('primary key')).toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Indexes' })).toHaveTextContent('t_pkey');
  expect(screen.getByText('2.4 MB')).toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Definition' })).toHaveTextContent('CREATE TABLE t');
});
