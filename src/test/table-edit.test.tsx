import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  tablePage: vi.fn(),
  tableCount: vi.fn(),
  tableStructure: vi.fn(),
  planRowChange: vi.fn(),
  prepareQuery: vi.fn(),
  approveQuery: vi.fn(),
  runQuery: vi.fn(),
  confirm: vi.fn(),
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
    targetFor: () => ({ session: 's1', connectionName: 'main', environment: 'Development' }),
    confirm: mocks.confirm,
  }),
}));

import { TableTab } from '../components/workbench/TableTab';

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });

it('sends a grid edit to Rust and confirms the SQL Rust planned', async () => {
  const columns = [
    { name: 'id', typeName: 'int4', nullable: null },
    { name: 'name', typeName: 'text', nullable: null },
  ];
  mocks.tablePage.mockReturnValue(ok({ columns, rows: [[{ t: 'Int', v: 1 }, { t: 'Text', v: 'Ann' }]] }));
  mocks.tableCount.mockReturnValue(ok(1));
  mocks.tableStructure.mockReturnValue(ok(null));
  const planned = `UPDATE "public"."t" SET "name" = 'Bea' WHERE "id" = '1'`;
  mocks.planRowChange.mockReturnValue(ok({ sql: planned, warnings: ['no key'] }));
  const verdict = { span: { start: 0, end: planned.length }, kind: 'Update', readOnly: false, riskReasons: [], reason: null };
  mocks.prepareQuery.mockReturnValue(ok([verdict]));
  mocks.confirm.mockResolvedValue(false);

  const tab = {
    id: 't',
    kind: 'table' as const,
    title: 't',
    profileId: 'p1',
    table: { schema: 'public', name: 't' },
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <TableTab tab={tab} />
    </QueryClientProvider>,
  );

  const cell = await screen.findByText('Ann');
  fireEvent.doubleClick(cell);
  fireEvent.change(await screen.findByLabelText('name'), { target: { value: 'Bea' } });
  fireEvent.click(screen.getByRole('button', { name: 'Review change' }));

  await waitFor(() => expect(mocks.confirm).toHaveBeenCalledOnce());
  const change = mocks.planRowChange.mock.calls[0]?.[2];
  expect(change.kind).toBe('update');
  expect(change.values).toEqual([{ column: 'name', value: 'Bea' }]);
  expect(change.key).toContainEqual({ column: 'id', value: '1' });

  const request = mocks.confirm.mock.calls[0]?.[0];
  expect(request.sql).toBe(planned);
  expect(request.warnings).toEqual(['no key']);
  // Declined, so nothing may have been approved or run.
  expect(mocks.approveQuery).not.toHaveBeenCalled();
  expect(mocks.runQuery).not.toHaveBeenCalled();
});
