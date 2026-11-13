import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { openUrl } from '@tauri-apps/plugin-opener';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { ArrowUp, Bot, Check, Copy, MessageSquarePlus, RotateCcw, Square, Trash2 } from 'lucide-react';
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
import { Button, cn, ErrorNote, Kbd, Select } from '../ui';
import { CHART_WIDTH, chartHeight, chartTop, ResultChart } from './ResultChart';
import { ReviewCard } from './ReviewCard';

/** Renders a model's markdown. Only https and mailto links are clickable. */
export function Markdown({ text }: { text: string }) {
  return (
    <div
      className={cn(
        'space-y-2.5 text-sm leading-relaxed',
        '[&_code]:rounded [&_code]:bg-raised [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[12px]',
        '[&_pre]:overflow-auto [&_pre]:rounded-md [&_pre]:border [&_pre]:border-line [&_pre]:bg-ground [&_pre]:p-3',
        '[&_pre_code]:bg-transparent [&_pre_code]:p-0',
        '[&_ul]:list-disc [&_ul]:space-y-1 [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:space-y-1 [&_ol]:pl-5',
        '[&_h1]:text-base [&_h1]:font-semibold [&_h2]:text-sm [&_h2]:font-semibold [&_h3]:text-sm [&_h3]:font-semibold',
        '[&_table]:w-full [&_table]:border-collapse [&_table]:text-xs [&_td]:border [&_td]:border-line [&_td]:px-2 [&_td]:py-1',
        '[&_th]:border [&_th]:border-line [&_th]:bg-panel [&_th]:px-2 [&_th]:py-1 [&_th]:text-left [&_th]:font-semibold',
        '[&_blockquote]:border-l-2 [&_blockquote]:border-line [&_blockquote]:pl-3 [&_blockquote]:text-muted',
      )}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) =>
            isSafeLink(href) ? (
              <a
                href={href}
                className="text-accent underline-offset-2 hover:underline"
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
  if (query.rowsAffected !== null) return <p className="px-3 py-2 text-sm">{query.rowsAffected} rows affected.</p>;
  if (stat.kind === 'stat' && !query.chart) {
    return (
      <p className="flex items-baseline gap-2 px-3 py-3">
        <span className="text-2xl font-semibold tabular-nums">{stat.value}</span>
        <span className="text-sm text-muted">{stat.label}</span>
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
      {chart ? (
        <div className="p-3">
          <ResultChart chart={chart} drawingRef={drawing} />
        </div>
      ) : (
        <div className="max-h-72 overflow-auto">
          <table className="w-full border-collapse font-mono text-xs">
            <thead className="sticky top-0 bg-panel">
              <tr>
                {query.columns.map((c, i) => (
                  <th
                    key={i}
                    className="whitespace-nowrap border-b border-line px-3 py-1.5 text-left font-sans font-semibold text-muted"
                  >
                    {c.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {query.rows.slice(0, 500).map((row, index) => (
                <tr key={index} className="hover:bg-hover">
                  {row.map((cell, i) => (
                    <td key={i} className="max-w-xs truncate border-b border-line/60 px-3 py-1">
                      {cellText(cell)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <ErrorNote message={saveError} />
      <div className="flex flex-wrap items-center gap-2 border-t border-line px-3 py-1.5">
        <p className="text-xs tabular-nums text-muted">
          {query.rows.length.toLocaleString()} rows in {query.elapsedMs} ms
          {query.truncated ? ', stopped at the limit' : ''}
        </p>
        {anyChart && (
          <div className="ml-auto flex items-center gap-1.5">
            <Select
              aria-label="Show as"
              className="h-6 w-52 text-xs"
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
                className="h-6 w-32 text-xs"
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
      </div>
    </div>
  );
}

/** Copies text and says so for a moment. */
function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className="rounded p-1 text-muted hover:bg-hover hover:text-ink"
      onClick={() => {
        void navigator.clipboard?.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      }}
    >
      {copied ? <Check size={13} className="text-safe" /> : <Copy size={13} />}
    </button>
  );
}

const STATUS: Record<QueryPart['status'], { text: string; tone: string }> = {
  review: { text: 'Waiting for review', tone: 'text-change' },
  running: { text: 'Running', tone: 'text-accent' },
  done: { text: '', tone: '' },
  declined: { text: 'Declined. It did not run.', tone: 'text-muted' },
  cancelled: { text: 'Cancelled.', tone: 'text-muted' },
  error: { text: 'It failed.', tone: 'text-danger' },
};

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
  const status = STATUS[query.status];
  const note = query.status === 'error' ? (query.error ?? status.text) : status.text;
  return (
    <div className="my-3 overflow-hidden rounded-md border border-line bg-ground">
      <div className="flex h-8 items-center gap-2 border-b border-line bg-panel pl-3 pr-1 text-xs">
        <span className="font-semibold text-muted">SQL</span>
        {query.readOnly && <span className="rounded border border-line px-1.5 text-2xs text-muted">Read</span>}
        {query.status === 'running' && (
          <span aria-hidden className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
        )}
        {note && <span className={cn('min-w-0 truncate', status.tone)}>{note}</span>}
        <span className="ml-auto flex items-center gap-0.5">
          {query.status === 'running' && (
            <Button tone="quiet" className="h-6 px-2 text-xs" onClick={() => void cancelQuery(turn, query.id)}>
              <Square size={11} aria-hidden /> Stop this query
            </Button>
          )}
          <CopyButton text={query.sql} label="Copy SQL" />
        </span>
      </div>
      <pre className="max-h-48 overflow-auto px-3 py-2 font-mono text-xs leading-relaxed">{query.sql}</pre>
      {query.status === 'done' && (
        <div className="border-t border-line">
          <QueryResult query={query} />
        </div>
      )}
    </div>
  );
}

/** Starting points that fill the box, so nothing is sent without the user pressing Send. */
const STARTERS = [
  'Which tables are the largest?',
  'Describe the main tables and how they relate',
  'Find tables without a primary key',
  'Which columns are never filled in?',
];

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
  const box = useRef<HTMLTextAreaElement>(null);
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

  const submit = () => {
    if (!text.trim() || running) return;
    void send(text.trim());
    setText('');
  };

  const status = !profile
    ? 'Not connected. Connect in the side panel to let the assistant read your data.'
    : profile.engine === 'MySql' && !namespace.database
      ? `${profile.name}: no database in use. Pick one so the assistant knows where to work.`
      : `Working in ${where}`;

  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 select-none flex-col border-r border-line bg-panel">
        <div className="flex h-9 shrink-0 items-center justify-between pl-3 pr-1.5">
          <h2 className="text-xs font-semibold text-muted">Conversations</h2>
          <Button
            tone="quiet"
            className="w-[26px] px-0"
            aria-label="New conversation"
            title="New conversation"
            onClick={() => {
              setConversation(newId());
              setTurns([]);
            }}
          >
            <MessageSquarePlus size={15} aria-hidden />
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-1.5 pb-2">
          {conversations.length === 0 && <p className="px-2 py-1 text-xs text-muted">Past conversations appear here.</p>}
          {conversations.map((c) => (
            <div
              key={c.id}
              className={cn(
                'group flex items-center rounded',
                c.id === conversation ? 'bg-pick' : 'hover:bg-hover',
              )}
            >
              <button type="button" className="min-w-0 flex-1 px-2 py-1.5 text-left" onClick={() => open(c.id)}>
                <span className="block truncate text-sm">{c.title}</span>
                {(c.connection || c.database) && (
                  <span className="block truncate text-xs text-muted">
                    {[c.connection, c.database].filter(Boolean).join(' · ')}
                  </span>
                )}
              </button>
              <button
                type="button"
                aria-label={`Delete ${c.title}`}
                title="Delete"
                className="mr-1 rounded p-1 text-muted opacity-0 hover:bg-hover hover:text-danger focus:opacity-100 group-hover:opacity-100"
                onClick={() => void forget(c.id)}
              >
                <Trash2 size={13} aria-hidden />
              </button>
            </div>
          ))}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex min-h-[38px] shrink-0 select-none flex-wrap items-center gap-1.5 border-b border-line bg-panel px-2 py-1.5">
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
          <div className="ml-auto flex flex-wrap items-center gap-4 pr-1 text-xs">
            <label className="flex items-center gap-1.5">
              <input type="checkbox" checked={autoRun} onChange={(e) => setAutoRun(e.target.checked)} />
              Run safe reads automatically
            </label>
            <label
              className={cn('flex items-center gap-1.5', shareForbidden && 'text-muted')}
              title={shareForbidden ? 'This connection does not allow it' : undefined}
            >
              <input
                type="checkbox"
                disabled={shareForbidden}
                checked={share && !shareForbidden}
                onChange={(e) => setShare(e.target.checked)}
              />
              Let the model read query results
              {shareForbidden && <span>(not allowed on this connection)</span>}
            </label>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto">
          <div className="mx-auto max-w-3xl px-6 py-6">
            {turns.length === 0 && (
              <div className="pt-[8vh]">
                <span className="flex h-9 w-9 items-center justify-center rounded-lg border border-line bg-panel text-accent">
                  <Bot size={18} aria-hidden />
                </span>
                <h1 className="mt-4 text-lg font-semibold">Ask about your data</h1>
                <p className="mt-1 max-w-prose text-sm text-muted">
                  Reads run by themselves. Anything that changes data waits for your approval.
                </p>
                <div className="mt-5 grid gap-2 sm:grid-cols-2">
                  {STARTERS.map((starter) => (
                    <button
                      key={starter}
                      type="button"
                      disabled={disabled}
                      className="rounded-md border border-line bg-panel px-3 py-2 text-left text-sm text-ink/90 transition-colors hover:border-edge hover:bg-hover disabled:opacity-40"
                      onClick={() => {
                        setText(starter);
                        box.current?.focus();
                      }}
                    >
                      {starter}
                    </button>
                  ))}
                </div>
              </div>
            )}
            {turns.map((turn) =>
              turn.note ? (
                <p
                  key={turn.id}
                  role="note"
                  className="my-6 flex items-center gap-3 text-xs text-muted before:h-px before:flex-1 before:bg-line after:h-px after:flex-1 after:bg-line"
                >
                  {turn.note}
                </p>
              ) : (
                <article key={turn.id} className="group/turn mb-8">
                  <div className="mb-3 flex justify-end">
                    <p className="max-w-[85%] whitespace-pre-wrap rounded-lg rounded-br-sm border border-line bg-raised px-3 py-2 text-sm">
                      {turn.userText}
                    </p>
                  </div>
                  <div className="flex gap-3">
                    <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-line bg-panel text-accent">
                      <Bot size={13} aria-hidden />
                    </span>
                    <div className="min-w-0 flex-1">
                      {turn.parts.length === 0 && !turn.finished && !turn.error && (
                        <p className="flex h-6 items-center gap-1 text-sm text-muted" aria-live="polite">
                          Thinking
                          <span aria-hidden className="flex gap-0.5">
                            <span className="h-1 w-1 animate-pulse rounded-full bg-muted" />
                            <span className="h-1 w-1 animate-pulse rounded-full bg-muted [animation-delay:150ms]" />
                            <span className="h-1 w-1 animate-pulse rounded-full bg-muted [animation-delay:300ms]" />
                          </span>
                        </p>
                      )}
                      {turn.parts.map((part, index) =>
                        part.kind === 'text' ? (
                          <Markdown key={index} text={part.text} />
                        ) : (
                          <QueryBlock key={part.query.id} query={part.query} turn={turn.id} profile={profile} />
                        ),
                      )}
                      <ErrorNote message={turn.error} />
                      {turn.finished && (
                        <div className="mt-2 flex items-center gap-0.5 opacity-60 transition-opacity focus-within:opacity-100 group-hover/turn:opacity-100">
                          <Button tone="quiet" className="h-6 px-1.5 text-xs" onClick={() => void regenerate(turn)}>
                            <RotateCcw size={12} aria-hidden /> Regenerate
                          </Button>
                          <CopyButton
                            label="Copy answer"
                            text={turn.parts
                              .map((part) => (part.kind === 'text' ? part.text : part.query.sql))
                              .join('\n\n')}
                          />
                        </div>
                      )}
                    </div>
                  </div>
                </article>
              ),
            )}
            <div ref={bottom} />
          </div>
        </div>

        <div className="shrink-0 px-6 pb-3">
          <div className="mx-auto max-w-3xl">
            <ErrorNote message={error} />
            <form
              className={cn(
                'mt-2 rounded-lg border border-edge/70 bg-raised transition-colors focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20',
                disabled && 'opacity-60',
              )}
              onSubmit={(event) => {
                event.preventDefault();
                submit();
              }}
            >
              <textarea
                ref={box}
                aria-label="Message"
                rows={3}
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
                className="block max-h-48 min-h-[64px] w-full resize-none bg-transparent px-3 pt-2.5 text-sm outline-none placeholder:text-muted"
              />
              <div className="flex items-center gap-2 px-2 pb-2">
                <p className="min-w-0 truncate pl-1 text-xs text-muted" aria-live="polite">
                  {status}
                </p>
                <span className="ml-auto hidden items-center gap-1 text-2xs text-muted md:flex">
                  <Kbd>Enter</Kbd> to send, <Kbd>Shift Enter</Kbd> for a new line
                </span>
                {running ? (
                  <Button tone="danger" className="h-7" onClick={() => void cancelTurn(running)}>
                    <Square size={11} aria-hidden /> Stop
                  </Button>
                ) : (
                  <Button tone="primary" type="submit" className="h-7" disabled={disabled || !text.trim()}>
                    <ArrowUp size={14} aria-hidden /> Send
                  </Button>
                )}
              </div>
            </form>
          </div>
        </div>
      </div>
    </div>
  );
}
