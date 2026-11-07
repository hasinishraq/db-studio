import { useState } from 'react';
import type { ChartChoice } from '../../lib/chart';

const WIDTH = 560;
const HEIGHT = 180;
const LEFT = 48;
const BOTTOM = 22;

/** One series, so the title names it and no legend is needed. Colour stays clear of the risk palette. */
export function ResultChart({ chart }: { chart: Exclude<ChartChoice, { kind: 'none' | 'stat' }> }) {
  const [hover, setHover] = useState<number | null>(null);
  const { points } = chart;
  const top = Math.max(...points.map((p) => p.y), 0);
  const bottom = Math.min(...points.map((p) => p.y), 0);
  const span = top - bottom || 1;
  const plotWidth = WIDTH - LEFT;
  const plotHeight = HEIGHT - BOTTOM;
  const yFor = (value: number) => plotHeight - ((value - bottom) / span) * (plotHeight - 8);
  const step = plotWidth / points.length;
  const xFor = (index: number) => LEFT + step * index + step / 2;
  const line = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${xFor(i)},${yFor(p.y)}`).join(' ');
  const active = hover === null ? null : points[hover];

  return (
    <figure className="my-2">
      <figcaption className="mb-1 text-sm text-muted">
        {chart.yLabel} by {chart.xLabel}
      </figcaption>
      <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} className="w-full max-w-xl" role="img" aria-label={`${chart.yLabel} by ${chart.xLabel}`}>
        <line x1={LEFT} x2={WIDTH} y1={yFor(0)} y2={yFor(0)} stroke="var(--line)" />
        <text x={LEFT - 6} y={yFor(top) + 4} textAnchor="end" fontSize={11} fill="var(--muted)">
          {top.toLocaleString()}
        </text>
        <text x={LEFT - 6} y={yFor(bottom) + 4} textAnchor="end" fontSize={11} fill="var(--muted)">
          {bottom.toLocaleString()}
        </text>
        {chart.kind === 'line' && <path d={line} fill="none" stroke="var(--series)" strokeWidth={2} />}
        {points.map((point, index) => {
          const barWidth = Math.max(2, step - 2);
          const height = Math.abs(yFor(point.y) - yFor(0));
          return (
            <g key={index} onMouseEnter={() => setHover(index)} onMouseLeave={() => setHover(null)}>
              {chart.kind === 'bar' ? (
                <rect
                  x={xFor(index) - barWidth / 2}
                  y={Math.min(yFor(point.y), yFor(0))}
                  width={barWidth}
                  height={height}
                  rx={Math.min(4, barWidth / 2)}
                  fill="var(--series)"
                />
              ) : (
                hover === index && <circle cx={xFor(index)} cy={yFor(point.y)} r={4} fill="var(--series)" stroke="var(--panel)" strokeWidth={2} />
              )}
              {/* The hit target is the full column, larger than the mark. */}
              <rect x={xFor(index) - step / 2} y={0} width={step} height={plotHeight} fill="transparent" />
            </g>
          );
        })}
      </svg>
      <p className="h-5 text-sm">{active ? `${active.x}: ${active.y.toLocaleString()}` : ''}</p>
    </figure>
  );
}
