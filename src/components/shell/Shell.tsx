import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Command } from 'cmdk';
import { clsx } from 'clsx';
import { Group, Panel, Separator } from 'react-resizable-panels';
import { Bot, Database, FileClock, History, Settings, ShieldCheck, X } from 'lucide-react';
import { api } from '../../api';
import { changeZoom, restoreZoom } from '../../api/zoom';
import { events } from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import { zoomAction } from '../../lib/zoom';
import { AiTab } from '../ai/AiTab';
import { AuditTab } from '../workbench/AuditTab';
import { ErDiagram } from '../workbench/ErDiagram';
import { SettingsTab } from '../workbench/SettingsTab';
import { SqlTab } from '../workbench/SqlTab';
import { TableTab } from '../workbench/TableTab';
import { Button } from '../ui';
import { Explorer } from './Explorer';

type Side = 'explorer' | 'history' | 'drafts';

function HistoryList() {
  const { openTab } = useStudio();
  const { data = [], refetch } = useQuery({ queryKey: ['history'], queryFn: () => api.historyList(200) });
  return (
    <div className="h-full overflow-auto p-2 text-sm">
      {data.length === 0 && <p className="text-muted">Queries you run appear here.</p>}
      {data.length > 0 && (
        <Button className="mb-2" onClick={() => void api.historyClear().then(() => refetch())}>
          Clear history
        </Button>
      )}
      {data.map((entry) => (
        <button
          key={entry.id}
          type="button"
          className="mb-0.5 block w-full truncate rounded px-1.5 py-1 text-left font-mono text-xs hover:bg-raised"
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
    <div className="h-full overflow-auto p-2 text-sm">
      {data.length === 0 && <p className="text-muted">Queries save themselves here as you type.</p>}
      {data.map((draft) => (
        <button
          key={draft.id}
          type="button"
          className="block w-full truncate rounded px-1.5 py-1 text-left hover:bg-raised"
          onClick={() =>
            openTab({ id: draft.id, kind: 'sql', title: draft.title, profileId: draft.profileId, draftId: draft.id, sql: draft.sql })
          }
        >
          {draft.title}
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

function Palette({ onClose, actions }: { onClose: () => void; actions: { label: string; run: () => void }[] }) {
  return (
    <div className="fixed inset-0 z-50 bg-black/55" onClick={onClose}>
      <Command
        label="Commands"
        className="mx-auto mt-24 w-[92vw] max-w-lg rounded-lg border border-line bg-panel p-2 shadow-pop"
        onClick={(e) => e.stopPropagation()}
      >
        <Command.Input autoFocus placeholder="Type a command" className="mb-2 w-full rounded bg-ground px-2 py-1 text-sm outline-none" />
        <Command.List>
          <Command.Empty className="p-2 text-sm text-muted">No matching command.</Command.Empty>
          {actions.map((action) => (
            <Command.Item
              key={action.label}
              onSelect={() => {
                action.run();
                onClose();
              }}
              className="cursor-pointer rounded px-2 py-1 text-sm data-[selected=true]:here"
            >
              {action.label}
            </Command.Item>
          ))}
        </Command.List>
      </Command>
    </div>
  );
}

/** One item of the left bar. Every item is the same size, so the bar lines up. */
function RailButton({
  label,
  short,
  icon: Icon,
  active,
  onClick,
}: {
  label: string;
  short: string;
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
        'flex h-14 w-full flex-col items-center justify-center gap-1 text-[11px] leading-none',
        active ? 'here text-accent' : 'text-muted hover:bg-raised hover:text-ink',
      )}
    >
      <Icon size={19} aria-hidden />
      <span>{short}</span>
    </button>
  );
}

/** The band down the window's edge says which environment you are touching. */
const BAND = { Development: 'bg-safe', Staging: 'bg-change', Production: 'bg-danger' } as const;
const ON_BAND = { Development: 'text-on-safe', Staging: 'text-on-change', Production: 'text-on-danger' } as const;

export function Shell() {
  const { tabs, activeId, activate, closeTab, openTab, activeProfile, sessions, setSession, profiles } = useStudio();
  const [side, setSide] = useState<Side>('explorer');
  const [showSide, setShowSide] = useState(true);
  const [palette, setPalette] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
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
      setNotice('A connection was lost. Connect again to continue.');
    });
    const warned = events.secretStoreWarning.listen((event) => setNotice(event.payload.message));
    return () => {
      void lost.then((stop) => stop());
      void warned.then((stop) => stop());
    };
  }, [sessions, setSession]);

  useEffect(() => {
    void api.importLegacy().then((summary) => {
      if (summary.found) {
        const lines = [`Imported ${summary.profiles} connections and ${summary.drafts} queries.`, ...summary.notes];
        setNotice(lines.join(' '));
      }
    });
  }, []);

  const environment = activeProfile?.environment;
  const sideItems: { id: Side; label: string; short: string; icon: typeof Database }[] = [
    { id: 'explorer', label: 'Connections', short: 'Explorer', icon: Database },
    { id: 'history', label: 'History', short: 'History', icon: History },
    { id: 'drafts', label: 'Saved queries', short: 'Saved', icon: FileClock },
  ];
  const paletteActions = [
    { label: 'New query', run: newQuery },
    { label: 'New connection', run: () => window.dispatchEvent(new Event('studio:new-connection')) },
    { label: 'Open the assistant', run: openAi },
    { label: 'Open the audit log', run: openAudit },
    { label: 'Open settings', run: openSettings },
    { label: 'Make everything bigger', run: () => changeZoom('in') },
    { label: 'Make everything smaller', run: () => changeZoom('out') },
    { label: 'Reset size', run: () => changeZoom('reset') },
  ];

  return (
    <div className="flex h-full">
      <div
        aria-hidden
        className={clsx('w-1.5 shrink-0', environment ? BAND[environment] : 'bg-line')}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 border-b border-line bg-panel px-3 py-1 text-sm">
          <strong>DB Studio</strong>
          {activeProfile && (
            <span className="text-muted">
              {activeProfile.name}
              {sessions[activeProfile.id]?.database ? `, ${sessions[activeProfile.id]?.database}` : ''}
            </span>
          )}
          {environment && environment !== 'Development' && (
            <span className={clsx('rounded px-1.5 text-xs font-bold', BAND[environment], ON_BAND[environment])}>{environment}</span>
          )}
          {activeProfile?.readOnly && <span className="rounded border border-line px-1.5 text-xs">Read-only</span>}
          {notice && (
            <span className="ml-auto flex items-center gap-1 truncate text-muted">
              {notice}
              <button type="button" aria-label="Dismiss" onClick={() => setNotice(null)}>
                <X size={13} />
              </button>
            </span>
          )}
        </header>
        <div className="flex min-h-0 flex-1">
          <nav aria-label="Main" className="flex w-16 shrink-0 flex-col border-r border-line bg-panel py-1">
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
            <div aria-hidden className="mx-3 my-1 h-px bg-line" />
            <RailButton label="Assistant" short="Assistant" icon={Bot} active={activeId === 'ai'} onClick={openAi} />
            <RailButton label="Audit log" short="Audit" icon={ShieldCheck} active={activeId === 'audit'} onClick={openAudit} />
            <div className="mt-auto">
              <RailButton
                label="Settings"
                short="Settings"
                icon={Settings}
                active={activeId === 'settings'}
                onClick={openSettings}
              />
            </div>
          </nav>
          <Group orientation="horizontal" className="min-w-0 flex-1">
            {showSide && (
              <>
                <Panel defaultSize="22%" minSize="12%" maxSize="45%">
                  <div className="h-full bg-panel">
                    {side === 'explorer' ? <Explorer /> : side === 'history' ? <HistoryList /> : <DraftList />}
                  </div>
                </Panel>
                <Separator className="w-1 bg-line hover:bg-edge" />
              </>
            )}
            <Panel>
              <div className="flex h-full min-w-0 flex-col">
                <div role="tablist" className="flex overflow-x-auto overflow-y-hidden border-b border-line bg-panel">
                  {tabs.map((tab) => (
                    <div
                      key={tab.id}
                      className={clsx(
                        'group flex shrink-0 items-center gap-1 border-r border-line pl-3 pr-1.5 text-sm',
                        // The active tab joins the workspace under it.
                        tab.id === activeId
                          ? 'here-top bg-ground font-bold text-ink'
                          : 'text-muted hover:bg-raised hover:text-ink',
                      )}
                    >
                      <button
                        type="button"
                        role="tab"
                        className="py-1.5"
                        aria-selected={tab.id === activeId}
                        onClick={() => activate(tab.id)}
                      >
                        {tab.title}
                      </button>
                      <button
                        type="button"
                        aria-label={`Close ${tab.title}`}
                        title="Close"
                        className={clsx(
                          'rounded p-0.5 hover:bg-pick',
                          tab.id === activeId ? 'opacity-100' : 'opacity-0 focus:opacity-100 group-hover:opacity-100',
                        )}
                        onClick={() => closeTab(tab.id)}
                      >
                        <X size={12} />
                      </button>
                    </div>
                  ))}
                </div>
                <main className="min-h-0 flex-1">
                  {tabs.map((tab) => (
                    <div key={tab.id} className={clsx('h-full', tab.id !== active?.id && 'hidden')}>
                      <TabBody tab={tab} />
                    </div>
                  ))}
                  {!active && (
                    <div className="flex h-full flex-col items-start justify-center gap-3 p-10">
                      <h1 className="text-xl font-bold">Work with your data, with a check before every change.</h1>
                      <p className="max-w-prose text-sm text-muted">
                        Connect to a database from the panel on the left, then open a table or write a query. Press Ctrl+K
                        for commands.
                      </p>
                      <Button tone="primary" onClick={newQuery}>
                        New query
                      </Button>
                    </div>
                  )}
                </main>
              </div>
            </Panel>
          </Group>
        </div>
      </div>
      {palette && <Palette onClose={() => setPalette(false)} actions={paletteActions} />}
    </div>
  );
}
