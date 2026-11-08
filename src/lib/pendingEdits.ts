import type { CellValue, ChangeSet, ColumnMeta, Value } from '../bindings';
import { cellText } from './resultStore';

/** A cell's text as the database gave it; `null` is SQL NULL. */
export type CellText = string | null;

/** A row added in the grid. Cells left out take the column's default. */
export interface NewRow {
  id: number;
  values: Record<number, CellText>;
}

/**
 * Grid edits not yet saved. Rows are named by their position on the loaded page, so the pending
 * state must be saved or discarded before the page changes.
 */
export interface Pending {
  /** Row index, then column index, to the new text. */
  edits: Record<number, Record<number, CellText>>;
  inserts: NewRow[];
  deletes: number[];
  nextId: number;
}

export const emptyPending = (): Pending => ({ edits: {}, inserts: [], deletes: [], nextId: 1 });

/** The text of a loaded value; NULL becomes `null`. */
export function rawValue(value: Value | undefined): CellText {
  return !value || value.t === 'Null' ? null : cellText(value);
}

/** Bytes and undecodable values are only previews, so they are never edited. */
export function isPreview(value: Value | undefined): boolean {
  return value?.t === 'Bytes' || value?.t === 'Unsupported';
}

/** Sets a loaded cell. Setting it back to what was loaded removes the edit. */
export function editCell(state: Pending, row: number, column: number, value: CellText, loaded: CellText): Pending {
  const rowEdits = { ...(state.edits[row] ?? {}) };
  if (value === loaded) delete rowEdits[column];
  else rowEdits[column] = value;
  const edits = { ...state.edits };
  if (Object.keys(rowEdits).length === 0) delete edits[row];
  else edits[row] = rowEdits;
  return { ...state, edits };
}

export function revertCell(state: Pending, row: number, column: number): Pending {
  const rowEdits = { ...(state.edits[row] ?? {}) };
  delete rowEdits[column];
  const edits = { ...state.edits };
  if (Object.keys(rowEdits).length === 0) delete edits[row];
  else edits[row] = rowEdits;
  return { ...state, edits };
}

export function revertRow(state: Pending, row: number): Pending {
  const edits = { ...state.edits };
  delete edits[row];
  return { ...state, edits, deletes: state.deletes.filter((r) => r !== row) };
}

export function addRow(state: Pending): Pending {
  return { ...state, inserts: [{ id: state.nextId, values: {} }, ...state.inserts], nextId: state.nextId + 1 };
}

/** Sets a cell of a new row; `undefined` goes back to the default. */
export function editNewCell(state: Pending, id: number, column: number, value: CellText | undefined): Pending {
  return {
    ...state,
    inserts: state.inserts.map((row) => {
      if (row.id !== id) return row;
      const values = { ...row.values };
      if (value === undefined) delete values[column];
      else values[column] = value;
      return { ...row, values };
    }),
  };
}

export function removeNewRow(state: Pending, id: number): Pending {
  return { ...state, inserts: state.inserts.filter((row) => row.id !== id) };
}

/** Marks rows for deletion, or unmarks them when every one is already marked. */
export function toggleDeletes(state: Pending, rows: number[]): Pending {
  if (rows.length === 0) return state;
  const marked = new Set(state.deletes);
  const all = rows.every((row) => marked.has(row));
  for (const row of rows) {
    if (all) marked.delete(row);
    else marked.add(row);
  }
  return { ...state, deletes: [...marked].sort((a, b) => a - b) };
}

export interface PendingCounts {
  changed: number;
  added: number;
  deleted: number;
}

export function counts(state: Pending): PendingCounts {
  const deleted = new Set(state.deletes);
  return {
    changed: Object.keys(state.edits).filter((row) => !deleted.has(Number(row))).length,
    added: state.inserts.length,
    deleted: deleted.size,
  };
}

export function hasPending(state: Pending): boolean {
  const c = counts(state);
  return c.changed + c.added + c.deleted > 0;
}

/**
 * The change set Rust plans from. Keys are the loaded primary key values; Rust checks them again
 * and decides the SQL. A deleted row's edits are dropped.
 */
export function toChangeSet(state: Pending, columns: ColumnMeta[], rows: Value[][], primaryKey: string[]): ChangeSet {
  const position = (name: string) => columns.findIndex((c) => c.name === name);
  const keyOf = (row: Value[]): CellValue[] =>
    primaryKey.map((name) => ({ column: name, value: rawValue(row[position(name)]) }));
  const deleted = new Set(state.deletes);

  const updates = Object.entries(state.edits)
    .map(([index, cells]) => [Number(index), cells] as const)
    .filter(([index]) => !deleted.has(index) && rows[index])
    .sort(([a], [b]) => a - b)
    .map(([index, cells]) => {
      const row = rows[index] ?? [];
      const edited = Object.keys(cells)
        .map(Number)
        .sort((a, b) => a - b);
      return {
        key: keyOf(row),
        before: edited.map((c) => ({ column: columns[c]?.name ?? '', value: rawValue(row[c]) })),
        after: edited.map((c) => ({ column: columns[c]?.name ?? '', value: cells[c] ?? null })),
      };
    });

  const inserts = [...state.inserts].reverse().map((row) =>
    Object.keys(row.values)
      .map(Number)
      .sort((a, b) => a - b)
      .map((c) => ({ column: columns[c]?.name ?? '', value: row.values[c] ?? null })),
  );

  const deletes = state.deletes.flatMap((index) => (rows[index] ? [keyOf(rows[index])] : []));
  return { updates, inserts, deletes };
}
