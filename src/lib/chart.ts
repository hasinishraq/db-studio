import type { ColumnMeta, Value } from '../bindings';

export type ChartChoice =
  | { kind: 'none' }
  | { kind: 'stat'; label: string; value: string }
  | { kind: 'line' | 'bar'; xLabel: string; yLabel: string; points: { x: string; y: number }[] };

function numeric(value: Value | undefined): number | null {
  if (!value) return null;
  if (value.t === 'Int' || value.t === 'Float') return value.v;
  if (value.t === 'BigInt' || value.t === 'Decimal') {
    const parsed = Number(value.v);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function label(value: Value | undefined): string | null {
  if (!value || value.t === 'Null') return null;
  return 'v' in value && typeof value.v !== 'object' ? String(value.v) : null;
}

const MAX_POINTS = 60;

/** An id is a label, not a measure, so plotting it says nothing. */
function isIdentifier(name: string): boolean {
  return /(^|_)id$/i.test(name) || /[a-z]Id$/.test(name);
}

/** Picks the form the data's job calls for: one number, a change over time, or a comparison. */
export function chooseChart(columns: ColumnMeta[], rows: Value[][]): ChartChoice {
  const first = rows[0];
  if (!first) return { kind: 'none' };
  if (rows.length === 1 && columns.length === 1) {
    const value = numeric(first[0]);
    if (value !== null) return { kind: 'stat', label: columns[0]?.name ?? '', value: value.toLocaleString() };
  }
  if (columns.length < 2 || rows.length < 2 || rows.length > MAX_POINTS) return { kind: 'none' };
  const y = columns.findIndex((column, i) => numeric(first[i]) !== null && !isIdentifier(column.name));
  const x = columns.findIndex((_, i) => i !== y && label(first[i]) !== null);
  if (y < 0 || x < 0) return { kind: 'none' };
  const points = rows.flatMap((row) => {
    const px = label(row[x]);
    const py = numeric(row[y]);
    return px !== null && py !== null ? [{ x: px, y: py }] : [];
  });
  const kind = first[x]?.t === 'Temporal' ? 'line' : 'bar';
  return { kind, xLabel: columns[x]?.name ?? '', yLabel: columns[y]?.name ?? '', points };
}
