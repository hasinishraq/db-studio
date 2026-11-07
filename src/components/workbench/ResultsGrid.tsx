import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type MutableRefObject } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { ColumnMeta, Value } from '../../bindings';
import { cellText, type ResultStore, type StatementResult } from '../../lib/resultStore';
import { Button, Input } from '../ui';

const ROW_HEIGHT = 24;
const DEFAULT_WIDTH = 160;

function Cell({ value, width }: { value: Value | undefined; width: number }) {
  const isNull = value?.t === 'Null';
  return (
    <div
      style={{ width }}
      className={`shrink-0 truncate border-r border-line px-2 font-mono text-xs leading-6 ${isNull ? 'italic text-muted' : ''}`}
      title={cellText(value)}
    >
      {cellText(value)}
    </div>
  );
}

function tsv(rows: Value[][]): string {
  return rows.map((row) => row.map(cellText).join('\t')).join('\n');
}

/** The rows a person has narrowed the grid to, in the order shown. */
export interface GridPick {
  kind: 'selected' | 'filtered';
  columns: ColumnMeta[];
  rows: Value[][];
}

/** A virtual grid. Only visible rows exist in the DOM, so a million rows stay smooth. */
export function ResultsGrid({
  store,
  statement,
  onStop,
  pickRef,
}: {
  store: ResultStore;
  statement: StatementResult;
  onStop?: () => void;
  pickRef?: MutableRefObject<(() => GridPick | null) | null>;
}) {
  useSyncExternalStore(store.subscribe, store.snapshot);
  const parent = useRef<HTMLDivElement>(null);
  const [widths, setWidths] = useState<Record<number, number>>({});
  const [sort, setSort] = useState<{ column: number; descending: boolean } | null>(null);
  const [filter, setFilter] = useState('');
  // Positions in the result, so a selection survives sorting.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const anchor = useRef<number | null>(null);
  const count = statement.rowCount;

  // Sort and filter work on the rows loaded so far, through an index, leaving the chunks untouched.
  const order = useMemo(() => {
    if (!sort && !filter) return null;
    const needle = filter.toLowerCase();
    const indexes: number[] = [];
    for (let i = 0; i < count; i += 1) {
      const row = statement.rowAt(i);
      if (!needle || row?.some((cell) => cellText(cell).toLowerCase().includes(needle))) indexes.push(i);
    }
    if (sort) {
      const key = (i: number) => cellText(statement.rowAt(i)?.[sort.column]);
      const direction = sort.descending ? -1 : 1;
      indexes.sort((a, b) => direction * key(a).localeCompare(key(b), undefined, { numeric: true }));
    }
    return indexes;
  }, [sort, filter, count, statement]);

  const total = order?.length ?? count;
  const rowAt = useCallback((i: number) => statement.rowAt(order ? (order[i] ?? -1) : i), [order, statement]);
  const virtual = useVirtualizer({
    count: total,
    getScrollElement: () => parent.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 20,
  });

  const sourceIndex = useCallback((shown: number) => (order ? (order[shown] ?? -1) : shown), [order]);

  useEffect(() => {
    setSelected(new Set());
    anchor.current = null;
  }, [statement, filter]);

  const pick = useCallback((): GridPick | null => {
    if (selected.size === 0 && !filter) return null;
    const rows: Value[][] = [];
    for (let shown = 0; shown < total; shown += 1) {
      if (selected.size > 0 && !selected.has(sourceIndex(shown))) continue;
      const row = rowAt(shown);
      if (row) rows.push(row);
    }
    return { kind: selected.size > 0 ? 'selected' : 'filtered', columns: statement.columns, rows };
  }, [selected, filter, total, sourceIndex, rowAt, statement]);

  useEffect(() => {
    if (!pickRef) return undefined;
    pickRef.current = pick;
    return () => {
      pickRef.current = null;
    };
  }, [pickRef, pick]);

  const clickRow = (shown: number, event: React.MouseEvent) => {
    const index = sourceIndex(shown);
    const adding = event.ctrlKey || event.metaKey;
    if (event.shiftKey && anchor.current !== null) {
      const from = Math.min(anchor.current, shown);
      const to = Math.max(anchor.current, shown);
      const next = new Set(adding ? selected : []);
      for (let i = from; i <= to; i += 1) next.add(sourceIndex(i));
      setSelected(next);
      return;
    }
    anchor.current = shown;
    const next = new Set(adding ? selected : []);
    if (selected.has(index) && (adding || selected.size === 1)) next.delete(index);
    else next.add(index);
    setSelected(next);
  };

  const onGridKey = (event: React.KeyboardEvent) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      const all = new Set<number>();
      for (let i = 0; i < total; i += 1) all.add(sourceIndex(i));
      setSelected(all);
    } else if (event.key === 'Escape') setSelected(new Set());
  };

  const width = (column: number) => widths[column] ?? DEFAULT_WIDTH;
  const fullWidth = statement.columns.reduce((sum, _, i) => sum + width(i), 0);

  const startResize = (column: number, event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = width(column);
    const move = (e: PointerEvent) =>
      setWidths((w) => ({ ...w, [column]: Math.max(50, startWidth + e.clientX - startX) }));
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const copy = () => {
    const rows: Value[][] = [];
    if (selected.size > 0) rows.push(...(pick()?.rows ?? []).slice(0, 10_000));
    else {
      for (let i = 0; i < Math.min(total, 10_000); i += 1) {
        const row = rowAt(i);
        if (row) rows.push(row);
      }
    }
    const header = statement.columns.map((c) => c.name).join('\t');
    void navigator.clipboard.writeText(`${header}\n${tsv(rows)}`);
  };

  const fetching = statement.status === 'running';
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-2 border-b border-line px-2 py-1 text-sm">
        <span className="text-muted">
          {fetching ? `Fetching ${count.toLocaleString()} rows` : `${count.toLocaleString()} rows`}
        </span>
        {selected.size > 0 && (
          <>
            <span>{selected.size.toLocaleString()} selected</span>
            <Button onClick={() => setSelected(new Set())}>Clear selection</Button>
          </>
        )}
        {fetching && onStop && (
          <Button tone="danger" onClick={onStop}>
            Stop
          </Button>
        )}
        <Input
          className="ml-auto max-w-48"
          aria-label="Filter loaded rows"
          placeholder="Filter loaded rows"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <Button onClick={copy}>Copy</Button>
      </div>
      <div
        ref={parent}
        role="grid"
        aria-multiselectable
        className="min-h-0 flex-1 overflow-auto"
        tabIndex={0}
        aria-label="Query results"
        onKeyDown={onGridKey}
      >
        <div style={{ width: fullWidth }} className="sticky top-0 z-10 flex border-b border-line bg-panel">
          {statement.columns.map((column, index) => (
            <div key={`${column.name}${index}`} style={{ width: width(index) }} className="relative shrink-0 border-r border-line">
              <button
                type="button"
                className="w-full truncate px-2 text-left text-xs font-bold leading-6"
                title={column.typeName}
                onClick={() =>
                  setSort((s) =>
                    s?.column === index ? (s.descending ? null : { column: index, descending: true }) : { column: index, descending: false },
                  )
                }
              >
                {column.name}
                {sort?.column === index ? (sort.descending ? ' ▼' : ' ▲') : ''}
              </button>
              <span
                aria-hidden
                onPointerDown={(e) => startResize(index, e)}
                className="absolute right-0 top-0 h-full w-1 cursor-col-resize hover:bg-line"
              />
            </div>
          ))}
        </div>
        <div style={{ height: virtual.getTotalSize(), width: fullWidth, position: 'relative' }}>
          {virtual.getVirtualItems().map((item) => (
            <div
              key={item.key}
              role="row"
              aria-selected={selected.has(sourceIndex(item.index))}
              style={{ position: 'absolute', top: 0, transform: `translateY(${item.start}px)`, height: ROW_HEIGHT }}
              className={`flex cursor-default select-none border-b border-line/40 ${
                selected.has(sourceIndex(item.index)) ? 'bg-line' : 'hover:bg-raised'
              }`}
              onClick={(event) => clickRow(item.index, event)}
            >
              {statement.columns.map((_, column) => (
                <Cell key={column} value={rowAt(item.index)?.[column]} width={width(column)} />
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
