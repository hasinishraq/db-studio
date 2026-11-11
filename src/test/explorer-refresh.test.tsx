import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConnectionProfile } from '../bindings';

const mocks = vi.hoisted(() => ({
  settingsGet: vi.fn(),
  listSchemas: vi.fn(),
  listDatabases: vi.fn(),
  listObjects: vi.fn(),
  useDatabase: vi.fn(),
  setSession: vi.fn(),
  studio: { current: {} as Record<string, unknown> },
}));
vi.mock('@tauri-apps/api/core', () => ({ Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({ commands: mocks, events: {} }));
vi.mock('../context/Studio', () => ({ newId: () => crypto.randomUUID(), useStudio: () => mocks.studio.current }));

import { Explorer } from '../components/shell/Explorer';

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });

const mysql: ConnectionProfile = {
  id: 'p1',
  name: 'Local MySQL',
  engine: 'MySql',
  host: '127.0.0.1',
  port: 3306,
  database: '',
  username: 'u',
  tlsMode: 'Disable',
  caCertPath: null,
  clientCertPath: null,
  clientKeyPath: null,
  ssh: null,
  environment: 'Development',
  readOnly: false,
  aiPolicy: 'Default',
  color: null,
  defaultSchema: null,
  hasPassword: false,
};

const renderExplorer = (profile: ConnectionProfile, database: string | null) => {
  mocks.studio.current = {
    profiles: [profile],
    sessions: { p1: { sessionId: 's1', profileId: 'p1', engine: profile.engine, database, schema: null } },
    setSession: mocks.setSession,
    openTab: vi.fn(),
    tabs: [],
    closeTab: vi.fn(),
  };
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <Explorer />
    </QueryClientProvider>,
  );
};

const pick = (row: HTMLElement, item: string) => {
  fireEvent.contextMenu(row);
  fireEvent.click(screen.getByRole('menuitem', { name: item }));
};

describe('the explorer tree', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.settingsGet.mockReturnValue(new Promise(() => {}));
    mocks.listSchemas.mockReturnValue(ok([{ name: 'shop' }, { name: 'archive' }]));
    mocks.listDatabases.mockReturnValue(ok(['app', 'other']));
    mocks.listObjects.mockReturnValue(ok([{ schema: 'shop', name: 'orders', kind: 'Table' }]));
    mocks.useDatabase.mockReturnValue(
      ok({ sessionId: 's1', profileId: 'p1', engine: 'MySql', server: {}, database: 'shop', schema: null }),
    );
  });

  it('refreshes the databases of a server from its right-click menu', async () => {
    renderExplorer(mysql, null);
    await screen.findByText('archive');
    expect(mocks.listSchemas).toHaveBeenCalledTimes(1);
    mocks.listSchemas.mockReturnValue(ok([{ name: 'shop' }, { name: 'archive' }, { name: 'fresh' }]));
    pick(screen.getByText('Local MySQL'), 'Refresh');
    expect(await screen.findByText('fresh')).toBeInTheDocument();
    expect(mocks.listSchemas).toHaveBeenCalledTimes(2);
  });

  it('refreshes the tables of a database from its right-click menu', async () => {
    renderExplorer(mysql, null);
    const shop = await screen.findByText('shop');
    fireEvent.click(shop);
    await screen.findByText('orders');
    mocks.listObjects.mockReturnValue(ok([{ schema: 'shop', name: 'invoices', kind: 'Table' }]));
    pick(shop, 'Refresh');
    expect(await screen.findByText('invoices')).toBeInTheDocument();
    expect(screen.queryByText('orders')).toBeNull();
  });

  it('opens a database on a left click without putting it in use', async () => {
    renderExplorer(mysql, null);
    const shop = await screen.findByText('shop');
    fireEvent.click(shop);
    expect(await screen.findByText('orders')).toBeInTheDocument();
    expect(mocks.useDatabase).not.toHaveBeenCalled();
    expect(screen.queryByText('in use')).toBeNull();
  });

  it('puts a database in use only from the right-click menu', async () => {
    renderExplorer(mysql, null);
    pick(await screen.findByText('shop'), 'Use as default');
    await waitFor(() => expect(mocks.useDatabase).toHaveBeenCalledWith('s1', 'shop'));
  });

  it('does not switch a PostgreSQL database on a left click', async () => {
    renderExplorer({ ...mysql, engine: 'Postgres', database: 'app' }, 'app');
    fireEvent.click(await screen.findByText('other'));
    expect(mocks.useDatabase).not.toHaveBeenCalled();
  });
});
