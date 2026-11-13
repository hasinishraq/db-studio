import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { ArrowDown, ArrowUp, ArrowUpDown, Lock, Undo2, X } from 'lucide-react';
import { clsx } from 'clsx';
import type { ColumnMeta, LockReason, Value } from '../../bindings';
import { cellText } from '../../lib/resultStore';
import {
  editCell,
  editNewCell,
  isPreview,
  rawValue,
  removeNewRow,
  revertCell,
  revertRow,
  type CellText,
  type Pending,
} from '../../lib/pendingEdits';

/** What may be edited; `null` turns inline editing off. Decided in Rust. */
export interface Editable {
  locked: Map<string, LockReason>;
}

const LOCK_TEXT: Record<LockReason, string> = {
  Masked: 'Masked, so it cannot be edited',
  Binary: 'Binary, so it cannot be edited here',
};

type Line = { kind: 'new'; id: number } | { kind: 'row'; index: number };
interface Spot {
  line: number;
  column: number;
}

/** The cell editor. Enter or Tab keep the value, Esc drops it, Alt+N sets NULL. */
function CellEditor({
  initial,
  isNew,
  onDone,
}: {
  initial: CellText | undefined;
  isNew: boolean;
  onDone: (result: { value: CellText | undefined; move: 'down' | 'right' | 'left' | null } | null) => void;
}) {
  const [text, setText] = useState(initial ?? '');
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);
  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    event.stopPropagation();
    if (event.key === 'Enter') {
      event.preventDefault();
      onDone({ value: text, move: 'down' });
    } else if (event.key === 'Tab') {
      event.preventDefault();
      onDone({ value: text, move: event.shiftKey ? 'left' : 'right' });
    } else if (event.key === 'Escape') {
      event.preventDefault();
      onDone(null);
    } else if (event.altKey && event.key.toLowerCase() === 'n') {
      event.preventDefault();
      onDone({ value: null, move: null });
    }
  };
  return (
    <div className="flex min-w-[12rem] items-center gap-1">
      <input
        ref={input}
        aria-label="Cell value"
        aria-description="Enter or Tab to keep, Escape to cancel, Alt+N for NULL"
        className="w-full rounded border border-accent bg-ground px-1 py-0.5 text-xs text-ink outline-none"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={keys}
      />
      <button
        type="button"
        className="rounded border border-edge bg-raised px-1 text-[10px] hover:bg-pick"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => onDone({ value: null, move: null })}
      >
        NULL
      </button>
      {isNew && (
        <button
          type="button"
          className="rounded border border-edge bg-raised px-1 text-[10px] hover:bg-pick"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onDone({ value: undefined, move: null })}
        >
          Default
        </button>
      )}
    </div>
  );
}

/** The table tab's grid. Edits stay pending here until the tab saves or discards them. */
export function EditableGrid({
  columns,
  rows,
  offset,
  nullText,
  rowHeight,
  pending,
  onPending,
  editable,
  selected,
  onSelected,
  sort,
  onSort,
  onRowOpen,
}: {
  columns: ColumnMeta[];
  rows: Value[][];
  offset: number;
  nullText: string;
  rowHeight: number;
  pending: Pending;
  onPending: (next: Pending) => void;
  editable: Editable | null;
  selected: Set<number>;
  onSelected: (next: Set<number>) => void;
  sort: { column: string; descending: boolean } | null;
  onSort: (column: string) => void;
  onRowOpen: (row: Value[]) => void;
}) {
  const lines: Line[] = [
    ...pending.inserts.map((row) => ({ kind: 'new' as const, id: row.id })),
    ...rows.map((_, index) => ({ kind: 'row' as const, index })),
  ];
  const [active, setActive] = useState<Spot | null>(null);
  const [editing, setEditing] = useState<Spot | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; row: number; column: number } | null>(null);
  const activeCell = useRef<HTMLTableCellElement>(null);
  const deleted = new Set(pending.deletes);

  useLayoutEffect(() => {
    if (!editing) activeCell.current?.focus();
  }, [active, editing]);

  // Positions shift when the page or the new rows change.
  useEffect(() => {
    setActive(null);
    setEditing(null);
  }, [rows]);

  const canEdit = (spot: Spot): boolean => {
    const line = lines[spot.line];
    const column = columns[spot.column];
    if (!editable || !line || !column || editable.locked.has(column.name)) return false;
    if (line.kind === 'new') return true;
    return !deleted.has(line.index) && !isPreview(rows[line.index]?.[spot.column]);
  };

  const loadedText = (line: Line, column: number): CellText | undefined => {
    if (line.kind === 'new') return pending.inserts.find((r) => r.id === line.id)?.values[column];
    const edits = pending.edits[line.index];
    if (edits && column in edits) return edits[column] ?? null;
    return rawValue(rows[line.index]?.[column]);
  };

  const finish = (spot: Spot, result: Parameters<Parameters<typeof CellEditor>[0]['onDone']>[0]) => {
    setEditing(null);
    if (!result) return;
    const line = lines[spot.line];
    if (!line) return;
    if (line.kind === 'new') {
      onPending(editNewCell(pending, line.id, spot.column, result.value));
    } else {
      const loaded = rawValue(rows[line.index]?.[spot.column]);
      onPending(editCell(pending, line.index, spot.column, result.value ?? null, loaded));
    }
    const step = { down: [1, 0], right: [0, 1], left: [0, -1] } as const;
    if (result.move) {
      const [dl, dc] = step[result.move];
      const next = {
        line: Math.min(lines.length - 1, Math.max(0, spot.line + dl)),
        column: Math.min(columns.length - 1, Math.max(0, spot.column + dc)),
      };
      setActive(next);
      if (canEdit(next) && result.move !== 'down') setEditing(next);
    }
  };

  const keys = (event: KeyboardEvent<HTMLTableSectionElement>) => {
    if (editing || !active) return;
    const moves: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
    };
    const move = moves[event.key];
    if (move) {
      event.preventDefault();
      setActive({
        line: Math.min(lines.length - 1, Math.max(0, active.line + move[0])),
        column: Math.min(columns.length - 1, Math.max(0, active.column + move[1])),
      });
    } else if ((event.key === 'Enter' || event.key === 'F2') && canEdit(active)) {
      event.preventDefault();
      setEditing(active);
    }
  };

  const toggleSelected = (index: number, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(index);
    else next.delete(index);
    onSelected(next);
  };

  return (
    <>
      <table className="border-collapse font-mono text-xs" role="grid" aria-label="Table data">
        <thead className="sticky top-0 z-10 select-none bg-panel shadow-[inset_0_-1px_0_var(--edge)]">
          <tr>
            <th className="h-8 w-9 border-b border-r border-line" aria-label="Select" />
            {columns.map((column) => {
              const lock = editable?.locked.get(column.name);
              return (
                <th key={column.name} className="h-8 border-b border-r border-line p-0 text-left font-sans">
                  <button
                    type="button"
                    className="group flex h-8 w-full items-center gap-1.5 whitespace-nowrap px-2.5 text-xs font-semibold hover:bg-hover"
                    title={column.typeName}
                    onClick={() => onSort(column.name)}
                  >
                    {lock && <Lock size={11} aria-label={LOCK_TEXT[lock]} className="text-muted" />}
                    <span>{column.name}</span>
                    <span className="font-mono text-2xs font-normal text-muted">{column.typeName}</span>
                    {sort?.column === column.name ? (
                      sort.descending ? (
                        <ArrowDown size={12} aria-label="sorted descending" className="ml-auto text-accent" />
                      ) : (
                        <ArrowUp size={12} aria-label="sorted ascending" className="ml-auto text-accent" />
                      )
                    ) : (
                      <ArrowUpDown size={12} aria-hidden className="ml-auto text-muted opacity-0 group-hover:opacity-100" />
                    )}
                  </button>
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody onKeyDown={keys}>
          {lines.map((line, lineIndex) => {
            const isNew = line.kind === 'new';
            const index = line.kind === 'row' ? line.index : -1;
            const row = line.kind === 'row' ? rows[line.index] : undefined;
            const gone = !isNew && deleted.has(index);
            const edits = isNew ? undefined : pending.edits[index];
            return (
              <tr
                key={isNew ? `new-${line.id}` : `row-${index}`}
                aria-selected={!isNew && selected.has(index)}
                className={clsx(
                  gone && 'text-muted line-through',
                  isNew && 'bg-change/10',
                  !isNew && !gone && (selected.has(index) ? 'here' : `hover:bg-hover ${index % 2 ? 'stripe' : ''}`),
                )}
                onDoubleClick={() => !editable && row && onRowOpen(row)}
              >
                <td className="whitespace-nowrap border-b border-r border-line/40 px-2 text-center">
                  {isNew ? (
                    <button
                      type="button"
                      aria-label="Remove new row"
                      title="Remove new row"
                      className="rounded p-0.5 text-muted hover:bg-hover hover:text-ink"
                      onClick={() => onPending(removeNewRow(pending, line.id))}
                    >
                      <X size={12} aria-hidden />
                    </button>
                  ) : (
                    <input
                      type="checkbox"
                      aria-label={`Select row ${offset + index + 1}`}
                      checked={selected.has(index)}
                      onChange={(e) => toggleSelected(index, e.target.checked)}
                    />
                  )}
                  {isNew && <span className="ml-1 font-sans text-2xs font-semibold text-change">New</span>}
                  {gone && (
                    <button
                      type="button"
                      aria-label={`Keep row ${offset + index + 1}`}
                      title="Keep this row"
                      className="ml-1 rounded p-0.5 hover:bg-hover"
                      onClick={() => onPending(revertRow(pending, index))}
                    >
                      <Undo2 size={12} aria-hidden />
                    </button>
                  )}
                </td>
                {columns.map((column, c) => {
                  const spot = { line: lineIndex, column: c };
                  const isActive = active?.line === lineIndex && active.column === c;
                  const isEditing = editing?.line === lineIndex && editing.column === c;
                  const changed = Boolean(edits && c in edits);
                  const value = row?.[c];
                  const newValue = isNew ? pending.inserts.find((r) => r.id === line.id)?.values[c] : undefined;
                  const shown: CellText | undefined = isNew ? newValue : changed ? (edits?.[c] ?? null) : rawValue(value);
                  const isDefault = isNew && newValue === undefined;
                  const lock = editable?.locked.get(column.name);
                  return (
                    <td
                      key={column.name}
                      ref={isActive ? activeCell : undefined}
                      tabIndex={isActive || (!active && lineIndex === 0 && c === 0) ? 0 : -1}
                      title={
                        lock
                          ? LOCK_TEXT[lock]
                          : changed
                            ? `Was: ${value?.t === 'Null' ? nullText : cellText(value)}`
                            : undefined
                      }
                      style={{ height: rowHeight }}
                      className={clsx(
                        'max-w-xs truncate border-b border-r border-line/40 px-2.5 outline-none',
                        isActive && 'ring-2 ring-inset ring-accent',
                        changed && !gone && 'bg-change/15',
                        (shown === null || isDefault) && 'italic text-muted',
                      )}
                      onClick={() => setActive(spot)}
                      onDoubleClick={(e) => {
                        if (canEdit(spot)) {
                          e.stopPropagation();
                          setActive(spot);
                          setEditing(spot);
                        }
                      }}
                      onContextMenu={(e) => {
                        if (isNew || (!changed && !gone)) return;
                        e.preventDefault();
                        setMenu({ x: e.clientX, y: e.clientY, row: index, column: c });
                      }}
                    >
                      {isEditing ? (
                        <CellEditor initial={loadedText(line, c)} isNew={isNew} onDone={(result) => finish(spot, result)} />
                      ) : isDefault ? (
                        'default'
                      ) : shown === null ? (
                        nullText
                      ) : isNew || changed ? (
                        shown
                      ) : (
                        cellText(value)
                      )}
                    </td>
                  );
                })}
              </tr>
            );
          })}
        </tbody>
      </table>
      {menu && (
        <div className="fixed inset-0 z-40" onClick={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null); }}>
          <div
            role="menu"
            className="fixed z-50 min-w-[9rem] animate-pop-in rounded-md border border-line bg-raised p-1 text-sm shadow-pop"
            style={{ left: menu.x, top: menu.y }}
            onKeyDown={(e) => e.key === 'Escape' && setMenu(null)}
          >
            {pending.edits[menu.row] && menu.column in (pending.edits[menu.row] ?? {}) && (
              <button
                type="button"
                role="menuitem"
                autoFocus
                className="flex h-7 w-full items-center rounded px-2 text-left hover:bg-accent hover:text-on-accent"
                onClick={() => onPending(revertCell(pending, menu.row, menu.column))}
              >
                Revert cell
              </button>
            )}
            <button
              type="button"
              role="menuitem"
              className="flex h-7 w-full items-center rounded px-2 text-left hover:bg-accent hover:text-on-accent"
              onClick={() => onPending(revertRow(pending, menu.row))}
            >
              Revert row
            </button>
          </div>
        </div>
      )}
    </>
  );
}
