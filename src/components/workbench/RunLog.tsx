import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { clsx } from 'clsx';
import { Group, Panel, Separator } from 'react-resizable-panels';
import { Button } from '../ui';

export type LogTone = 'info' | 'change' | 'error';

export interface LogEntry {
  id: number;
  at: number;
  tone: LogTone;
  text: string;
  sql: string | null;
}

/** What ran, kept for as long as the tab is open. */
export function useRunLog() {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const next = useRef(0);
  const add = useCallback((tone: LogTone, text: string, sql: string | null = null) => {
    next.current += 1;
    const entry = { id: next.current, at: Date.now(), tone, text, sql };
    setEntries((all) => [...all, entry]);
  }, []);
  const clear = useCallback(() => setEntries([]), []);
  return { entries, add, clear };
}

/** A small dot before each entry says what kind it is. */
const MARK: Record<LogTone, string> = {
  info: 'bg-edge',
  change: 'bg-change',
  error: 'bg-danger',
};

/** Sits under the table data, newest entry last. */
export function RunLog({ entries, onClear }: { entries: LogEntry[]; onClear: () => void }) {
  const list = useRef<HTMLOListElement>(null);
  useEffect(() => {
    if (list.current) list.current.scrollTop = list.current.scrollHeight;
  }, [entries]);
  if (entries.length === 0) return null;
  return (
    <section aria-label="Run log" className="flex h-full min-h-0 flex-col bg-panel">
      <div className="flex h-7 shrink-0 select-none items-center justify-between border-b border-line pl-3 pr-1">
        <h2 className="text-xs font-semibold text-muted">Log</h2>
        <Button tone="quiet" className="h-5 px-1.5 text-xs" onClick={onClear}>
          Clear log
        </Button>
      </div>
      <ol ref={list} className="min-h-0 flex-1 overflow-auto px-3 py-1">
        {entries.map((entry) => (
          <li key={entry.id} className="flex h-5 items-center gap-2 text-xs">
            <span aria-hidden className={clsx('size-1.5 shrink-0 rounded-full', MARK[entry.tone])} />
            <time className="shrink-0 tabular-nums text-muted">{new Date(entry.at).toLocaleTimeString()}</time>
            <span className="shrink-0">{entry.text}</span>
            {entry.sql && (
              <code className="min-w-0 truncate font-mono text-muted" title={entry.sql}>
                {entry.sql}
              </code>
            )}
          </li>
        ))}
      </ol>
    </section>
  );
}

/** The data above, the log below, and a handle between them to drag. */
export function WithLog({ log, children }: { log: ReturnType<typeof useRunLog>; children: ReactNode }) {
  return (
    <Group orientation="vertical" className="min-h-0 flex-1">
      <Panel minSize="10%">
        <div className="flex h-full min-h-0 flex-col">{children}</div>
      </Panel>
      {log.entries.length > 0 && (
        <>
          <Separator
            aria-label="Resize log"
            className="h-px bg-line transition-colors hover:bg-accent data-[separator=active]:bg-accent"
          />
          <Panel defaultSize="25%" minSize="8%">
            <RunLog entries={log.entries} onClear={log.clear} />
          </Panel>
        </>
      )}
    </Group>
  );
}
