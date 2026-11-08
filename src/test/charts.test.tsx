import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChartSpec, ColumnMeta, Value } from '../bindings';

const mocks = vi.hoisted(() => ({ exportDiagram: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: class {} }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({ commands: mocks, events: {} }));

import { QueryResult } from '../components/ai/AiTab';
import { niceTicks, ResultChart } from '../components/ai/ResultChart';
import { buildChart, initialView, type BuiltChart, type ChartType } from '../lib/chart';
import type { QueryPart } from '../lib/chat';

const col = (name: string, typeName = 'text'): ColumnMeta => ({ name, typeName, nullable: null });
const text = (v: string): Value => ({ t: 'Text', v });
const int = (v: number): Value => ({ t: 'Int', v });

const columns = [col('region'), col('sales', 'int4'), col('returns', 'int4')];
const rows: Value[][] = [
  [text('north'), int(40), int(4)],
  [text('south'), int(30), int(3)],
  [text('east'), int(20), int(2)],
];

const built = (type: ChartType, c = columns, r = rows, spec: ChartSpec | null = null): BuiltChart => {
  const result = buildChart(type, c, r, spec);
  if (!('chart' in result)) throw new Error(result.reason);
  return result.chart;
};
const reason = (type: ChartType, c: ColumnMeta[], r: Value[][], spec: ChartSpec | null = null) => {
  const result = buildChart(type, c, r, spec);
  return 'reason' in result ? result.reason : null;
};

describe('building charts', () => {
  it('guesses labels and numeric series, skipping ids', () => {
    const chart = built('bar', [col('id', 'int4'), ...columns], rows.map((r, i) => [int(i + 1), ...r]));
    expect(chart.labels).toEqual(['north', 'south', 'east']);
    expect(chart.series.map((s) => s.name)).toEqual(['sales', 'returns']);
    expect(chart.title).toBe('sales, returns by region');
  });

  it('follows the columns the assistant named', () => {
    const spec: ChartSpec = { kind: 'bar', x: 'region', y: ['returns'], title: 'Returns' };
    const chart = built('bar', columns, rows, spec);
    expect(chart.series).toEqual([{ name: 'returns', values: [4, 3, 2] }]);
    expect(chart.title).toBe('Returns');
  });

  it('takes one series for a pie, largest first', () => {
    const chart = built('pie', columns, [[text('a'), int(1), int(0)], [text('b'), int(5), int(0)]]);
    expect(chart.labels).toEqual(['b', 'a']);
    expect(chart.series).toHaveLength(1);
  });

  it('folds a pie past six parts into Other', () => {
    const many = Array.from({ length: 9 }, (_, i) => [text(`p${i}`), int(100 - i), int(0)]);
    const chart = built('donut', columns, many);
    expect(chart.labels).toHaveLength(6);
    expect(chart.labels.at(-1)).toBe('Other');
    expect(chart.series[0]?.values.at(-1)).toBe(95 + 94 + 93 + 92);
    expect(chart.folded).toBe(true);
  });

  it('says why a result does not fit', () => {
    expect(reason('pie', columns, [[text('a'), int(-1), int(0)], [text('b'), int(2), int(0)]])).toMatch(/negative/);
    expect(reason('pie', columns, [[text('a'), int(3), int(0)]])).toMatch(/at least two/);
    expect(reason('stacked_bar', [col('region'), col('sales', 'int4')], rows.map((r) => r.slice(0, 2)))).toMatch(/two or more/);
    expect(reason('bar', [col('a'), col('b')], [[text('x'), text('y')]])).toMatch(/numeric/);
    expect(reason('line', columns, rows.slice(0, 1))).toMatch(/at least two/);
    expect(reason('bar', columns, [])).toMatch(/no rows/);
    const many = Array.from({ length: 41 }, (_, i) => [text(`r${i}`), int(i), int(i)]);
    expect(reason('bar', columns, many)).toMatch(/at most 40 rows/);
    expect(reason('horizontal_bar', columns, many)).toBeNull();
    expect(reason('scatter', [col('a'), col('b', 'int4')], [[text('x'), int(1)], [text('y'), int(2)]])).not.toBeNull();
  });

  it('plots two numeric columns as a scatter', () => {
    const chart = built('scatter', [col('price', 'int4'), col('sold', 'int4')], [[int(1), int(10)], [int(2), int(20)]]);
    expect(chart.xValues).toEqual([1, 2]);
    expect(chart.series).toEqual([{ name: 'sold', values: [10, 20] }]);
  });

  it('opens on the assistant’s choice when it fits, and on the table when nothing does', () => {
    expect(initialView(columns, rows, { kind: 'pie', x: 'region', y: ['sales'], title: null })).toBe('pie');
    expect(initialView(columns, rows, { kind: 'pie', x: 'region', y: ['nope'], title: null })).toBe('bar');
    expect(initialView([col('a'), col('b')], [[text('x'), text('y')]], null)).toBe('table');
  });

  it('ticks are clean numbers covering the data', () => {
    expect(niceTicks(0, 87)).toEqual([0, 25, 50, 75, 100]);
    expect(niceTicks(-12, 30)).toEqual([-20, 0, 20, 40]);
    expect(niceTicks(5, 5)[0]).toBe(0);
  });
});

describe('drawing charts', () => {
  it('a pie lists every part with its share, so no slice relies on colour', () => {
    render(<ResultChart chart={built('pie', columns, rows)} />);
    expect(screen.getByRole('img', { name: 'sales by region' })).toBeInTheDocument();
    expect(screen.getByText('north')).toBeInTheDocument();
    expect(screen.getByText(/44%/)).toBeInTheDocument();
    expect(document.querySelectorAll('path[fill^="var(--chart-"]')).toHaveLength(3);
  });

  it('two series get a legend; one series does not', () => {
    const { unmount } = render(<ResultChart chart={built('bar')} />);
    expect(screen.getByText('returns')).toBeInTheDocument();
    unmount();
    render(<ResultChart chart={built('bar', columns, rows, { kind: 'bar', x: 'region', y: ['sales'], title: null })} />);
    expect(screen.queryByText('returns')).toBeNull();
  });

  it('every form draws without errors', () => {
    for (const type of ['bar', 'horizontal_bar', 'stacked_bar', 'line', 'area', 'pie', 'donut'] as const) {
      const { unmount, container } = render(<ResultChart chart={built(type)} />);
      expect(container.querySelector('svg')?.innerHTML).not.toContain('NaN');
      unmount();
    }
    const { container } = render(
      <ResultChart chart={built('scatter', [col('price', 'int4'), col('sold', 'int4')], [[int(1), int(10)], [int(2), int(20)]])} />,
    );
    expect(container.querySelectorAll('circle')).toHaveLength(2);
  });
});

describe('the result in the chat', () => {
  const part = (chart: ChartSpec | null): QueryPart => ({
    id: 'q',
    sql: 'SELECT region, sales, returns FROM t',
    status: 'done',
    readOnly: true,
    reason: null,
    riskReasons: [],
    columns,
    rows,
    rowsAffected: null,
    truncated: false,
    elapsedMs: 3,
    error: null,
    chart,
  });

  beforeEach(() => mocks.exportDiagram.mockReset());

  it('shows the chart the assistant asked for, and lets the user switch', () => {
    render(<QueryResult query={part({ kind: 'donut', x: 'region', y: ['sales'], title: 'Sales share' })} />);
    expect(screen.getByRole('img', { name: 'Sales share' })).toBeInTheDocument();
    const chooser = screen.getByLabelText('Show as') as HTMLSelectElement;
    expect(chooser.value).toBe('donut');
    fireEvent.change(chooser, { target: { value: 'table' } });
    expect(screen.queryByRole('img')).toBeNull();
    expect(screen.getByText('north')).toBeInTheDocument();
  });

  it('greys out forms that do not fit, with the reason', () => {
    render(<QueryResult query={part(null)} />);
    const pie = screen.getByRole('option', { name: /^Stacked bar/ }) as HTMLOptionElement;
    expect(pie.disabled).toBe(false);
    const negative = [...rows, [text('west'), int(-5), int(1)]];
    render(<QueryResult query={{ ...part(null), rows: negative }} />);
    const options = screen.getAllByRole('option', { name: /^Pie/ }) as HTMLOptionElement[];
    expect(options.at(-1)?.disabled).toBe(true);
    expect(options.at(-1)?.textContent).toMatch(/negative/);
  });

  it('saves the chart through Rust as a standalone light SVG', async () => {
    mocks.exportDiagram.mockResolvedValue({ status: 'ok', data: null });
    render(<QueryResult query={part({ kind: 'bar', x: 'region', y: ['sales'], title: 'Sales' })} />);
    fireEvent.change(screen.getByLabelText('Save chart'), { target: { value: 'Pdf' } });
    await waitFor(() => expect(mocks.exportDiagram).toHaveBeenCalledOnce());
    const [svg, format, name] = mocks.exportDiagram.mock.calls[0] ?? [];
    expect(format).toBe('Pdf');
    expect(name).toBe('Sales');
    expect(svg).toContain('viewBox="0 0 640 300"');
    expect(svg).not.toContain('var(--');
    expect(svg).toContain('#2a78d6');
  });
});
