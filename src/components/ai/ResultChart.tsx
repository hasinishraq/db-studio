import { useRef, useState, type MouseEvent, type ReactNode, type RefObject } from 'react';
import type { BuiltChart } from '../../lib/chart';

const W = 640;
const LEFT = 56;
const RIGHT = 16;
const BOTTOM = 28;
const BAR_MAX = 24;
const GAP = 2;

const color = (index: number) => `var(--chart-${(index % 5) + 1})`;

/** Clean tick values covering `min`..`max`. */
export function niceTicks(min: number, max: number, count = 4): number[] {
  if (min === max) {
    max = min === 0 ? 1 : min + Math.abs(min);
    if (min > 0) min = 0;
  }
  const raw = (max - min) / count;
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 2.5, 5, 10].find((m) => m * power >= raw) ?? 10) * power;
  const ticks: number[] = [];
  for (let t = Math.floor(min / step) * step; t <= max + step * 1e-9; t += step) ticks.push(Number(t.toPrecision(12)));
  if ((ticks.at(-1) ?? 0) < max) ticks.push(Number(((ticks.at(-1) ?? 0) + step).toPrecision(12)));
  return ticks;
}

/** Full numbers under 10,000, compact above, one style per axis. */
export function formatter(ticks: number[]): (n: number) => string {
  const compact = Math.max(...ticks.map(Math.abs)) >= 10_000;
  const style = new Intl.NumberFormat(undefined, compact ? { notation: 'compact', maximumFractionDigits: 1 } : { maximumFractionDigits: 2 });
  return (n) => style.format(n);
}

const short = (text: string, length: number) => (text.length > length ? `${text.slice(0, length - 1)}…` : text);

/** A bar with a 4px rounded data end and a square end on the baseline. */
function barPath(along: number, from: number, to: number, thickness: number, vertical: boolean): string {
  const length = Math.abs(to - from);
  const r = Math.min(4, thickness / 2, length);
  const dir = to < from ? -1 : 1;
  if (vertical) {
    const [x0, x1] = [along, along + thickness];
    const end = to;
    return `M${x0},${from} L${x0},${end - dir * r} Q${x0},${end} ${x0 + r},${end} L${x1 - r},${end} Q${x1},${end} ${x1},${end - dir * r} L${x1},${from} Z`;
  }
  const [y0, y1] = [along, along + thickness];
  return `M${from},${y0} L${to - dir * r},${y0} Q${to},${y0} ${to},${y0 + r} L${to},${y1 - r} Q${to},${y1} ${to - dir * r},${y1} L${from},${y1} Z`;
}

interface Tip {
  x: number;
  y: number;
  lines: string[];
}

function Legend({ chart, y }: { chart: BuiltChart; y: number }) {
  let x = LEFT;
  return (
    <g>
      {chart.series.map((series, index) => {
        const at = x;
        x += 24 + Math.min(series.name.length, 20) * 7;
        return (
          <g key={series.name} transform={`translate(${at},${y})`}>
            <rect width={10} height={10} rx={2} fill={color(index)} />
            <text x={15} y={9} fontSize={11} fill="var(--muted)">
              {short(series.name, 20)}
            </text>
          </g>
        );
      })}
    </g>
  );
}

function YAxis({ ticks, scale, format, width }: { ticks: number[]; scale: (n: number) => number; format: (n: number) => string; width: number }) {
  return (
    <g>
      {ticks.map((tick) => (
        <g key={tick}>
          <line x1={LEFT} x2={width - RIGHT} y1={scale(tick)} y2={scale(tick)} stroke="var(--line)" strokeWidth={1} />
          <text x={LEFT - 6} y={scale(tick) + 4} textAnchor="end" fontSize={11} fill="var(--muted)">
            {format(tick)}
          </text>
        </g>
      ))}
    </g>
  );
}

function XLabels({ labels, center, y }: { labels: string[]; center: (i: number) => number; y: number }) {
  const room = Math.max(1, Math.floor((W - LEFT - RIGHT) / 64));
  const every = Math.ceil(labels.length / room);
  return (
    <g>
      {labels.map((text, i) =>
        i % every === 0 ? (
          <text key={i} x={center(i)} y={y} textAnchor="middle" fontSize={11} fill="var(--muted)">
            {short(text, 10)}
          </text>
        ) : null,
      )}
    </g>
  );
}

/** Bars, grouped bars, stacked bars, lines and areas, on one category axis. */
function Cartesian({ chart, top, onTip }: { chart: BuiltChart; top: number; onTip: (tip: Tip | null) => void }) {
  const height = 300;
  const plotTop = top;
  const plotBottom = height - BOTTOM;
  const n = chart.labels.length;
  const stacked = chart.type === 'stacked_bar';
  const all = stacked
    ? chart.labels.map((_, i) => chart.series.reduce((sum, s) => sum + (s.values[i] ?? 0), 0))
    : chart.series.flatMap((s) => s.values.filter((v): v is number => v !== null));
  const ticks = niceTicks(Math.min(0, ...all), Math.max(0, ...all));
  const [low, high] = [ticks[0] ?? 0, ticks.at(-1) ?? 1];
  const scale = (v: number) => plotBottom - ((v - low) / (high - low || 1)) * (plotBottom - plotTop);
  const format = formatter(ticks);
  const band = (W - LEFT - RIGHT) / n;
  const center = (i: number) => LEFT + band * i + band / 2;
  const line = chart.type === 'line' || chart.type === 'area';
  const [hover, setHover] = useState<number | null>(null);

  const tipAt = (i: number, event: MouseEvent) => {
    setHover(i);
    const lines = chart.series.map((s) => `${s.name}: ${s.values[i] === null ? 'NULL' : format(s.values[i] ?? 0)}`);
    onTip({ x: event.clientX, y: event.clientY, lines: [chart.labels[i] ?? '', ...lines] });
  };

  const k = chart.series.length;
  const thickness = stacked || k === 1 ? Math.min(BAR_MAX, band * 0.7) : Math.min(BAR_MAX, (band * 0.8 - GAP * (k - 1)) / k);
  const group = stacked ? thickness : thickness * k + GAP * (k - 1);

  return (
    <g>
      <YAxis ticks={ticks} scale={scale} format={format} width={W} />
      <line x1={LEFT} x2={W - RIGHT} y1={scale(0)} y2={scale(0)} stroke="var(--muted)" strokeWidth={1} />
      {line &&
        chart.series.map((series, s) => {
          const points = series.values.map((v, i) => (v === null ? null : `${center(i)},${scale(v)}`));
          const path = points.reduce<string>((d, p, i) => (p ? `${d}${d && points[i - 1] ? 'L' : 'M'}${p}` : d), '');
          return (
            <g key={series.name}>
              {chart.type === 'area' && (
                <path
                  d={`${path} L${center(n - 1)},${scale(0)} L${center(0)},${scale(0)} Z`}
                  fill={color(s)}
                  fillOpacity={0.1}
                  stroke="none"
                />
              )}
              <path d={path} fill="none" stroke={color(s)} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
              {hover !== null && series.values[hover] !== null && (
                <circle cx={center(hover)} cy={scale(series.values[hover] ?? 0)} r={4} fill={color(s)} stroke="var(--chart-surface)" strokeWidth={2} />
              )}
            </g>
          );
        })}
      {line && hover !== null && <line x1={center(hover)} x2={center(hover)} y1={plotTop} y2={plotBottom} stroke="var(--muted)" strokeWidth={1} />}
      {!line &&
        chart.labels.map((_, i) => {
          let base = 0;
          return (
            <g key={i}>
              {chart.series.map((series, s) => {
                const value = series.values[i];
                if (value === null || value === undefined) return null;
                if (stacked) {
                  const from = scale(base);
                  base += value;
                  const to = scale(base);
                  const topmost = chart.series.slice(s + 1).every((later) => !later.values[i]);
                  // A 2px gap of surface separates a segment from the one above it.
                  const end = topmost ? to : Math.min(from, to + GAP);
                  if (from - end <= 0) return null;
                  return topmost ? (
                    <path key={s} d={barPath(center(i) - thickness / 2, from, end, thickness, true)} fill={color(s)} />
                  ) : (
                    <rect key={s} x={center(i) - thickness / 2} y={end} width={thickness} height={from - end} fill={color(s)} />
                  );
                }
                const x = center(i) - group / 2 + s * (thickness + GAP);
                return <path key={s} d={barPath(x, scale(0), scale(value), thickness, true)} fill={color(s)} />;
              })}
            </g>
          );
        })}
      <XLabels labels={chart.labels} center={center} y={height - 8} />
      {chart.labels.map((_, i) => (
        <rect
          key={i}
          x={LEFT + band * i}
          y={plotTop}
          width={band}
          height={plotBottom - plotTop}
          fill="transparent"
          onMouseMove={(e) => tipAt(i, e)}
          onMouseLeave={() => {
            setHover(null);
            onTip(null);
          }}
        />
      ))}
    </g>
  );
}

/** Ranked bars, one row each, for many or long labels. */
function Horizontal({ chart, top, onTip }: { chart: BuiltChart; top: number; onTip: (tip: Tip | null) => void }) {
  const row = 22;
  const labelWidth = 128;
  const n = chart.labels.length;
  const k = chart.series.length;
  const height = top + n * row + BOTTOM;
  const values = chart.series.flatMap((s) => s.values.filter((v): v is number => v !== null));
  const ticks = niceTicks(Math.min(0, ...values), Math.max(0, ...values));
  const [low, high] = [ticks[0] ?? 0, ticks.at(-1) ?? 1];
  const right = W - RIGHT - 48;
  const scale = (v: number) => labelWidth + ((v - low) / (high - low || 1)) * (right - labelWidth);
  const format = formatter(ticks);
  const thickness = Math.min(BAR_MAX, (row * 0.8 - GAP * (k - 1)) / k);
  return (
    <g>
      {ticks.map((tick) => (
        <g key={tick}>
          <line x1={scale(tick)} x2={scale(tick)} y1={top} y2={height - BOTTOM} stroke="var(--line)" strokeWidth={1} />
          <text x={scale(tick)} y={height - 8} textAnchor="middle" fontSize={11} fill="var(--muted)">
            {format(tick)}
          </text>
        </g>
      ))}
      <line x1={scale(0)} x2={scale(0)} y1={top} y2={height - BOTTOM} stroke="var(--muted)" strokeWidth={1} />
      {chart.labels.map((text, i) => {
        const y = top + i * row + (row - (thickness * k + GAP * (k - 1))) / 2;
        return (
          <g
            key={i}
            onMouseMove={(e) =>
              onTip({
                x: e.clientX,
                y: e.clientY,
                lines: [text, ...chart.series.map((s) => `${s.name}: ${s.values[i] === null ? 'NULL' : format(s.values[i] ?? 0)}`)],
              })
            }
            onMouseLeave={() => onTip(null)}
          >
            <rect x={0} y={top + i * row} width={W} height={row} fill="transparent" />
            <text x={labelWidth - 8} y={top + i * row + row / 2 + 4} textAnchor="end" fontSize={11} fill="var(--ink)">
              {short(text, 18)}
            </text>
            {chart.series.map((series, s) => {
              const value = series.values[i];
              if (value === null || value === undefined) return null;
              return <path key={s} d={barPath(y + s * (thickness + GAP), scale(0), scale(value), thickness, false)} fill={color(s)} />;
            })}
            {k === 1 && n <= 20 && chart.series[0]?.values[i] !== null && (
              <text x={scale(chart.series[0]?.values[i] ?? 0) + 6} y={top + i * row + row / 2 + 4} fontSize={11} fill="var(--muted)">
                {format(chart.series[0]?.values[i] ?? 0)}
              </text>
            )}
          </g>
        );
      })}
    </g>
  );
}

/** Part of a whole. Shares and names sit in the legend, so no slice relies on colour alone. */
function Pie({ chart, top, onTip }: { chart: BuiltChart; top: number; onTip: (tip: Tip | null) => void }) {
  const height = Math.max(260, top + chart.labels.length * 24 + 24);
  const values = chart.series[0]?.values.map((v) => v ?? 0) ?? [];
  const total = values.reduce((a, b) => a + b, 0) || 1;
  const radius = 100;
  const [cx, cy] = [LEFT + radius, top + (height - top) / 2];
  const inner = chart.type === 'donut' ? radius * 0.6 : 0;
  const percent = (v: number) => `${((v / total) * 100).toFixed(v / total < 0.1 ? 1 : 0)}%`;
  const fill = (i: number) => (chart.folded && i === chart.labels.length - 1 ? 'var(--chart-other)' : color(i));
  let angle = -Math.PI / 2;
  const slices = values.map((value, i) => {
    const start = angle;
    angle += (value / total) * Math.PI * 2;
    return { i, value, start, end: angle };
  });
  const point = (r: number, a: number) => `${cx + r * Math.cos(a)},${cy + r * Math.sin(a)}`;
  const format = formatter(values);
  return (
    <g>
      {slices.map(({ i, value, start, end }) => {
        const large = end - start > Math.PI ? 1 : 0;
        const whole = end - start >= Math.PI * 2 - 1e-9;
        const d = whole
          ? `M${point(radius, start)} A${radius},${radius} 0 1 1 ${point(radius, start + Math.PI)} A${radius},${radius} 0 1 1 ${point(radius, start)} Z`
          : inner
            ? `M${point(radius, start)} A${radius},${radius} 0 ${large} 1 ${point(radius, end)} L${point(inner, end)} A${inner},${inner} 0 ${large} 0 ${point(inner, start)} Z`
            : `M${cx},${cy} L${point(radius, start)} A${radius},${radius} 0 ${large} 1 ${point(radius, end)} Z`;
        return (
          <path
            key={i}
            d={d}
            fill={fill(i)}
            stroke="var(--chart-surface)"
            strokeWidth={GAP}
            strokeLinejoin="round"
            onMouseMove={(e) =>
              onTip({ x: e.clientX, y: e.clientY, lines: [chart.labels[i] ?? '', `${format(value)} (${percent(value)})`] })
            }
            onMouseLeave={() => onTip(null)}
          />
        );
      })}
      {chart.labels.map((text, i) => (
        <g key={i} transform={`translate(${cx + radius + 40},${top + 8 + i * 24})`}>
          <rect width={10} height={10} rx={2} fill={fill(i)} />
          <text x={16} y={9} fontSize={12} fill="var(--ink)">
            {short(text, 28)}
          </text>
          <text x={W - (cx + radius + 40) - RIGHT} y={9} textAnchor="end" fontSize={12} fill="var(--muted)">
            {format(values[i] ?? 0)} · {percent(values[i] ?? 0)}
          </text>
        </g>
      ))}
    </g>
  );
}

/** Two value axes. */
function Scatter({ chart, top, onTip }: { chart: BuiltChart; top: number; onTip: (tip: Tip | null) => void }) {
  const height = 300;
  const plotBottom = height - BOTTOM;
  const ys = chart.series.flatMap((s) => s.values.filter((v): v is number => v !== null));
  const yTicks = niceTicks(Math.min(...ys), Math.max(...ys));
  const xTicks = niceTicks(Math.min(...chart.xValues), Math.max(...chart.xValues));
  const sy = (v: number) => plotBottom - ((v - (yTicks[0] ?? 0)) / ((yTicks.at(-1) ?? 1) - (yTicks[0] ?? 0) || 1)) * (plotBottom - top);
  // The last x label is centred on its tick, so the plot stops short of the edge to fit it.
  const sx = (v: number) => LEFT + ((v - (xTicks[0] ?? 0)) / ((xTicks.at(-1) ?? 1) - (xTicks[0] ?? 0) || 1)) * (W - LEFT - RIGHT - 24);
  const fy = formatter(yTicks);
  const fx = formatter(xTicks);
  return (
    <g>
      <YAxis ticks={yTicks} scale={sy} format={fy} width={W} />
      {xTicks.map((tick) => (
        <text key={tick} x={sx(tick)} y={height - 8} textAnchor="middle" fontSize={11} fill="var(--muted)">
          {fx(tick)}
        </text>
      ))}
      {chart.series.map((series, s) =>
        series.values.map((v, i) =>
          v === null ? null : (
            <circle
              key={`${s}-${i}`}
              cx={sx(chart.xValues[i] ?? 0)}
              cy={sy(v)}
              r={4}
              fill={color(s)}
              stroke="var(--chart-surface)"
              strokeWidth={2}
              onMouseMove={(e) =>
                onTip({ x: e.clientX, y: e.clientY, lines: [`${chart.x}: ${fx(chart.xValues[i] ?? 0)}`, `${series.name}: ${fy(v)}`] })
              }
              onMouseLeave={() => onTip(null)}
            />
          ),
        ),
      )}
    </g>
  );
}

/** Draws a built chart. Everything, title and legend too, sits in one SVG so it saves as an image. */
export function ResultChart({ chart, drawingRef }: { chart: BuiltChart; drawingRef?: RefObject<SVGGElement | null> }) {
  const [tip, setTipState] = useState<Tip | null>(null);
  const figure = useRef<HTMLElement>(null);
  /** Tips arrive in window coordinates; the box is placed inside the figure. */
  const setTip = (next: Tip | null) => {
    const box = figure.current?.getBoundingClientRect();
    setTipState(next && box ? { ...next, x: next.x - box.left, y: next.y - box.top } : null);
  };
  const pie = chart.type === 'pie' || chart.type === 'donut';
  const legend = !pie && chart.series.length > 1;
  const top = chartTop(chart);
  let body: ReactNode;
  if (pie) body = <Pie chart={chart} top={top} onTip={setTip} />;
  else if (chart.type === 'horizontal_bar') body = <Horizontal chart={chart} top={top} onTip={setTip} />;
  else if (chart.type === 'scatter') body = <Scatter chart={chart} top={top} onTip={setTip} />;
  else body = <Cartesian chart={chart} top={top} onTip={setTip} />;

  return (
    <figure ref={figure} className="relative my-2 max-w-2xl">
      <svg viewBox={`0 0 ${W} ${chartHeight(chart, top)}`} className="w-full" role="img" aria-label={chart.title}>
        <g ref={drawingRef} fontFamily="Atkinson Hyperlegible, sans-serif">
          <text x={LEFT} y={18} fontSize={14} fontWeight={700} fill="var(--ink)">
            {short(chart.title, 70)}
          </text>
          {legend && <Legend chart={chart} y={30} />}
          {body}
        </g>
      </svg>
      {chart.folded && <figcaption className="text-xs text-muted">The smallest parts are combined as Other.</figcaption>}
      {tip && (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-10 rounded border border-line bg-panel px-2 py-1 text-xs shadow-pop"
          style={{ left: Math.min(tip.x + 12, 520), top: tip.y + 12 }}
        >
          {tip.lines.map((text, i) => (
            <div key={i} className={i === 0 ? 'font-bold' : 'text-muted'}>
              {text}
            </div>
          ))}
        </div>
      )}
    </figure>
  );
}

/** Room above the plot: the title, and the legend when there are two or more series. */
export function chartTop(chart: BuiltChart): number {
  const pie = chart.type === 'pie' || chart.type === 'donut';
  return !pie && chart.series.length > 1 ? 52 : 34;
}

/** The height each form draws at, so the outer SVG and the export fit it. */
export function chartHeight(chart: BuiltChart, top: number): number {
  if (chart.type === 'pie' || chart.type === 'donut') return Math.max(260, top + chart.labels.length * 24 + 24);
  if (chart.type === 'horizontal_bar') return top + chart.labels.length * 22 + BOTTOM;
  return 300;
}

export { W as CHART_WIDTH };
