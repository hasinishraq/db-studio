import type { ChartKind, ChartSpec, ColumnMeta, Value } from '../bindings';
import { cellText } from './resultStore';

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

// ---------------------------------------------------------------------------------------------
// Charts the user or the assistant picks. The assistant names columns only; values always come
// from the rows the database returned.

export type ChartType = ChartKind;

export const CHART_TYPES: { type: ChartType; label: string }[] = [
  { type: 'bar', label: 'Bar' },
  { type: 'horizontal_bar', label: 'Horizontal bar' },
  { type: 'stacked_bar', label: 'Stacked bar' },
  { type: 'line', label: 'Line' },
  { type: 'area', label: 'Area' },
  { type: 'pie', label: 'Pie' },
  { type: 'donut', label: 'Donut' },
  { type: 'scatter', label: 'Scatter' },
];

/** Categorical slots, in fixed order. Past this, series are cut and pie slices fold to Other. */
export const MAX_SERIES = 5;
/** One slice per categorical colour, so no hue repeats; the tail folds into a grey Other. */
const MAX_PIE_SLICES = MAX_SERIES;
const MAX_SCATTER_SERIES = 3;
const LIMITS: Partial<Record<ChartType, number>> = {
  bar: 40,
  stacked_bar: 40,
  horizontal_bar: 50,
  line: 1000,
  area: 1000,
  scatter: 2000,
};

export interface Series {
  name: string;
  values: (number | null)[];
}

export interface BuiltChart {
  type: ChartType;
  title: string;
  /** What the labels are, for the axis and the table view. */
  x: string;
  labels: string[];
  series: Series[];
  /** Scatter only: the numeric x of each point. */
  xValues: number[];
  /** Pie only: whether the tail was folded into "Other". */
  folded: boolean;
}

export type ChartResult = { chart: BuiltChart } | { reason: string };

/** Columns picked by the assistant, or guessed: the first label column and the numeric ones. */
function pickColumns(type: ChartType, columns: ColumnMeta[], rows: Value[][], spec: ChartSpec | null) {
  const index = (name: string) => columns.findIndex((c) => c.name === name);
  if (spec) {
    const y = spec.y.map(index).filter((i) => i >= 0);
    return { x: index(spec.x), y };
  }
  const first = rows[0] ?? [];
  const isNumber = (i: number) => rows.some((row) => numeric(row[i]) !== null) && numeric(first[i]) !== null;
  const measures = columns.map((_, i) => i).filter((i) => isNumber(i) && !isIdentifier(columns[i]?.name ?? ''));
  if (type === 'scatter') {
    const [x, ...y] = measures;
    return { x: x ?? -1, y };
  }
  // Text or dates label best; an id is a label too, but only when nothing else is.
  let x = columns.findIndex((_, i) => !measures.includes(i) && label(first[i]) !== null && numeric(first[i]) === null);
  if (x < 0) x = columns.findIndex((_, i) => !measures.includes(i) && label(first[i]) !== null);
  // Every column is a number (a year and a total, say): the first one labels the rest.
  if (x < 0 && columns.length >= 2) x = 0;
  return { x, y: measures.filter((i) => i !== x) };
}

/** Builds a chart of `type`, or says plainly why this result does not fit it. */
export function buildChart(type: ChartType, columns: ColumnMeta[], rows: Value[][], spec: ChartSpec | null = null): ChartResult {
  if (rows.length === 0) return { reason: 'There are no rows to plot.' };
  const picked = pickColumns(type, columns, rows, spec);
  const name = (i: number) => columns[i]?.name ?? '';
  if (picked.x < 0) return { reason: 'It needs a column of labels.' };
  let y = picked.y.filter((i) => i !== picked.x);
  if (y.length === 0) return { reason: 'It needs a numeric column to plot.' };
  const title = spec?.title ?? `${y.slice(0, type === 'pie' || type === 'donut' ? 1 : MAX_SERIES).map(name).join(', ')} by ${name(picked.x)}`;
  const limit = LIMITS[type];
  if (limit !== undefined && rows.length > limit) {
    return { reason: `It shows at most ${limit} rows; this result has ${rows.length.toLocaleString()}.` };
  }

  if (type === 'scatter') {
    y = y.slice(0, MAX_SCATTER_SERIES);
    const xValues = rows.map((row) => numeric(row[picked.x]));
    if (xValues.some((v) => v === null)) return { reason: `It needs numbers in ${name(picked.x)} for the x axis.` };
    return {
      chart: {
        type,
        title,
        x: name(picked.x),
        labels: rows.map((row) => cellText(row[picked.x])),
        series: y.map((i) => ({ name: name(i), values: rows.map((row) => numeric(row[i])) })),
        xValues: xValues as number[],
        folded: false,
      },
    };
  }

  const labels = rows.map((row) => (row[picked.x]?.t === 'Null' ? 'NULL' : cellText(row[picked.x])));

  if (type === 'pie' || type === 'donut') {
    const column = y[0] as number;
    const values = rows.map((row) => numeric(row[column]));
    if (values.some((v) => v !== null && v < 0)) return { reason: 'A pie cannot show negative values.' };
    let slices = labels
      .map((text, i) => ({ text, value: values[i] ?? 0 }))
      .filter((slice) => slice.value > 0)
      .sort((a, b) => b.value - a.value);
    if (slices.length < 2) return { reason: 'A pie needs at least two parts above zero.' };
    const folded = slices.length > MAX_PIE_SLICES;
    if (folded) {
      const rest = slices.slice(MAX_PIE_SLICES - 1).reduce((sum, slice) => sum + slice.value, 0);
      slices = [...slices.slice(0, MAX_PIE_SLICES - 1), { text: 'Other', value: rest }];
    }
    return {
      chart: {
        type,
        title,
        x: name(picked.x),
        labels: slices.map((s) => s.text),
        series: [{ name: name(column), values: slices.map((s) => s.value) }],
        xValues: [],
        folded,
      },
    };
  }

  y = y.slice(0, MAX_SERIES);
  const series = y.map((i) => ({ name: name(i), values: rows.map((row) => numeric(row[i])) }));
  if ((type === 'line' || type === 'area') && rows.length < 2) return { reason: 'A line needs at least two rows.' };
  if (type === 'stacked_bar') {
    if (series.length < 2) return { reason: 'Stacking needs two or more value columns.' };
    if (series.some((s) => s.values.some((v) => v !== null && v < 0))) return { reason: 'Stacked bars cannot show negative values.' };
  }
  return { chart: { type, title, x: name(picked.x), labels, series, xValues: [], folded: false } };
}

/** The chart shown first: the assistant's choice when it fits, else a guess, else the table. */
export function initialView(columns: ColumnMeta[], rows: Value[][], spec: ChartSpec | null): ChartType | 'table' {
  if (spec && 'chart' in buildChart(spec.kind, columns, rows, spec)) return spec.kind;
  const guess = chooseChart(columns, rows);
  if (guess.kind === 'line' || guess.kind === 'bar') return guess.kind;
  return 'table';
}
