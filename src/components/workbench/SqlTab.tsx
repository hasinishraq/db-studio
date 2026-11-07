import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Group, Panel, Separator } from 'react-resizable-panels';
import { Play, Square } from 'lucide-react';
import { api, ApiError, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { ColumnInfo, ExportFormat, TxState } from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import { ResultStore } from '../../lib/resultStore';
import { Button, ErrorNote, Select } from '../ui';
import { PlanView } from './PlanView';
import { ResultsGrid } from './ResultsGrid';
import { SqlEditor, type EditorHandle } from './SqlEditor';

type SqlTabData = Extract<Tab, { kind: 'sql' }>;

const noSubscribe = () => () => {};
const noSnapshot = () => 0;

function useSchemaColumns(session: string | undefined, profile: string | null): ColumnInfo[] {
  const { data = [] } = useQuery({
    queryKey: ['schema', profile, 'columns'],
    enabled: Boolean(session),
    queryFn: async () => {
      if (!session) return [];
      const schemas = await api.listSchemas(session);
      const lists = await Promise.all(schemas.map((schema) => api.columns(session, schema)));
      return lists.flat();
    },
  });
  return data;
}

function TransactionBar({ session }: { session: string }) {
  const [state, setState] = useState<TxState>({ mode: 'Auto', dirty: false });
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void api.txState(session).then(setState);
    const refresh = () => void api.txState(session).then(setState);
    window.addEventListener('studio:run-finished', refresh);
    return () => window.removeEventListener('studio:run-finished', refresh);
  }, [session]);
  const act = async (work: () => Promise<TxState>) => {
    setError(null);
    try {
      setState(await work());
    } catch (e) {
      setError(e instanceof ApiError && e.code === 'DirtyTransaction' ? 'Commit or roll back first.' : errorMessage(e));
    }
  };
  return (
    <div className="flex items-center gap-1">
      <Select
        aria-label="Commit mode"
        className="w-28"
        value={state.mode}
        onChange={(e) => void act(() => api.setTxMode(session, e.target.value as TxState['mode']))}
      >
        <option value="Auto">Auto commit</option>
        <option value="Manual">Manual</option>
      </Select>
      {state.mode === 'Manual' && (
        <>
          {state.dirty && <span className="text-sm text-change">Uncommitted changes</span>}
          <Button onClick={() => void act(() => api.commitTx(session))}>Commit</Button>
          <Button onClick={() => void act(() => api.rollbackTx(session))}>Roll back</Button>
        </>
      )}
      {error && <span className="text-sm text-danger">{error}</span>}
    </div>
  );
}

/** A byte offset into the text, from a 1-based character position the server reported. */
function charOffset(text: string, bytes: number): number {
  return new TextDecoder().decode(new TextEncoder().encode(text).slice(0, bytes)).length;
}

export function SqlTab({ tab }: { tab: SqlTabData }) {
  const { profiles, sessions, targetFor, confirm, updateTab } = useStudio();
  const client = useQueryClient();
  const [sql, setSql] = useState(tab.sql);
  const [store, setStore] = useState<ResultStore | null>(null);
  const [active, setActive] = useState(0);
  const [log, setLog] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [plan, setPlan] = useState<string | null>(null);
  const [draftId] = useState(tab.draftId ?? newId());
  const [saved, setSaved] = useState(tab.sql);
  const editor = useRef<EditorHandle | null>(null);
  const session = tab.profileId ? sessions[tab.profileId]?.sessionId : undefined;
  const columns = useSchemaColumns(session, tab.profileId);
  const target = targetFor(tab.profileId);

  // Autosave, so a draft survives a restart.
  useEffect(() => {
    const timer = setTimeout(() => {
      if (sql === saved || !sql.trim()) return;
      void api.draftsSave({
        id: draftId,
        profileId: tab.profileId,
        title: sql.trim().slice(0, 60),
        sql,
        savedSql: sql,
        updatedAt: Date.now(),
      });
      setSaved(sql);
      void client.invalidateQueries({ queryKey: ['drafts'] });
    }, 2000);
    return () => clearTimeout(timer);
  }, [sql, saved, draftId, tab.profileId, client]);

  const addLog = (line: string) => setLog((lines) => [...lines, line]);

  const run = async () => {
    if (!target) return;
    const text = editor.current?.selection() || sql;
    const next = new ResultStore();
    const id = newId();
    setStore(next);
    setActive(0);
    setPlan(null);
    setError(null);
    setRunId(id);
    editor.current?.clearMarks();
    try {
      const result = await runFlow({ target, sql: text, origin: 'Editor', confirm, runId: id, store: next });
      if (result.status === 'declined') {
        addLog('Statement not run: confirmation declined');
        return;
      }
      result.outcomes.forEach((outcome, index) => {
        const span = result.verdicts[index]?.span;
        if (outcome.kind === 'command') {
          addLog(`${outcome.rowsAffected} rows affected in ${outcome.elapsedMs} ms`);
        } else if (outcome.kind === 'error') {
          addLog(outcome.error.message);
          setError(outcome.error.message);
          if (span) {
            const position = outcome.error.detail?.position;
            const start = position ? span.start + position - 1 : span.start;
            editor.current?.markError(charOffset(text, start), charOffset(text, span.end), outcome.error.message);
          }
        }
      });
      // A structure change means the explorer and completion are out of date.
      if (result.verdicts.some((v) => v.kind === 'Ddl')) {
        void client.invalidateQueries({ queryKey: ['schema', tab.profileId] });
      }
      void client.invalidateQueries({ queryKey: ['history'] });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunId(null);
      window.dispatchEvent(new Event('studio:run-finished'));
    }
  };

  const stop = () => {
    if (runId) void api.cancelRun(runId);
  };

  const explain = async () => {
    if (!session) return;
    setError(null);
    try {
      setPlan(await api.explain(session, editor.current?.selection() || sql));
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const exportResults = async (format: ExportFormat) => {
    if (!session) return;
    setError(null);
    try {
      const rows = await api.exportQuery(session, editor.current?.selection() || sql, format, newId(), () => {});
      addLog(`Exported ${rows.toLocaleString()} rows`);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === '.') stop();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  // Rows keep arriving during a run, so this must re-read the store on every update.
  useSyncExternalStore(store?.subscribe ?? noSubscribe, store?.snapshot ?? noSnapshot);
  const statements = store ? [...store.statements.entries()] : [];
  const withRows = statements.filter(([, s]) => s.columns.length > 0);
  const current = withRows[active]?.[1] ?? withRows[0]?.[1];

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-2 py-1">
        <Select
          aria-label="Connection"
          className="w-44"
          value={tab.profileId ?? ''}
          onChange={(e) => updateTab(tab.id, { profileId: e.target.value || null })}
        >
          <option value="">Choose a connection</option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id} disabled={!sessions[p.id]}>
              {p.name}
            </option>
          ))}
        </Select>
        {runId ? (
          <Button tone="danger" onClick={stop}>
            <Square size={13} aria-hidden /> Stop
          </Button>
        ) : (
          <Button tone="primary" disabled={!target} onClick={() => void run()} title="Ctrl+Enter">
            <Play size={13} aria-hidden /> Run
          </Button>
        )}
        <Button disabled={!session} onClick={() => void explain()}>
          Explain
        </Button>
        <Select
          aria-label="Export results"
          className="w-28"
          value=""
          disabled={!session}
          onChange={(e) => void exportResults(e.target.value as ExportFormat)}
        >
          <option value="">Export</option>
          <option value="Csv">CSV</option>
          <option value="JsonLines">JSON lines</option>
          <option value="Xlsx">Excel</option>
          <option value="SqlInserts">SQL inserts</option>
        </Select>
        {session && <TransactionBar session={session} />}
      </div>
      <Group orientation="vertical" className="min-h-0 flex-1">
        <Panel defaultSize="45%" minSize="15%">
          <SqlEditor value={sql} onChange={setSql} onRun={() => void run()} columns={columns} handle={editor} />
        </Panel>
        <Separator className="h-1 bg-line" />
        <Panel minSize="15%">
          <div className="flex h-full flex-col">
            {withRows.length > 1 && (
              <div className="flex gap-1 border-b border-line px-2">
                {withRows.map(([index], position) => (
                  <button
                    key={index}
                    type="button"
                    onClick={() => setActive(position)}
                    className={`px-2 py-1 text-sm ${position === active ? 'border-b-2 border-ink' : 'text-muted'}`}
                  >
                    Result {position + 1}
                  </button>
                ))}
              </div>
            )}
            <ErrorNote message={error} />
            <div className="min-h-0 flex-1">
              {plan !== null ? (
                <PlanView text={plan} />
              ) : store && current ? (
                <ResultsGrid store={store} statement={current} onStop={runId ? stop : undefined} />
              ) : (
                <p className="p-3 text-sm text-muted">
                  {target ? 'Results appear here. Press Ctrl+Enter to run.' : 'Connect to a database to run this query.'}
                </p>
              )}
            </div>
            {log.length > 0 && (
              <div className="max-h-24 overflow-auto border-t border-line px-2 py-1 font-mono text-xs text-muted">
                {log.map((line, index) => (
                  <div key={index}>{line}</div>
                ))}
              </div>
            )}
          </div>
        </Panel>
      </Group>
    </div>
  );
}
