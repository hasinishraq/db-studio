import { usePrefs } from '../../api/settings';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Database,
  Eye,
  GripVertical,
  Grid2x2Plus,
  Network,
  Pencil,
  Plug,
  Plus,
  Table,
  Trash2,
  Unplug,
} from 'lucide-react';
import { api, ApiError, errorMessage } from '../../api';
import type { ConnectionProfile, DbObject, HostKeyDetail, SessionId } from '../../bindings';
import { newId, useStudio } from '../../context/Studio';
import { blankProfile, ConnectionDialog, HostKeyPrompt } from '../modals/ConnectionDialog';
import { CreateDatabaseDialog } from '../modals/CreateDatabaseDialog';
import { CreateTableDialog } from '../modals/CreateTableDialog';
import { inOrder, loadCollapsed, loadOrder, moveTo, saveCollapsed, saveOrder } from '../../lib/explorerPrefs';
import { Button, ErrorNote, Input } from '../ui';

export function matches(text: string, search: string): boolean {
  return text.toLowerCase().includes(search.trim().toLowerCase());
}

function ObjectRow({ object, profile }: { object: DbObject; profile: ConnectionProfile }) {
  const { openTab } = useStudio();
  const Icon = object.kind === 'Table' ? Table : Eye;
  const open = () =>
    openTab({
      id: `table:${profile.id}:${object.schema}:${object.name}`,
      kind: 'table',
      title: object.name,
      profileId: profile.id,
      table: { schema: object.schema, name: object.name },
    });
  return (
    <button
      type="button"
      onClick={open}
      className="flex w-full items-center gap-1.5 rounded-r py-1 pl-3 pr-2 text-left text-sm hover:bg-raised"
    >
      <Icon size={13} className="shrink-0 text-muted" aria-hidden />
      <span className="truncate">{object.name}</span>
    </button>
  );
}

function SchemaNode({
  session,
  schema,
  profile,
  search,
  chosen = false,
  onChoose,
  nested = false,
}: {
  session: SessionId;
  schema: string;
  profile: ConnectionProfile;
  search: string;
  chosen?: boolean;
  onChoose?: () => void;
  nested?: boolean;
}) {
  // Under a PostgreSQL database the schemas sit one level deeper.
  const indent = nested ? 'pl-3' : 'pl-5';
  const guide = nested ? 'ml-5' : 'ml-7';
  const { openTab } = useStudio();
  const [expanded, setExpanded] = useState(false);
  const [creating, setCreating] = useState(false);
  const searching = search.trim().length > 0;
  const { data: objects = [], error } = useQuery({
    queryKey: ['schema', profile.id, 'objects', session, schema],
    queryFn: () => api.listObjects(session, { name: schema }),
    enabled: expanded || searching,
  });
  const shown = objects.filter((object) => matches(object.name, search));
  const Chevron = expanded || searching ? ChevronDown : ChevronRight;
  return (
    <div>
      <div className={`group flex items-center gap-1 pr-1 ${chosen ? 'here' : 'hover:bg-raised'}`}>
        <button
          type="button"
          aria-expanded={expanded}
          aria-current={chosen || undefined}
          data-level={nested ? 'schema' : 'database'}
          title={onChoose && !chosen ? `Use this ${nested ? 'schema' : 'database'}` : undefined}
          onClick={() => {
            setExpanded(onChoose && !chosen ? true : !expanded);
            onChoose?.();
          }}
          className={`flex min-w-0 flex-1 items-center gap-1 py-1 text-sm ${indent}`}
        >
          <Chevron size={13} aria-hidden className="shrink-0 text-muted" />
          <span className={`truncate ${chosen ? 'font-bold' : ''}`}>{schema}</span>
        </button>
        {chosen && <span className="shrink-0 text-xs text-muted">in use</span>}
        <Button tone="quiet" className="p-1" title={`New table in ${schema}`} onClick={() => setCreating(true)}>
          <Grid2x2Plus size={14} aria-hidden />
          <span className="sr-only">New table</span>
        </Button>
        <Button
          tone="quiet"
          className="p-1"
          title={`Diagram of ${schema}`}
          onClick={() =>
            openTab({ id: `er:${profile.id}:${schema}`, kind: 'er', title: `${schema} diagram`, profileId: profile.id, schema })
          }
        >
          <Network size={14} aria-hidden />
          <span className="sr-only">Diagram</span>
        </Button>
      </div>
      {error && <ErrorNote message={errorMessage(error)} />}
      {(expanded || searching) && (
        <div className={`border-l border-line ${guide}`}>
          {shown.map((object) => (
            <ObjectRow key={object.name} object={object} profile={profile} />
          ))}
          {shown.length === 0 && <p className="py-1 pl-3 text-xs text-muted">{searching ? 'No match' : 'No tables'}</p>}
        </div>
      )}
      {creating && <CreateTableDialog profileId={profile.id} schema={schema} onClose={() => setCreating(false)} />}
    </div>
  );
}

function ConnectionNode({
  profile,
  search,
  onEdit,
  collapsed,
  onToggle,
  grip,
}: {
  profile: ConnectionProfile;
  search: string;
  onEdit: () => void;
  collapsed: boolean;
  onToggle: () => void;
  grip: ReactNode;
}) {
  const { sessions, setSession, openTab, tabs, closeTab } = useStudio();
  const client = useQueryClient();
  const session = sessions[profile.id];
  const current = session?.database ?? null;
  const separateDatabases = profile.engine === 'Postgres';
  const prefs = usePrefs();
  const [creatingDatabase, setCreatingDatabase] = useState(false);
  const [databaseOpen, setDatabaseOpen] = useState(true);
  // A search shows matches even inside a collapsed connection.
  const open = !collapsed || search.trim().length > 0;
  const [state, setState] = useState<'offline' | 'connecting' | 'failed'>('offline');
  const [error, setError] = useState<string | null>(null);
  const [hostKey, setHostKey] = useState<{ detail: HostKeyDetail; changed: boolean } | null>(null);
  const { data: schemas = [], isSuccess: schemasLoaded } = useQuery({
    queryKey: ['schema', profile.id, 'list', session?.sessionId],
    queryFn: () => api.listSchemas(session?.sessionId ?? ''),
    enabled: Boolean(session),
  });
  const { data: databases = [] } = useQuery({
    queryKey: ['schema', profile.id, 'databases', session?.sessionId],
    queryFn: () => api.listDatabases(session?.sessionId ?? ''),
    enabled: Boolean(session) && separateDatabases,
  });

  // A MySQL database in use that has been dropped is no longer in use.
  useEffect(() => {
    if (session && profile.engine === 'MySql' && current && schemasLoaded && !schemas.some((s) => s.name === current)) {
      setSession(profile.id, { ...session, database: null });
    }
  }, [session, profile.engine, profile.id, current, schemas, schemasLoaded, setSession]);

  const choose = async (database: string) => {
    if (!session || database === current) return;
    setError(null);
    try {
      const info = await api.switchDatabase(session.sessionId, database);
      // These tabs name objects in the database just left.
      if (separateDatabases) {
        tabs
          .filter((tab) => (tab.kind === 'table' || tab.kind === 'er') && tab.profileId === profile.id)
          .forEach((tab) => closeTab(tab.id));
      }
      setSession(profile.id, info);
    } catch (e) {
      setError(
        e instanceof ApiError && e.code === 'DirtyTransaction'
          ? 'Commit or roll back first. Then change database.'
          : errorMessage(e),
      );
    }
  };

  const chooseSchema = async (schema: string) => {
    if (!session || schema === session.schema) return;
    setError(null);
    try {
      setSession(profile.id, await api.switchSchema(session.sessionId, schema));
    } catch (e) {
      setError(
        e instanceof ApiError && e.code === 'DirtyTransaction'
          ? 'Commit or roll back first. Then change schema.'
          : errorMessage(e),
      );
    }
  };

  const schemaNodes = (selectable: boolean, nested = false) =>
    session &&
    schemas.map((schema) => (
      <SchemaNode
        key={schema.name}
        session={session.sessionId}
        schema={schema.name}
        profile={profile}
        search={search}
        chosen={selectable && schema.name === (nested ? session.schema : current)}
        onChoose={selectable ? () => void (nested ? chooseSchema(schema.name) : choose(schema.name)) : undefined}
        nested={nested}
      />
    ));

  const connect = async () => {
    setState('connecting');
    setError(null);
    try {
      setSession(profile.id, await api.connect(profile.id));
      setState('offline');
      if (prefs.openQueryOnConnect) {
        openTab({ id: newId(), kind: 'sql', title: 'New query', profileId: profile.id, draftId: null, sql: '' });
      }
    } catch (e) {
      setState('failed');
      if (e instanceof ApiError && e.hostKey) {
        setHostKey({ detail: e.hostKey, changed: e.code === 'HostKeyChanged' });
      } else setError(errorMessage(e));
    }
  };

  const disconnect = async () => {
    if (!session) return;
    try {
      await api.disconnect(session.sessionId, false);
      setSession(profile.id, null);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'DirtyTransaction') {
        setError('Commit or roll back first. This connection has uncommitted changes.');
      } else setError(errorMessage(e));
    }
  };

  const remove = async () => {
    await api.deleteProfile(profile.id);
    await client.invalidateQueries({ queryKey: ['profiles'] });
  };

  const trust = async () => {
    if (!hostKey) return;
    await api.trustHostKey(profile.id, hostKey.detail.keyType ?? 'unknown', hostKey.detail.fingerprint, hostKey.changed);
    setHostKey(null);
    await connect();
  };

  return (
    <div>
      <div data-connection={profile.id} className="group relative flex items-center gap-1 py-1 pl-0.5 pr-2 hover:bg-raised">
        {grip}
        <button
          type="button"
          aria-expanded={open}
          aria-label={`${open ? 'Collapse' : 'Expand'} ${profile.name}`}
          title={open ? 'Collapse' : 'Expand'}
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-1 text-left"
        >
          {open ? (
            <ChevronDown size={13} aria-hidden className="shrink-0 text-muted" />
          ) : (
            <ChevronRight size={13} aria-hidden className="shrink-0 text-muted" />
          )}
          <Database
            size={14}
            aria-hidden
            className={`shrink-0 ${session ? 'text-safe' : state === 'failed' ? 'text-danger' : 'text-muted'}`}
          />
          <span className="min-w-0 truncate font-bold">{profile.name}</span>
          {!open && current && <span className="min-w-0 truncate text-xs text-muted">{current}</span>}
        </button>
        {session && <span className="text-xs text-safe">Connected</span>}
        {state === 'connecting' && <span className="text-xs text-muted">Connecting</span>}
        {/* Over the end of the row, so hidden actions do not cut the name short. */}
        <span className="absolute right-1 flex gap-0.5 rounded bg-raised opacity-0 focus-within:opacity-100 group-hover:opacity-100 [&>button]:rounded [&>button]:p-1 [&>button:hover]:bg-pick">
          {session ? (
            <button type="button" aria-label={`Disconnect ${profile.name}`} onClick={() => void disconnect()}>
              <Unplug size={14} />
            </button>
          ) : (
            <button type="button" aria-label={`Connect ${profile.name}`} onClick={() => void connect()}>
              <Plug size={14} />
            </button>
          )}
          <button type="button" aria-label={`Edit ${profile.name}`} onClick={onEdit}>
            <Pencil size={14} />
          </button>
          <button type="button" aria-label={`Delete ${profile.name}`} onClick={() => void remove()}>
            <Trash2 size={14} />
          </button>
        </span>
      </div>
      {open && session && (
        <div className="mb-1 ml-5 flex flex-wrap gap-1">
          <Button
            className="py-0.5 text-xs"
            onClick={() =>
              openTab({ id: newId(), kind: 'sql', title: 'New query', profileId: profile.id, draftId: null, sql: '' })
            }
          >
            <Plus size={12} aria-hidden /> New query
          </Button>
          {profile.engine !== 'Sqlite' && (
            <Button className="py-0.5 text-xs" onClick={() => setCreatingDatabase(true)}>
              <Plus size={12} aria-hidden /> New database
            </Button>
          )}
        </div>
      )}
      {creatingDatabase && (
        <CreateDatabaseDialog
          profileId={profile.id}
          engine={profile.engine}
          onCreated={async (name, useNow) => {
            if (useNow) await choose(name);
          }}
          onClose={() => setCreatingDatabase(false)}
        />
      )}
      <div className="px-2">
        <ErrorNote message={error} />
        {hostKey && <HostKeyPrompt {...hostKey} onTrust={() => void trust()} onCancel={() => setHostKey(null)} />}
      </div>
      {!open
        ? null
        : separateDatabases && databases.length > 0
        ? databases.map((database) => (
            <div key={database}>
              <div className={`flex items-center pr-2 ${database === current ? 'here' : 'hover:bg-raised'}`}>
                <button
                  type="button"
                  aria-current={database === current || undefined}
                  data-level="database"
                  aria-expanded={database === current ? databaseOpen : undefined}
                  title={database === current ? (databaseOpen ? 'Collapse' : 'Expand') : 'Use this database'}
                  onClick={() => {
                    if (database === current) setDatabaseOpen(!databaseOpen);
                    else {
                      setDatabaseOpen(true);
                      void choose(database);
                    }
                  }}
                  className={`flex min-w-0 flex-1 items-center gap-1 py-1 pl-3 text-left text-sm ${
                    database === current ? 'font-bold' : ''
                  }`}
                >
                  {database === current && databaseOpen ? (
                    <ChevronDown size={13} aria-hidden className="shrink-0 text-muted" />
                  ) : (
                    <ChevronRight size={13} aria-hidden className="shrink-0 text-muted" />
                  )}
                  <Database size={13} aria-hidden className="shrink-0 text-muted" />
                  <span className="truncate">{database}</span>
                </button>
                {database === current && <span className="shrink-0 text-xs text-muted">in use</span>}
              </div>
              {database === current && databaseOpen && (
                <div className="ml-5 border-l border-line">{schemaNodes(true, true)}</div>
              )}
            </div>
          ))
        : schemaNodes(profile.engine === 'MySql')}
    </div>
  );
}

export function Explorer() {
  const { profiles } = useStudio();
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<ConnectionProfile | null>(null);
  const [order, setOrder] = useState(loadOrder);
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [dragging, setDragging] = useState<{ id: string; to: number } | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const ordered = inOrder(profiles, order);

  useEffect(() => {
    const open = () => setEditing(blankProfile());
    window.addEventListener('studio:new-connection', open);
    return () => window.removeEventListener('studio:new-connection', open);
  }, []);

  const place = (id: string, to: number) => {
    const next = moveTo(
      ordered.map((p) => p.id),
      id,
      to,
    );
    setOrder(next);
    saveOrder(next);
  };

  const toggle = (id: string) => {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setCollapsed(next);
    saveCollapsed(next);
  };

  const setAll = (closed: boolean) => {
    const next = new Set(closed ? profiles.map((p) => p.id) : []);
    setCollapsed(next);
    saveCollapsed(next);
  };

  /** Where a pointer at this height would drop: before the first header whose middle is below it. */
  const slotAt = (y: number) => {
    const headers = [...(list.current?.querySelectorAll('[data-connection]') ?? [])];
    const index = headers.findIndex((header) => {
      const box = header.getBoundingClientRect();
      return y < box.top + box.height / 2;
    });
    return index < 0 ? headers.length : index;
  };

  const startDrag = (id: string, event: React.PointerEvent) => {
    event.preventDefault();
    setDragging({ id, to: slotAt(event.clientY) });
    const move = (e: PointerEvent) => setDragging({ id, to: slotAt(e.clientY) });
    const up = (e: PointerEvent) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      place(id, slotAt(e.clientY));
      setDragging(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const grip = (profile: ConnectionProfile, index: number) => (
    <button
      type="button"
      aria-label={`Move ${profile.name}`}
      title="Drag to reorder, or press Alt+Up and Alt+Down"
      className="cursor-grab touch-none rounded p-0.5 text-muted opacity-60 hover:bg-pick hover:text-ink hover:opacity-100 focus:opacity-100 group-hover:opacity-100"
      onPointerDown={(event) => startDrag(profile.id, event)}
      onKeyDown={(event) => {
        if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
        event.preventDefault();
        place(profile.id, event.key === 'ArrowUp' ? index - 1 : index + 2);
      }}
    >
      <GripVertical size={14} aria-hidden />
    </button>
  );

  const marker = <div aria-hidden className="mx-2 h-0.5 rounded bg-ink" />;
  return (
    <div className="flex h-full flex-col">
      <div className="flex gap-1 p-2">
        <Input aria-label="Search tables" placeholder="Search tables" value={search} onChange={(e) => setSearch(e.target.value)} />
        <Button aria-label="New connection" title="New connection" onClick={() => setEditing(blankProfile())}>
          <Plus size={14} />
        </Button>
      </div>
      {profiles.length > 1 && (
        <div className="flex items-center justify-end gap-1 whitespace-nowrap px-2 pb-1">
          <Button tone="quiet" className="shrink-0 px-1.5 py-0.5 text-xs" title="Collapse every connection" onClick={() => setAll(true)}>
            <ChevronsDownUp size={13} aria-hidden /> Collapse all
          </Button>
          <Button tone="quiet" className="shrink-0 px-1.5 py-0.5 text-xs" title="Expand every connection" onClick={() => setAll(false)}>
            <ChevronsUpDown size={13} aria-hidden /> Expand all
          </Button>
        </div>
      )}
      <div ref={list} className={`min-h-0 flex-1 overflow-auto ${dragging ? 'cursor-grabbing select-none' : ''}`}>
        {profiles.length === 0 && (
          <div className="p-3 text-sm text-muted">
            <p className="mb-2">Add a connection to browse a database.</p>
            <Button tone="primary" onClick={() => setEditing(blankProfile())}>
              Add a connection
            </Button>
          </div>
        )}
        {ordered.map((profile, index) => (
          <div key={profile.id} className={dragging?.id === profile.id ? 'opacity-50' : ''}>
            {dragging && dragging.to === index && marker}
            <div className="border-b border-line/60">
              <ConnectionNode
                profile={profile}
                search={search}
                onEdit={() => setEditing(profile)}
                collapsed={collapsed.has(profile.id)}
                onToggle={() => toggle(profile.id)}
                grip={grip(profile, index)}
              />
            </div>
          </div>
        ))}
        {dragging && dragging.to === ordered.length && marker}
      </div>
      {editing && <ConnectionDialog initial={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}
