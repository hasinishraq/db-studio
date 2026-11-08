import { describe, expect, it } from 'vitest';
import type { ColumnMeta, Value } from '../bindings';
import {
  addRow,
  counts,
  editCell,
  editNewCell,
  emptyPending,
  hasPending,
  removeNewRow,
  revertCell,
  revertRow,
  toChangeSet,
  toggleDeletes,
} from '../lib/pendingEdits';

const columns: ColumnMeta[] = [
  { name: 'id', typeName: 'int4', nullable: null },
  { name: 'name', typeName: 'text', nullable: null },
  { name: 'note', typeName: 'text', nullable: null },
];
const rows: Value[][] = [
  [{ t: 'Int', v: 1 }, { t: 'Text', v: 'Ann' }, { t: 'Null' }],
  [{ t: 'Int', v: 2 }, { t: 'Text', v: 'Bob' }, { t: 'Text', v: 'x' }],
  [{ t: 'Int', v: 3 }, { t: 'Text', v: 'Cy' }, { t: 'Text', v: 'y' }],
];

describe('pending edits', () => {
  it('drops an edit that goes back to the loaded value', () => {
    let state = editCell(emptyPending(), 0, 1, 'Bea', 'Ann');
    expect(hasPending(state)).toBe(true);
    state = editCell(state, 0, 1, 'Ann', 'Ann');
    expect(state.edits).toEqual({});
    expect(hasPending(state)).toBe(false);
  });

  it('tells NULL from empty text', () => {
    const toNull = editCell(emptyPending(), 0, 2, null, null);
    expect(hasPending(toNull)).toBe(false);
    const toEmpty = editCell(emptyPending(), 0, 2, '', null);
    expect(toEmpty.edits[0]).toEqual({ 2: '' });
  });

  it('reverts a cell and a row', () => {
    let state = editCell(emptyPending(), 1, 1, 'B', 'Bob');
    state = editCell(state, 1, 2, null, 'x');
    state = revertCell(state, 1, 1);
    expect(state.edits[1]).toEqual({ 2: null });
    state = toggleDeletes(state, [1]);
    state = revertRow(state, 1);
    expect(hasPending(state)).toBe(false);
  });

  it('marks deletes and unmarks them when all are marked', () => {
    let state = toggleDeletes(emptyPending(), [2, 0]);
    expect(state.deletes).toEqual([0, 2]);
    state = toggleDeletes(state, [0, 1]);
    expect(state.deletes).toEqual([0, 1, 2]);
    state = toggleDeletes(state, [0, 1]);
    expect(state.deletes).toEqual([2]);
  });

  it('counts a deleted row once, not as changed too', () => {
    let state = editCell(emptyPending(), 0, 1, 'A', 'Ann');
    state = toggleDeletes(state, [0]);
    state = addRow(state);
    expect(counts(state)).toEqual({ changed: 0, added: 1, deleted: 1 });
  });

  it('builds the change set Rust plans from', () => {
    let state = editCell(emptyPending(), 1, 2, null, 'x');
    state = editCell(state, 1, 1, 'Bo', 'Bob');
    state = editCell(state, 0, 1, 'gone', 'Ann');
    state = toggleDeletes(state, [0]);
    state = addRow(state);
    state = editNewCell(state, 1, 1, 'Dee');
    state = editNewCell(state, 1, 2, null);
    state = addRow(state);
    state = removeNewRow(state, 2);
    state = addRow(state);

    expect(toChangeSet(state, columns, rows, ['id'])).toEqual({
      updates: [
        {
          key: [{ column: 'id', value: '2' }],
          before: [
            { column: 'name', value: 'Bob' },
            { column: 'note', value: 'x' },
          ],
          after: [
            { column: 'name', value: 'Bo' },
            { column: 'note', value: null },
          ],
        },
      ],
      inserts: [
        [
          { column: 'name', value: 'Dee' },
          { column: 'note', value: null },
        ],
        [],
      ],
      deletes: [[{ column: 'id', value: '1' }]],
    });
  });

  it('a new cell set back to default is left out', () => {
    let state = addRow(emptyPending());
    state = editNewCell(state, 1, 1, 'a');
    state = editNewCell(state, 1, 1, undefined);
    expect(toChangeSet(state, columns, rows, ['id']).inserts).toEqual([[]]);
  });
});
