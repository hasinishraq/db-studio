import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { clsx } from 'clsx';
import { Group, Panel, Separator } from 'react-resizable-panels';
import {
  Bot,
  Database,
  FileClock,
  FileCode2,
  History,
  Lock,
  Network,
  Plus,
  Search,
  Settings,
  ShieldCheck,
  Table2,
  X,
} from 'lucide-react';
import { api } from '../../api';
import { changeZoom, restoreZoom } from '../../api/zoom';
import { events, type Engine } from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import { zoomAction } from '../../lib/zoom';
import { AiTab } from '../ai/AiTab';
import { AuditTab } from '../workbench/AuditTab';
import { ErDiagram } from '../workbench/ErDiagram';
import { SettingsTab } from '../workbench/SettingsTab';
import { SqlTab } from '../workbench/SqlTab';
import { TableTab } from '../workbench/TableTab';
import { Button, Kbd } from '../ui';
import { notify } from '../ui/toast';
import { Explorer } from './Explorer';
import { Palette, type PaletteAction } from './Palette';

type Side = 'explorer' | 'history' | 'drafts';

function HistoryList() {
  const { openTab } = useStudio();
  const { data = [], refetch } = useQuery({ queryKey: ['history'], queryFn: () => api.historyList(200) });
  return (
    <div className="h-full overflow-auto px-1.5 pb-2 text-sm">
      {data.length === 0 && <p className="px-2 py-1 text-muted">Queries you run appear here.</p>}
      {data.length > 0 && (
        <Button tone="quiet" className="mb-1 h-6 px-2 text-xs" onClick={() => void api.historyClear().then(() => refetch())}>
          Clear history
        </Button>
      )}
      {data.map((entry) => (
        <button
          key={entry.id}
          type="button"
          className="block h-7 w-full truncate rounded px-2 text-left font-mono text-xs text-ink/90 hover:bg-hover"
          title={entry.sql}
          onClick={() =>
            openTab({ id: newId(), kind: 'sql', title: 'From history', profileId: entry.profileId, draftId: null, sql: entry.sql })
          }
        >
          {entry.sql}
        </button>
      ))}
    </div>
  );
}

function DraftList() {
  const { openTab } = useStudio();
  const { data = [] } = useQuery({ queryKey: ['drafts'], queryFn: api.draftsList });
  return (
    <div className="h-full overflow-auto px-1.5 pb-2 text-sm">
      {data.length === 0 && <p className="px-2 py-1 text-muted">Queries save themselves here as you type.</p>}
      {data.map((draft) => (
        <button
          key={draft.id}
          type="button"
          className="flex h-7 w-full items-center gap-2 truncate rounded px-2 text-left hover:bg-hover"
          onClick={() =>
            openTab({ id: draft.id, kind: 'sql', title: draft.title, profileId: draft.profileId, draftId: draft.id, sql: draft.sql })
          }
        >
          <FileCode2 size={14} aria-hidden className="shrink-0 text-muted" />
          <span className="truncate">{draft.title}</span>
        </button>
      ))}
    </div>
  );
}

function TabBody({ tab }: { tab: Tab }) {
  switch (tab.kind) {
    case 'sql':
      return <SqlTab tab={tab} />;
    case 'table':
      return <TableTab tab={tab} />;
    case 'er':
      return <ErDiagram tab={tab} />;
    case 'ai':
      return <AiTab />;
    case 'audit':
      return <AuditTab />;
    case 'settings':
      return <SettingsTab />;
  }
}

/** One item of the activity bar: an icon, a tooltip, and an accent edge when it is open. */
function RailButton({
  label,
  icon: Icon,
  active,
  onClick,
}: {
  label: string;
  icon: typeof Database;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      title={label}
      onClick={onClick}
      className={clsx(
        'relative flex h-10 w-full items-center justify-center transition-colors',
        active
          ? 'text-ink before:absolute before:inset-y-2 before:left-0 before:w-0.5 before:rounded-r before:bg-accent'
          : 'text-muted hover:text-ink',
      )}
    >
      <span className={clsx('flex h-8 w-8 items-center justify-center rounded-md', active ? 'bg-hover' : 'hover:bg-hover')}>
        <Icon size={18} strokeWidth={1.75} aria-hidden />
      </span>
    </button>
  );
}

const TAB_ICON: Record<Tab['kind'], typeof Database> = {
  sql: FileCode2,
  table: Table2,
  er: Network,
  ai: Bot,
  audit: ShieldCheck,
  settings: Settings,
};

const ENGINE: Record<Engine, string> = { Postgres: 'PostgreSQL', MySql: 'MySQL', Sqlite: 'SQLite' };

/** The band down the window's edge says which environment you are touching. */
const BAND = { Development: 'bg-safe', Staging: 'bg-change', Production: 'bg-danger' } as const;
const ON_BAND = { Development: 'text-on-safe', Staging: 'text-on-change', Production: 'text-on-danger' } as const;

/** The empty workspace: what to do first, and the shortcuts worth learning. */
function Welcome({ onNewQuery, onPalette }: { onNewQuery: () => void; onPalette: () => void }) {
  const shortcuts: [string, string[]][] = [
    ['Search commands', ['Ctrl', 'K']],
    ['New query', ['Ctrl', 'T']],
    ['Toggle the side panel', ['Ctrl', 'B']],
    ['Open the assistant', ['Ctrl', 'J']],
  ];
  return (
    <div className="flex h-full items-center justify-center p-10">
      <div className="w-full max-w-md">
        <h1 className="text-xl font-semibold">Work with your data, with a check before every change.</h1>
        <p className="mt-2 text-sm text-muted">
          Connect to a database from the panel on the left, then open a table or write a query.
        </p>
        <div className="mt-5 flex gap-2">
          <Button tone="primary" onClick={onNewQuery}>
            New query
          </Button>
          <Button onClick={onPalette}>Search commands</Button>
        </div>
        <dl className="mt-8 divide-y divide-line border-y border-line text-sm">
          {shortcuts.map(([label, keys]) => (
            <div key={label} className="flex h-9 items-center justify-between">
              <dt className="text-muted">{label}</dt>
              <dd className="flex gap-1">
                {keys.map((key) => (
                  <Kbd key={key}>{key}</Kbd>
                ))}
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  );
}

export function Shell() {
  const { tabs, activeId, activate, closeTab, openTab, activeProfile, sessions, setSession, profiles } = useStudio();
  const [side, setSide] = useState<Side>('explorer');
  const [showSide, setShowSide] = useState(true);
  const [palette, setPalette] = useState(false);
  const active = tabs.find((t) => t.id === activeId) ?? null;

  const openAi = () => openTab({ id: 'ai', kind: 'ai', title: 'Assistant' });
  const openAudit = () => openTab({ id: 'audit', kind: 'audit', title: 'Audit log' });
  const openSettings = () => openTab({ id: 'settings', kind: 'settings', title: 'Settings' });
  const newQuery = () => {
    const connected = profiles.find((p) => sessions[p.id]);
    openTab({ id: newId(), kind: 'sql', title: 'New query', profileId: connected?.id ?? null, draftId: null, sql: '' });
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      if (!mod) return;
      const key = event.key.toLowerCase();
      const actions: Record<string, () => void> = {
        k: () => setPalette(true),
        b: () => setShowSide((s) => !s),
        j: openAi,
        l: openAudit,
        t: newQuery,
        ',': openSettings,
        w: () => activeId && closeTab(activeId),
        r: () => {},
        s: () => {},
      };
      const action = actions[key];
      if (action) {
        event.preventDefault();
        action();
      } else if (/^[1-4]$/.test(key)) {
        event.preventDefault();
        const tab = tabs[Number(key) - 1];
        if (tab) activate(tab.id);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  // Capture phase, so the shortcut works with the cursor in the SQL editor too.
  useEffect(() => {
    restoreZoom();
    const onKey = (event: KeyboardEvent) => {
      const action = zoomAction(event);
      if (!action) return;
      event.preventDefault();
      event.stopPropagation();
      changeZoom(action);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  useEffect(() => {
    const lost = events.connectionLost.listen((event) => {
      const entry = Object.entries(sessions).find(([, s]) => s.sessionId === event.payload.sessionId);
      if (entry) setSession(entry[0], null);
      notify.error('A connection was lost. Connect again to continue.');
    });
    const warned = events.secretStoreWarning.listen((event) => notify.warning(event.payload.message));
    return () => {
      void lost.then((stop) => stop());
      void warned.then((stop) => stop());
    };
  }, [sessions, setSession]);

  useEffect(() => {
    void api.importLegacy().then((summary) => {
      if (summary.found) {
        const lines = [`Imported ${summary.profiles} connections and ${summary.drafts} queries.`, ...summary.notes];
        notify.info(lines.join(' '));
      }
    });
  }, []);

  const environment = activeProfile?.environment;
  const sideItems: { id: Side; label: string; icon: typeof Database }[] = [
    { id: 'explorer', label: 'Connections', icon: Database },
    { id: 'history', label: 'History', icon: History },
    { id: 'drafts', label: 'Saved queries', icon: FileClock },
  ];
  const sideTitle = sideItems.find((item) => item.id === side)?.label ?? '';
  const connected = profiles.filter((p) => sessions[p.id]);
  const paletteActions: PaletteAction[] = [
    { group: 'Actions', label: 'New query', run: newQuery, icon: FileCode2, shortcut: ['Ctrl', 'T'] },
    {
      group: 'Actions',
      label: 'New connection',
      run: () => window.dispatchEvent(new Event('studio:new-connection')),
      icon: Plus,
    },
    { group: 'Actions', label: 'Open the assistant', run: openAi, icon: Bot, shortcut: ['Ctrl', 'J'] },
    { group: 'Actions', label: 'Open the audit log', run: openAudit, icon: ShieldCheck, shortcut: ['Ctrl', 'L'] },
    { group: 'Actions', label: 'Open settings', run: openSettings, icon: Settings, shortcut: ['Ctrl', ','] },
    ...connected.map<PaletteAction>((profile) => ({
      group: 'Connections',
      label: `New query on ${profile.name}`,
      icon: Database,
      run: () => openTab({ id: newId(), kind: 'sql', title: 'New query', profileId: profile.id, draftId: null, sql: '' }),
    })),
    ...tabs.map<PaletteAction>((tab) => ({
      group: 'Open tabs',
      label: tab.title,
      icon: TAB_ICON[tab.kind],
      run: () => activate(tab.id),
    })),
    { group: 'View', label: 'Toggle the side panel', run: () => setShowSide((s) => !s), shortcut: ['Ctrl', 'B'] },
    { group: 'View', label: 'Make everything bigger', run: () => changeZoom('in'), shortcut: ['Ctrl', '+'] },
    { group: 'View', label: 'Make everything smaller', run: () => changeZoom('out'), shortcut: ['Ctrl', '-'] },
    { group: 'View', label: 'Reset size', run: () => changeZoom('reset'), shortcut: ['Ctrl', '0'] },
  ];
  const activeSession = activeProfile ? sessions[activeProfile.id] : undefined;

  return (
    <div className="flex h-full">
      {/* The band down the window's edge says which environment you are touching. */}
      <div aria-hidden className={clsx('w-1 shrink-0', environment ? BAND[environment] : 'bg-line')} />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="grid h-9 shrink-0 select-none grid-cols-[1fr_minmax(0,420px)_1fr] items-center gap-3 border-b border-line bg-panel px-3 text-sm">
          <div className="flex min-w-0 items-center gap-2">
            <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-accent text-on-accent">
              <Database size={12} strokeWidth={2.25} aria-hidden />
            </span>
            <span className="font-semibold">DB Studio</span>
            {activeProfile && (
              <span className="flex min-w-0 items-center gap-1.5 text-muted">
                <span aria-hidden className="text-edge">
                  /
                </span>
                <span className="truncate text-ink/90">{activeProfile.name}</span>
                {activeSession?.database && (
                  <>
                    <span aria-hidden className="text-edge">
                      /
                    </span>
                    <span className="truncate">{activeSession.database}</span>
                  </>
                )}
              </span>
            )}
            {environment && environment !== 'Development' && (
              <span
                className={clsx('shrink-0 rounded px-1.5 py-px text-2xs font-semibold', BAND[environment], ON_BAND[environment])}
              >
                {environment}
              </span>
            )}
            {activeProfile?.readOnly && (
              <span className="flex shrink-0 items-center gap-1 rounded border border-line px-1.5 py-px text-2xs text-muted">
                <Lock size={10} aria-hidden />
                Read-only
              </span>
            )}
          </div>
          <button
            type="button"
            onClick={() => setPalette(true)}
            className="flex h-6 w-full items-center gap-2 rounded-md border border-line bg-ground px-2 text-xs text-muted transition-colors hover:border-edge hover:text-ink"
          >
            <Search size={13} aria-hidden />
            <span className="flex-1 truncate text-left">Search commands, tabs and connections</span>
            <Kbd>Ctrl K</Kbd>
          </button>
          <div className="flex justify-end">
            <Button tone="quiet" className="h-6 px-2 text-xs" onClick={newQuery} title="New query (Ctrl+T)">
              <Plus size={14} aria-hidden />
              New query
            </Button>
          </div>
        </header>
        <div className="flex min-h-0 flex-1">
          <nav aria-label="Main" className="flex w-11 shrink-0 select-none flex-col border-r border-line bg-panel py-1">
            {sideItems.map((item) => (
              <RailButton
                key={item.id}
                {...item}
                active={side === item.id && showSide}
                onClick={() => {
                  setShowSide(side !== item.id || !showSide);
                  setSide(item.id);
                }}
              />
            ))}
            <div aria-hidden className="mx-2.5 my-1 h-px bg-line" />
            <RailButton label="Assistant" icon={Bot} active={activeId === 'ai'} onClick={openAi} />
            <RailButton label="Audit log" icon={ShieldCheck} active={activeId === 'audit'} onClick={openAudit} />
            <div className="mt-auto">
              <RailButton label="Settings" icon={Settings} active={activeId === 'settings'} onClick={openSettings} />
            </div>
          </nav>
          <Group orientation="horizontal" className="min-w-0 flex-1">
            {showSide && (
              <>
                <Panel defaultSize="22%" minSize="12%" maxSize="45%">
                  <div className="flex h-full flex-col bg-panel">
                    <div className="flex h-9 shrink-0 select-none items-center px-3 text-xs font-semibold text-muted">
                      {sideTitle}
                    </div>
                    <div className="min-h-0 flex-1">
                      {side === 'explorer' ? <Explorer /> : side === 'history' ? <HistoryList /> : <DraftList />}
                    </div>
                  </div>
                </Panel>
                <Separator className="w-px bg-line transition-colors hover:bg-accent data-[separator=active]:bg-accent" />
              </>
            )}
            <Panel>
              <div className="flex h-full min-w-0 flex-col">
                <div
                  role="tablist"
                  className="flex h-9 shrink-0 select-none overflow-x-auto overflow-y-hidden border-b border-line bg-panel"
                >
                  {tabs.map((tab) => {
                    const Icon = TAB_ICON[tab.kind];
                    const current = tab.id === activeId;
                    return (
                      <div
                        key={tab.id}
                        className={clsx(
                          'group relative flex min-w-[120px] max-w-[220px] shrink-0 items-center gap-1.5 border-r border-line pl-3 pr-1 text-sm',
                          // The active tab joins the workspace under it.
                          current
                            ? 'bg-ground text-ink after:absolute after:inset-x-0 after:top-0 after:h-0.5 after:bg-accent'
                            : 'text-muted hover:bg-hover hover:text-ink',
                        )}
                        onAuxClick={(event) => event.button === 1 && closeTab(tab.id)}
                      >
                        <Icon size={14} aria-hidden className={clsx('shrink-0', current ? 'text-accent' : 'text-muted')} />
                        <button
                          type="button"
                          role="tab"
                          className="min-w-0 flex-1 truncate py-2 text-left"
                          aria-selected={current}
                          title={tab.title}
                          onClick={() => activate(tab.id)}
                        >
                          {tab.title}
                        </button>
                        <button
                          type="button"
                          aria-label={`Close ${tab.title}`}
                          title="Close (Ctrl+W)"
                          className={clsx(
                            'shrink-0 rounded p-0.5 text-muted hover:bg-hover hover:text-ink',
                            current ? 'opacity-100' : 'opacity-0 focus:opacity-100 group-hover:opacity-100',
                          )}
                          onClick={() => closeTab(tab.id)}
                        >
                          <X size={13} />
                        </button>
                      </div>
                    );
                  })}
                  <button
                    type="button"
                    aria-label="New query"
                    title="New query (Ctrl+T)"
                    className="flex w-9 shrink-0 items-center justify-center text-muted hover:bg-hover hover:text-ink"
                    onClick={newQuery}
                  >
                    <Plus size={15} />
                  </button>
                </div>
                <main className="min-h-0 flex-1 bg-ground">
                  {tabs.map((tab) => (
                    <div key={tab.id} className={clsx('h-full', tab.id !== active?.id && 'hidden')}>
                      <TabBody tab={tab} />
                    </div>
                  ))}
                  {!active && <Welcome onNewQuery={newQuery} onPalette={() => setPalette(true)} />}
                </main>
              </div>
            </Panel>
          </Group>
        </div>
        <footer className="flex h-6 shrink-0 select-none items-center gap-3 border-t border-line bg-panel px-3 text-xs text-muted">
          {activeProfile ? (
            <>
              <span className="flex items-center gap-1.5">
                <span aria-hidden className={clsx('h-1.5 w-1.5 rounded-full', activeSession ? 'bg-safe' : 'bg-edge')} />
                {activeSession ? 'Connected' : 'Not connected'}
              </span>
              <span>{ENGINE[activeProfile.engine]}</span>
              {activeSession?.server.version && <span className="truncate">{activeSession.server.version}</span>}
              {activeSession && (
                <span className={activeSession.server.tls ? '' : 'text-change'}>
                  {activeSession.server.tls
                    ? activeSession.server.tlsVerified
                      ? 'TLS verified'
                      : 'TLS, not verified'
                    : 'No TLS'}
                </span>
              )}
            </>
          ) : (
            <span>No connection selected</span>
          )}
          <span className="ml-auto">{tabs.length === 1 ? '1 tab' : `${tabs.length} tabs`}</span>
        </footer>
      </div>
      {palette && <Palette onClose={() => setPalette(false)} actions={paletteActions} />}
    </div>
  );
}
