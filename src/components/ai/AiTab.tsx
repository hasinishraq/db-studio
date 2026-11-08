import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { openUrl } from '@tauri-apps/plugin-opener';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { api, ApiError, errorMessage } from '../../api';
import { cancelQuery, cancelTurn, reviewQuery, sendTurn } from '../../api/ai';
import { usePrefs } from '../../api/settings';
import type { ConnectionProfile, DiagramFormat } from '../../bindings';
import { newId, useStudio } from '../../context/Studio';
import { buildChart, CHART_TYPES, chooseChart, initialView, type ChartType } from '../../lib/chart';
import { standaloneSvg } from '../../lib/diagramExport';
import {
  applyAiEvent,
  isSafeLink,
  loadConversation,
  saveConversation,
  type QueryPart,
  type TurnView,
} from '../../lib/chat';
import { cellText } from '../../lib/resultStore';
import { useNamespace, whereLabel } from '../../lib/useNamespace';
import { Button, ErrorNote, Select } from '../ui';
import { CHART_WIDTH, chartHeight, chartTop, ResultChart } from './ResultChart';
import { ReviewCard } from './ReviewCard';

/** Renders a model's markdown. Only https and mailto links are clickable. */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="prose-sm space-y-2 text-sm [&_code]:font-mono [&_pre]:overflow-auto [&_pre]:rounded [&_pre]:bg-ground [&_pre]:p-2">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) =>
            isSafeLink(href) ? (
              <a
                href={href}
                className="underline"
                onClick={(event) => {
                  event.preventDefault();
                  if (href) void openUrl(href);
                }}
              >
                {children}
              </a>
            ) : (
              <span>{children}</span>
            ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

export function QueryResult({ query }: { query: QueryPart }) {
  const stat = useMemo(() => chooseChart(query.columns, query.rows), [query.columns, query.rows]);
  const [view, setView] = useState<ChartType | 'table'>(() => initialView(query.columns, query.rows, query.chart));
  const [saveError, setSaveError] = useState<string | null>(null);
  const drawing = useRef<SVGGElement>(null);
  // Every form is tried, so the chooser can say why one does not fit.
  const options = useMemo(
    () =>
      CHART_TYPES.map((option) => ({
        ...option,
        result: buildChart(option.type, query.columns, query.rows, query.chart),
      })),
    [query.columns, query.rows, query.chart],
  );
  if (query.rowsAffected !== null) return <p className="text-sm">{query.rowsAffected} rows affected.</p>;
  if (stat.kind === 'stat' && !query.chart) {
    return (
      <p className="my-1">
        <span className="text-xl font-bold">{stat.value}</span> <span className="text-sm text-muted">{stat.label}</span>
      </p>
    );
  }
  const shown = view === 'table' ? null : options.find((o) => o.type === view)?.result;
  const chart = shown && 'chart' in shown ? shown.chart : null;
  const anyChart = options.some((o) => 'chart' in o.result);

  /** Rust draws the file and asks where to save it; the page never names a path. */
  const save = async (format: DiagramFormat) => {
    if (!drawing.current || !chart) return;
    setSaveError(null);
    try {
      const svg = standaloneSvg(drawing.current, {
        x: 0,
        y: 0,
        width: CHART_WIDTH,
        height: chartHeight(chart, chartTop(chart)),
      });
      await api.exportDiagram(svg, format, chart.title);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setSaveError(errorMessage(e));
    }
  };

  return (
    <div>
      {anyChart && (
        <div className="flex flex-wrap items-center gap-2">
          <Select
            aria-label="Show as"
            className="w-56"
            value={view}
            onChange={(e) => setView(e.target.value as ChartType | 'table')}
          >
            <option value="table">Table</option>
            {options.map((option) => (
              <option key={option.type} value={option.type} disabled={!('chart' in option.result)}>
                {'reason' in option.result ? `${option.label}: ${option.result.reason}` : `${option.label} chart`}
              </option>
            ))}
          </Select>
          {chart && (
            <Select
              aria-label="Save chart"
              className="w-36"
              value=""
              onChange={(e) => e.target.value && void save(e.target.value as DiagramFormat)}
            >
              <option value="">Save chart</option>
              <option value="Png">PNG image</option>
              <option value="Jpeg">JPEG image</option>
              <option value="Pdf">PDF document</option>
            </Select>
          )}
        </div>
      )}
      <ErrorNote message={saveError} />
      {chart ? (
        <ResultChart chart={chart} drawingRef={drawing} />
      ) : (
        <div className="my-1 max-h-64 overflow-auto rounded bg-ground">
          <table className="font-mono text-xs">
            <thead className="sticky top-0 bg-panel">
              <tr>
                {query.columns.map((c, i) => (
                  <th key={i} className="px-2 text-left">
                    {c.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {query.rows.slice(0, 500).map((row, index) => (
                <tr key={index}>
                  {row.map((cell, i) => (
                    <td key={i} className="max-w-xs truncate px-2">
                      {cellText(cell)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="text-xs text-muted">
        {query.rows.length.toLocaleString()} rows in {query.elapsedMs} ms
        {query.truncated ? ', stopped at the limit' : ''}
      </p>
    </div>
  );
}

function QueryBlock({
  query,
  turn,
  profile,
}: {
  query: QueryPart;
  turn: string;
  profile: ConnectionProfile | null;
}) {
  if (query.status === 'review' && profile) {
    return (
      <ReviewCard
        query={query}
        connectionName={profile.name}
        environment={profile.environment}
        onDecide={(approved) => void reviewQuery(turn, query.id, approved)}
      />
    );
  }
  const note: Record<QueryPart['status'], string> = {
    review: 'Waiting for review',
    running: 'Running',
    done: '',
    declined: 'Declined. It did not run.',
    cancelled: 'Cancelled.',
    error: query.error ?? 'It failed.',
  };
  return (
    <div className="my-2">
      <pre className="overflow-auto rounded bg-ground p-2 font-mono text-xs">{query.sql}</pre>
      {query.status === 'running' && (
        <Button onClick={() => void cancelQuery(turn, query.id)}>Stop this query</Button>
      )}
      <p className="text-sm text-muted">{note[query.status]}</p>
      {query.status === 'done' && <QueryResult query={query} />}
    </div>
  );
}

export function AiTab() {
  const { profiles, sessions } = useStudio();
  const client = useQueryClient();
  const connected = profiles.filter((p) => sessions[p.id]);
  const [profileId, setProfileId] = useState<string>('');
  const [conversation, setConversation] = useState(() => newId());
  const [turns, setTurns] = useState<TurnView[]>([]);
  const [text, setText] = useState('');
  const [running, setRunning] = useState<string | null>(null);
  // Each chat starts from the Settings defaults and can be changed here.
  const prefs = usePrefs();
  const [autoRunChoice, setAutoRun] = useState<boolean | null>(null);
  const [shareChoice, setShare] = useState<boolean | null>(null);
  const autoRun = autoRunChoice ?? prefs.aiAutoRun;
  const share = shareChoice ?? prefs.aiShareRows;
  const [error, setError] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const { data: conversations = [] } = useQuery({
    queryKey: ['conversations'],
    queryFn: api.aiConversations,
  });
  const profile = connected.find((p) => p.id === profileId) ?? connected[0] ?? null;
  const shareForbidden =
    profile?.aiPolicy === 'NoRowSharing' || (profile?.environment === 'Production' && profile.aiPolicy === 'Default');
  const disabled = profile?.aiPolicy === 'Disabled';
  const namespace = useNamespace(profile);
  const where = profile ? whereLabel(profile, namespace.database, namespace.schema) : null;
  const sessionId = namespace.session?.sessionId;
  // The same lists the side panel shows, so switching here and there agree.
  const { data: schemas = [] } = useQuery({
    queryKey: ['schema', profile?.id, 'list', sessionId],
    queryFn: () => api.listSchemas(sessionId ?? ''),
    enabled: Boolean(sessionId) && profile?.engine !== 'Sqlite',
  });
  const { data: databases = [] } = useQuery({
    queryKey: ['schema', profile?.id, 'databases', sessionId],
    queryFn: () => api.listDatabases(sessionId ?? ''),
    enabled: Boolean(sessionId) && profile?.engine === 'Postgres',
  });
  const databaseChoices = profile?.engine === 'Postgres' ? databases : schemas.map((s) => s.name);

  // A note marks where the conversation moved, so earlier tables are read in their place.
  const lastWhere = useRef<string | null>(null);
  useEffect(() => {
    const before = lastWhere.current;
    lastWhere.current = where;
    if (before && where && before !== where) {
      setTurns((all) =>
        all.length === 0
          ? all
          : [
              ...all,
              {
                id: newId(),
                userText: '',
                parts: [],
                finished: true,
                error: null,
                note: `Now working in ${where}`,
              },
            ],
      );
    }
  }, [where]);

  const switchTo = async (step: () => Promise<void>) => {
    setError(null);
    try {
      await step();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  useEffect(() => {
    bottom.current?.scrollIntoView?.({ block: 'end' });
  }, [turns]);

  useEffect(() => {
    if (!running && turns.length > 0) saveConversation(conversation, turns);
  }, [running, turns, conversation]);

  const send = async (message: string) => {
    const turnId = newId();
    setError(null);
    setRunning(turnId);
    setTurns((all) => [
      ...all,
      {
        id: turnId,
        userText: message,
        parts: [],
        finished: false,
        error: null,
      },
    ]);
    try {
      await sendTurn(
        {
          conversationId: conversation,
          turnId,
          sessionId: profile ? (sessions[profile.id]?.sessionId ?? null) : null,
          text: message,
          autoRun,
          shareRows: share && !shareForbidden,
        },
        (event) => setTurns((all) => all.map((t) => (t.id === turnId ? applyAiEvent(t, event) : t))),
      );
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunning(null);
      void client.invalidateQueries({ queryKey: ['conversations'] });
    }
  };

  const regenerate = async (turn: TurnView) => {
    await api.aiForget(conversation, turn.id);
    setTurns((all) =>
      all.slice(
        0,
        all.findIndex((t) => t.id === turn.id),
      ),
    );
    void send(turn.userText);
  };

  const open = (id: string) => {
    setConversation(id);
    setTurns(loadConversation(id));
  };

  const forget = async (id: string) => {
    await api.aiForget(id, null);
    if (id === conversation) {
      setConversation(newId());
      setTurns([]);
    }
    void client.invalidateQueries({ queryKey: ['conversations'] });
  };

  return (
    <div className="flex h-full">
      <aside className="w-52 shrink-0 overflow-auto border-r border-line p-2 text-sm">
        <Button
          className="mb-2 w-full justify-center"
          onClick={() => {
            setConversation(newId());
            setTurns([]);
          }}
        >
          New conversation
        </Button>
        {conversations.map((c) => (
          <div key={c.id} className="group flex items-center">
            <button type="button" className="min-w-0 flex-1 py-1 text-left hover:underline" onClick={() => open(c.id)}>
              <span className="block truncate">{c.title}</span>
              {(c.connection || c.database) && (
                <span className="block truncate text-xs text-muted">
                  {[c.connection, c.database].filter(Boolean).join(' · ')}
                </span>
              )}
            </button>
            <button
              type="button"
              aria-label={`Delete ${c.title}`}
              className="px-1 text-muted opacity-0 focus:opacity-100 group-hover:opacity-100"
              onClick={() => void forget(c.id)}
            >
              Delete
            </button>
          </div>
        ))}
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="min-h-0 flex-1 overflow-auto p-3">
          {turns.length === 0 && (
            <p className="max-w-prose text-sm text-muted">
              Ask about your data. Reads run by themselves; anything that changes data waits for your approval.
            </p>
          )}
          {turns.map((turn) =>
            turn.note ? (
              <p
                key={turn.id}
                role="note"
                className="mb-4 max-w-3xl border-y border-line py-1 text-center text-xs text-muted"
              >
                {turn.note}
              </p>
            ) : (
              <article key={turn.id} className="mb-4 max-w-3xl">
                <p className="mb-1 rounded bg-raised px-2 py-1 text-sm">{turn.userText}</p>
                {turn.parts.map((part, index) =>
                  part.kind === 'text' ? (
                    <Markdown key={index} text={part.text} />
                  ) : (
                    <QueryBlock key={part.query.id} query={part.query} turn={turn.id} profile={profile} />
                  ),
                )}
                <ErrorNote message={turn.error} />
                {turn.finished && (
                  <Button className="mt-1" onClick={() => void regenerate(turn)}>
                    Regenerate
                  </Button>
                )}
              </article>
            ),
          )}
          <div ref={bottom} />
        </div>
        <ErrorNote message={error} />
        <div className="border-t border-line bg-panel p-2">
          <p className="mb-1 text-xs text-muted" aria-live="polite">
            {!profile
              ? 'Not connected. Connect in the side panel to let the assistant read your data.'
              : profile.engine === 'MySql' && !namespace.database
                ? `${profile.name}: no database in use. Pick one so the assistant knows where to work.`
                : `Working in ${where}`}
          </p>
          <div className="mb-1 flex flex-wrap items-center gap-3 text-sm">
            <Select
              aria-label="Connection"
              className="w-56"
              value={profile?.id ?? ''}
              onChange={(e) => setProfileId(e.target.value)}
            >
              {connected.length === 0 && <option value="">Not connected</option>}
              {connected.map((p) => (
                <option key={p.id} value={p.id}>
                  {whereLabel(p, sessions[p.id]?.database ?? null, sessions[p.id]?.schema ?? null)}
                </option>
              ))}
            </Select>
            {profile && profile.engine !== 'Sqlite' && (
              <Select
                aria-label="Database"
                className="w-40"
                value={namespace.database ?? ''}
                disabled={Boolean(running)}
                onChange={(e) => e.target.value && void switchTo(() => namespace.chooseDatabase(e.target.value))}
              >
                {!namespace.database && <option value="">Choose a database</option>}
                {databaseChoices.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </Select>
            )}
            {profile?.engine === 'Postgres' && (
              <Select
                aria-label="Schema"
                className="w-36"
                value={namespace.schema ?? ''}
                disabled={Boolean(running)}
                onChange={(e) => e.target.value && void switchTo(() => namespace.chooseSchema(e.target.value))}
              >
                {!namespace.schema && <option value="">Choose a schema</option>}
                {schemas.map((s) => (
                  <option key={s.name} value={s.name}>
                    {s.name}
                  </option>
                ))}
              </Select>
            )}
            <label className="flex items-center gap-1">
              <input type="checkbox" checked={autoRun} onChange={(e) => setAutoRun(e.target.checked)} />
              Run safe reads automatically
            </label>
            <label
              className="flex items-center gap-1"
              title={shareForbidden ? 'This connection does not allow it' : undefined}
            >
              <input
                type="checkbox"
                disabled={shareForbidden}
                checked={share && !shareForbidden}
                onChange={(e) => setShare(e.target.checked)}
              />
              Let the model read query results
              {shareForbidden && <span className="text-muted">(not allowed on this connection)</span>}
            </label>
          </div>
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (!text.trim() || running) return;
              void send(text.trim());
              setText('');
            }}
          >
            <textarea
              aria-label="Message"
              rows={2}
              value={text}
              disabled={disabled}
              placeholder={disabled ? 'The assistant is off for this connection.' : 'Ask about your data'}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  e.currentTarget.form?.requestSubmit();
                }
              }}
              className="w-full resize-none rounded border border-line bg-ground px-2 py-1 text-sm"
            />
            {running ? (
              <Button tone="danger" onClick={() => void cancelTurn(running)}>
                Stop
              </Button>
            ) : (
              <Button tone="primary" type="submit" disabled={disabled}>
                Send
              </Button>
            )}
          </form>
        </div>
      </div>
    </div>
  );
}
