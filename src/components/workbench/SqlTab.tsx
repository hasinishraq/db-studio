import { EXPORT_LABELS, usePrefs } from '../../api/settings';
import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Group, Panel, Separator } from 'react-resizable-panels';
import { Play, Square, X } from 'lucide-react';
import { api, ApiError, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { ColumnInfo, ExportFormat, TxState } from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import { ResultStore } from '../../lib/resultStore';
import { charOffset, rowCount, spanText } from '../../lib/sqlText';
import { Button, ErrorNote, Select } from '../ui';
import { PlanView } from './PlanView';
import { ResultsGrid, type GridPick } from './ResultsGrid';
import { useRunLog, WithLog } from './RunLog';
import { SqlEditor, type EditorHandle } from './SqlEditor';

type SqlTabData = Extract<Tab, { kind: 'sql' }>;

const noSubscribe = () => () => {};
const noSnapshot = () => 0;

function useSchemaColumns(session: string | undefined, profile: string | null): ColumnInfo[] {
  const { data = [] } = useQuery({
    queryKey: ['schema', profile, 'columns', session],
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

export function SqlTab({ tab }: { tab: SqlTabData }) {
  const { profiles, sessions, targetFor, confirm, updateTab } = useStudio();
  const client = useQueryClient();
  const [sql, setSql] = useState(tab.sql);
  const [store, setStore] = useState<ResultStore | null>(null);
  const [active, setActive] = useState(0);
  const log = useRunLog();
  const prefs = usePrefs();
  // The last rows shown stay on screen when a later statement returns none.
  const [kept, setKept] = useState<{ store: ResultStore; sql: string } | null>(null);
  const [stale, setStale] = useState(false);
  // Closing hides the results and the log until the next run.
  const [resultsOpen, setResultsOpen] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [plan, setPlan] = useState<string | null>(null);
  const [draftId] = useState(tab.draftId ?? newId());
  const [saved, setSaved] = useState(tab.sql);
  const editor = useRef<EditorHandle | null>(null);
  const gridPick = useRef<(() => GridPick | null) | null>(null);
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

  const run = async (override?: string) => {
    if (!target) return;
    const text = override ?? (editor.current?.selection() || sql);
    const next = new ResultStore();
    const id = newId();
    setResultsOpen(true);
    setStore(next);
    setActive(0);
    setPlan(null);
    setError(null);
    setRunId(id);
    editor.current?.clearMarks();
    try {
      const result = await runFlow({ target, sql: text, origin: 'Editor', confirm, runId: id, store: next });
      if (result.status === 'declined') {
        log.add('info', 'Statement not run: confirmation declined', text.trim());
        return;
      }
      result.outcomes.forEach((outcome, index) => {
        const verdict = result.verdicts[index];
        const span = verdict?.span;
        const statement = span ? spanText(text, span) : text;
        if (outcome.kind === 'rows') {
          const capped = outcome.truncated ? ', stopped at the limit' : '';
          log.add('info', `${rowCount(outcome.totalRows)} returned in ${outcome.elapsedMs} ms${capped}`, statement);
        } else if (outcome.kind === 'command') {
          const tone = verdict?.readOnly ? 'info' : 'change';
          log.add(tone, `${rowCount(outcome.rowsAffected)} affected in ${outcome.elapsedMs} ms`, statement);
        } else if (outcome.kind === 'error') {
          log.add('error', outcome.error.message, statement);
          setError(outcome.error.message);
          if (span) {
            const position = outcome.error.detail?.position;
            const start = position ? span.start + position - 1 : span.start;
            editor.current?.markError(charOffset(text, start), charOffset(text, span.end), outcome.error.message);
          }
        }
      });
      if ([...next.statements.values()].some((s) => s.columns.length > 0)) {
        // Refresh repeats only what Rust called a read, never a write that ran alongside it.
        const reads = result.verdicts.filter((v) => v.readOnly).map((v) => spanText(text, v.span).replace(/;\s*$/, ''));
        setKept({ store: next, sql: reads.join(';\n') });
        setStale(false);
      } else if (result.verdicts.some((v) => !v.readOnly)) {
        setStale(true);
      }
      // A structure change means the explorer and completion are out of date.
      if (result.verdicts.some((v) => v.kind === 'Ddl')) {
        void client.invalidateQueries({ queryKey: ['schema', tab.profileId] });
      }
      void client.invalidateQueries({ queryKey: ['history'] });
    } catch (e) {
      if (e instanceof ApiError && e.code === 'Cancelled') log.add('info', 'Stopped', text.trim());
      else {
        log.add('error', errorMessage(e), text.trim());
        setError(errorMessage(e));
      }
    } finally {
      setRunId(null);
      window.dispatchEvent(new Event('studio:run-finished'));
    }
  };

  const closeResults = () => {
    setResultsOpen(false);
    setStore(null);
    setKept(null);
    setStale(false);
    setPlan(null);
    setError(null);
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
      // Rows narrowed in the grid export as they are; otherwise the query runs again in full.
      const picked = gridPick.current?.();
      if (picked) {
        const rows = await api.exportRows(session, picked.columns, picked.rows, format, null);
        log.add('info', `Exported ${rowCount(rows)} (${picked.kind})`);
      } else {
        const rows = await api.exportQuery(session, editor.current?.selection() || sql, format, newId(), () => {});
        log.add('info', `Exported ${rowCount(rows)}`);
      }
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
  const rowsOf = (from: ResultStore | null | undefined) =>
    from ? [...from.statements.entries()].filter(([, s]) => s.columns.length > 0) : [];
  const fresh = rowsOf(store);
  const showing = fresh.length > 0 ? store : (kept?.store ?? null);
  const withRows = fresh.length > 0 ? fresh : rowsOf(kept?.store);
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
        {tab.profileId && sessions[tab.profileId]?.database && (
          <span
            className="flex items-center gap-1 rounded border border-line px-2 py-0.5 text-sm"
            title="Queries run in this database"
          >
            <span className="text-muted">in</span>
            <span className="font-bold">{sessions[tab.profileId]?.database}</span>
            {sessions[tab.profileId]?.schema && (
              <>
                <span className="ml-1 border-l border-line pl-2 text-muted">schema</span>
                <span className="font-bold">{sessions[tab.profileId]?.schema}</span>
              </>
            )}
          </span>
        )}
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
        <Button disabled={!session} title="Change the format in Settings" onClick={() => void exportResults(prefs.exportFormat)}>
          Export as {EXPORT_LABELS[prefs.exportFormat]}
        </Button>
        <Select
          aria-label="Export results"
          className="w-32"
          value=""
          disabled={!session}
          onChange={(e) => void exportResults(e.target.value as ExportFormat)}
        >
          <option value="">Other format</option>
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
        {resultsOpen && <Separator aria-label="Resize editor" className="h-1.5 bg-line hover:bg-edge" />}
        {resultsOpen && (
        <Panel minSize="15%">
          <div className="flex h-full flex-col">
            <div className="flex items-center gap-1 border-b border-line bg-panel pl-2">
              {withRows.length > 1 &&
                withRows.map(([index], position) => (
                  <button
                    key={index}
                    type="button"
                    onClick={() => setActive(position)}
                    className={`px-3 py-1 text-sm ${position === active ? 'here-top bg-ground font-bold' : 'text-muted hover:text-ink'}`}
                  >
                    Result {position + 1}
                  </button>
                ))}
              {withRows.length <= 1 && <span className="py-1 text-xs font-bold text-muted">Results</span>}
              <Button
                tone="quiet"
                className="ml-auto px-2 py-0.5 text-xs"
                aria-label="Close results"
                title="Close results and log. They open again on the next run."
                disabled={Boolean(runId)}
                onClick={closeResults}
              >
                <X size={13} aria-hidden /> Close
              </Button>
            </div>
            <ErrorNote message={error} />
            {stale && current && (
              <div className="flex items-center gap-2 border-b border-line px-2 py-1 text-sm">
                <span className="text-muted">These rows were loaded before your last change.</span>
                {kept?.sql && (
                  <Button disabled={Boolean(runId)} onClick={() => void run(kept.sql)}>
                    Refresh
                  </Button>
                )}
              </div>
            )}
            <WithLog log={log}>
            <div className="min-h-0 flex-1">
              {plan !== null ? (
                <PlanView text={plan} />
              ) : showing && current ? (
                <ResultsGrid store={showing} statement={current} onStop={runId ? stop : undefined} pickRef={gridPick} />
              ) : (
                <p className="p-3 text-sm text-muted">
                  {target ? 'Results appear here. Press Ctrl+Enter to run.' : 'Connect to a database to run this query.'}
                </p>
              )}
            </div>
            </WithLog>
          </div>
        </Panel>
        )}
      </Group>
    </div>
  );
}
