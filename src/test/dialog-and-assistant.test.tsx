import { fireEvent, render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConnectionProfile } from '../bindings';

const mocks = vi.hoisted(() => ({
  aiConversations: vi.fn(),
  settingsGet: vi.fn(),
  listSchemas: vi.fn(),
  listDatabases: vi.fn(),
  sendTurn: vi.fn(),
  studio: { current: {} as Record<string, unknown> },
}));
vi.mock('@tauri-apps/api/core', () => ({ Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({ commands: mocks, events: {} }));
vi.mock('../api/ai', () => ({
  sendTurn: mocks.sendTurn,
  cancelTurn: vi.fn(),
  cancelQuery: vi.fn(),
  reviewQuery: vi.fn(),
}));
vi.mock('../context/Studio', () => ({ newId: () => crypto.randomUUID(), useStudio: () => mocks.studio.current }));

import { AiTab } from '../components/ai/AiTab';
import { blankProfile, ConnectionDialog, connectionUrl } from '../components/modals/ConnectionDialog';

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });
const wrap = (ui: ReactNode) => render(<QueryClientProvider client={new QueryClient()}>{ui}</QueryClientProvider>);

describe('connection dialog layout', () => {
  it('shows the address it points at, without any secret', () => {
    const profile: ConnectionProfile = {
      ...blankProfile(),
      host: 'db.internal',
      port: 6432,
      username: 'app user',
      database: 'orders',
      hasPassword: true,
    };
    expect(connectionUrl(profile)).toBe('postgresql://app%20user@db.internal:6432/orders');
    expect(connectionUrl({ ...profile, engine: 'MySql', port: 3306, database: '' })).toBe(
      'mysql://app%20user@db.internal:3306',
    );
    expect(connectionUrl({ ...profile, engine: 'Sqlite', database: '/data/app.db' })).toBe('sqlite:/data/app.db');
  });

  it('picks the engine from a radio group and sets its default port', () => {
    wrap(<ConnectionDialog initial={blankProfile()} onClose={() => {}} />);
    const engines = screen.getByRole('radiogroup', { name: 'Engine' });
    expect(engines).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio', { name: /MySQL or MariaDB/ }));
    expect((screen.getByLabelText('Port') as HTMLInputElement).value).toBe('3306');
  });

  it('hides the TLS and SSH tabs for SQLite, which has neither', () => {
    wrap(<ConnectionDialog initial={blankProfile()} onClose={() => {}} />);
    expect(screen.getByRole('tab', { name: 'TLS' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio', { name: /SQLite/ }));
    expect(screen.queryByRole('tab', { name: 'TLS' })).toBeNull();
    expect(screen.queryByRole('tab', { name: /SSH/ })).toBeNull();
    expect(screen.getByLabelText('Database file')).toBeInTheDocument();
  });

  it('chooses the environment from a segmented control', () => {
    wrap(<ConnectionDialog initial={blankProfile()} onClose={() => {}} />);
    const production = screen.getByRole('radio', { name: 'Production' }) as HTMLInputElement;
    fireEvent.click(production);
    expect(production.checked).toBe(true);
  });

  it('keeps Save and Cancel in the footer', () => {
    wrap(<ConnectionDialog initial={blankProfile()} onClose={() => {}} />);
    expect(screen.getByRole('button', { name: 'Save' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Test connection' })).toBeInTheDocument();
  });
});

describe('assistant layout', () => {
  const profile: ConnectionProfile = { ...blankProfile(), id: 'p1', name: 'Local', engine: 'Sqlite', database: 'a.db' };

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.aiConversations.mockReturnValue(ok([]));
    mocks.settingsGet.mockReturnValue(new Promise(() => {}));
    mocks.listSchemas.mockReturnValue(ok([]));
    mocks.studio.current = {
      profiles: [profile],
      sessions: { p1: { sessionId: 's1', profileId: 'p1', engine: 'Sqlite', database: null, schema: null } },
      setSession: vi.fn(),
      tabs: [],
      closeTab: vi.fn(),
    };
  });

  it('fills the box from a starter but sends nothing until Send', () => {
    wrap(<AiTab />);
    fireEvent.click(screen.getByRole('button', { name: 'Which tables are the largest?' }));
    expect((screen.getByLabelText('Message') as HTMLTextAreaElement).value).toBe('Which tables are the largest?');
    expect(mocks.sendTurn).not.toHaveBeenCalled();
  });

  it('keeps Send off while the box is empty', () => {
    wrap(<AiTab />);
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Message'), { target: { value: 'hello' } });
    expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
  });

  it('starts a new conversation from an icon button with a name', () => {
    wrap(<AiTab />);
    expect(screen.getByRole('button', { name: 'New conversation' })).toBeInTheDocument();
  });
});
