import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronDown, ChevronRight, Database, Eye, Pencil, Plug, Plus, Table, Trash2, Unplug } from 'lucide-react';
import { api, ApiError, errorMessage } from '../../api';
import type { ConnectionProfile, DbObject, HostKeyDetail, SessionId } from '../../bindings';
import { newId, useStudio } from '../../context/Studio';
import { blankProfile, ConnectionDialog, HostKeyPrompt } from '../modals/ConnectionDialog';
import { CreateTableDialog } from '../modals/CreateTableDialog';
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
      className="flex w-full items-center gap-1.5 rounded py-0.5 pl-9 pr-2 text-left text-sm hover:bg-raised"
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
}: {
  session: SessionId;
  schema: string;
  profile: ConnectionProfile;
  search: string;
}) {
  const { openTab } = useStudio();
  const [expanded, setExpanded] = useState(false);
  const [creating, setCreating] = useState(false);
  const searching = search.trim().length > 0;
  const { data: objects = [], error } = useQuery({
    queryKey: ['schema', profile.id, 'objects', schema],
    queryFn: () => api.listObjects(session, { name: schema }),
    enabled: expanded || searching,
  });
  const shown = objects.filter((object) => matches(object.name, search));
  const Chevron = expanded || searching ? ChevronDown : ChevronRight;
  return (
    <div>
      <div className="flex items-center">
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
          className="flex min-w-0 flex-1 items-center gap-1 py-0.5 pl-5 text-sm hover:bg-raised"
        >
          <Chevron size={13} aria-hidden />
          <span className="truncate">{schema}</span>
        </button>
        <button type="button" className="px-1 text-xs text-muted hover:text-ink" onClick={() => setCreating(true)}>
          New table
        </button>
        <button
          type="button"
          className="px-2 text-xs text-muted hover:text-ink"
          onClick={() =>
            openTab({ id: `er:${profile.id}:${schema}`, kind: 'er', title: `${schema} diagram`, profileId: profile.id, schema })
          }
        >
          Diagram
        </button>
      </div>
      {error && <ErrorNote message={errorMessage(error)} />}
      {(expanded || searching) && shown.map((object) => <ObjectRow key={object.name} object={object} profile={profile} />)}
      {creating && <CreateTableDialog profileId={profile.id} schema={schema} onClose={() => setCreating(false)} />}
    </div>
  );
}

function ConnectionNode({ profile, search, onEdit }: { profile: ConnectionProfile; search: string; onEdit: () => void }) {
  const { sessions, setSession, openTab } = useStudio();
  const client = useQueryClient();
  const session = sessions[profile.id];
  const [state, setState] = useState<'offline' | 'connecting' | 'failed'>('offline');
  const [error, setError] = useState<string | null>(null);
  const [hostKey, setHostKey] = useState<{ detail: HostKeyDetail; changed: boolean } | null>(null);
  const { data: schemas = [] } = useQuery({
    queryKey: ['schema', profile.id, 'list'],
    queryFn: () => api.listSchemas(session?.sessionId ?? ''),
    enabled: Boolean(session),
  });

  const connect = async () => {
    setState('connecting');
    setError(null);
    try {
      setSession(profile.id, await api.connect(profile.id));
      setState('offline');
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
      <div className="group flex items-center gap-1 px-2 py-1">
        <Database
          size={14}
          aria-hidden
          className={session ? 'text-safe' : state === 'failed' ? 'text-danger' : 'text-muted'}
        />
        <span className="min-w-0 flex-1 truncate text-sm font-bold">{profile.name}</span>
        {state === 'connecting' && <span className="text-xs text-muted">Connecting</span>}
        <span className="flex gap-1 opacity-0 focus-within:opacity-100 group-hover:opacity-100">
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
      {session && (
        <Button
          className="ml-5"
          onClick={() =>
            openTab({ id: newId(), kind: 'sql', title: 'New query', profileId: profile.id, draftId: null, sql: '' })
          }
        >
          New query
        </Button>
      )}
      <div className="px-2">
        <ErrorNote message={error} />
        {hostKey && <HostKeyPrompt {...hostKey} onTrust={() => void trust()} onCancel={() => setHostKey(null)} />}
      </div>
      {session &&
        schemas.map((schema) => (
          <SchemaNode key={schema.name} session={session.sessionId} schema={schema.name} profile={profile} search={search} />
        ))}
    </div>
  );
}

export function Explorer() {
  const { profiles } = useStudio();
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<ConnectionProfile | null>(null);

  useEffect(() => {
    const open = () => setEditing(blankProfile());
    window.addEventListener('studio:new-connection', open);
    return () => window.removeEventListener('studio:new-connection', open);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <div className="flex gap-1 p-2">
        <Input aria-label="Search tables" placeholder="Search tables" value={search} onChange={(e) => setSearch(e.target.value)} />
        <Button aria-label="New connection" onClick={() => setEditing(blankProfile())}>
          <Plus size={14} />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {profiles.length === 0 && (
          <div className="p-3 text-sm text-muted">
            <p className="mb-2">Add a connection to browse a database.</p>
            <Button tone="primary" onClick={() => setEditing(blankProfile())}>
              Add a connection
            </Button>
          </div>
        )}
        {profiles.map((profile) => (
          <ConnectionNode key={profile.id} profile={profile} search={search} onEdit={() => setEditing(profile)} />
        ))}
      </div>
      {editing && <ConnectionDialog initial={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}
