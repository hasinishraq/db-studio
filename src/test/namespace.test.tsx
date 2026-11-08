import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConnectionProfile } from '../bindings';

const mocks = vi.hoisted(() => ({
  aiConversations: vi.fn(),
  settingsGet: vi.fn(),
  listSchemas: vi.fn(),
  listDatabases: vi.fn(),
  useDatabase: vi.fn(),
  useSchema: vi.fn(),
  rememberDefault: vi.fn(),
  setSession: vi.fn(),
  studio: { current: {} as Record<string, unknown> },
}));
vi.mock('@tauri-apps/api/core', () => ({ Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({ commands: mocks, events: {} }));
vi.mock('../context/Studio', () => ({ newId: () => crypto.randomUUID(), useStudio: () => mocks.studio.current }));

import { AiTab } from '../components/ai/AiTab';
import { useContextMenu, type MenuItem } from '../components/ui/ContextMenu';
import { whereLabel } from '../lib/useNamespace';

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });

const base: ConnectionProfile = {
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

describe('where a connection works', () => {
  it('names the server and the database, and the schema on PostgreSQL', () => {
    expect(whereLabel(base, 'shop', null)).toBe('Local MySQL › shop');
    expect(whereLabel(base, null, null)).toBe('Local MySQL (no database in use)');
    expect(whereLabel({ ...base, name: 'PG', engine: 'Postgres' }, 'app', 'sales')).toBe('PG › app › sales');
    expect(whereLabel({ ...base, name: 'Lite', engine: 'Sqlite', database: 'C:\\data\\notes.db' }, null, null)).toBe(
      'Lite › notes.db',
    );
  });
});

function Row({ items }: { items: MenuItem[] }) {
  const { triggers, menu } = useContextMenu('shop actions', () => items);
  return (
    <div>
      <button type="button" onContextMenu={triggers.onContextMenu} onKeyDown={triggers.onKeyDown}>
        shop
      </button>
      {menu}
    </div>
  );
}

describe('the right-click menu', () => {
  const use = vi.fn();
  const remember = vi.fn();
  const items: MenuItem[] = [
    { label: 'Use as default', onSelect: use },
    { label: 'Use as default and remember', onSelect: remember },
    { label: 'In use', onSelect: vi.fn(), disabled: true },
  ];

  it('opens on right-click and runs the picked item', () => {
    render(<Row items={items} />);
    fireEvent.contextMenu(screen.getByText('shop'));
    expect(screen.getByRole('menu', { name: 'shop actions' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Use as default and remember' }));
    expect(remember).toHaveBeenCalledOnce();
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('opens from the keyboard and moves with the arrow keys, skipping disabled items', () => {
    render(<Row items={items} />);
    fireEvent.keyDown(screen.getByText('shop'), { key: 'F10', shiftKey: true });
    const menu = screen.getByRole('menu');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Use as default' }));
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Use as default and remember' }));
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Use as default' }));
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('the assistant’s place', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.aiConversations.mockReturnValue(
      ok([{ id: 'c1', title: 'Top customers', updatedAt: 1, connection: 'Local MySQL', database: 'shop' }]),
    );
    mocks.settingsGet.mockReturnValue(new Promise(() => {}));
    mocks.listSchemas.mockReturnValue(ok([{ name: 'shop' }, { name: 'archive' }]));
    mocks.useDatabase.mockReturnValue(
      ok({ sessionId: 's1', profileId: 'p1', engine: 'MySql', server: {}, database: 'shop', schema: null }),
    );
  });

  const renderTab = (database: string | null) => {
    mocks.studio.current = {
      profiles: [base],
      sessions: { p1: { sessionId: 's1', profileId: 'p1', engine: 'MySql', database, schema: null } },
      setSession: mocks.setSession,
      tabs: [],
      closeTab: vi.fn(),
    };
    return render(
      <QueryClientProvider client={new QueryClient()}>
        <AiTab />
      </QueryClientProvider>,
    );
  };

  it('asks for a database when none is in use, and switches through Rust', async () => {
    renderTab(null);
    expect(screen.getByText(/no database in use. Pick one/)).toBeInTheDocument();
    const picker = (await screen.findByLabelText('Database')) as HTMLSelectElement;
    await waitFor(() => expect(picker.options).toHaveLength(3));
    fireEvent.change(picker, { target: { value: 'shop' } });
    await waitFor(() => expect(mocks.useDatabase).toHaveBeenCalledWith('s1', 'shop'));
    expect(mocks.setSession).toHaveBeenCalledWith('p1', expect.objectContaining({ database: 'shop' }));
  });

  it('names the server and database it works in', async () => {
    renderTab('shop');
    expect(screen.getByText('Working in Local MySQL › shop')).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Local MySQL › shop' })).toBeInTheDocument();
    expect(await screen.findByText('Local MySQL · shop')).toBeInTheDocument();
  });
});
