import { expect, it } from 'vitest';
import { BOX_WIDTH, tableHeight, type DiagramTable } from '../lib/diagram';
import { diagramBounds, standaloneSvg } from '../lib/diagramExport';

const tables: DiagramTable[] = [
  { name: 'orders', columns: [{ name: 'id', type: 'int', primaryKey: true }], keys: [] },
  { name: 'items', columns: [], keys: [] },
];

it('covers every table wherever it was dragged, with a margin', () => {
  const bounds = diagramBounds(tables, { orders: { x: -100, y: 50 }, items: { x: 400, y: 300 } });
  expect(bounds).toEqual({
    x: -124,
    y: 26,
    width: 400 + BOX_WIDTH + 100 + 48,
    height: 300 + tableHeight(tables[1] as DiagramTable) - 50 + 48,
  });
  expect(diagramBounds(tables, {})).toBeNull();
});

it('writes a standalone light SVG without pan, zoom, theme variables or classes', () => {
  const ns = 'http://www.w3.org/2000/svg';
  const g = document.createElementNS(ns, 'g');
  g.setAttribute('transform', 'translate(300,20) scale(0.4)');
  g.innerHTML =
    '<path d="M0 0L10 10" fill="none" stroke="var(--muted)"/>' +
    '<g class="cursor-move" transform="translate(5,5)"><title>orders</title>' +
    '<rect width="280" height="48" fill="var(--raised)" stroke="var(--line)"/>' +
    '<text fill="var(--ink)">orders</text><text fill="var(--unknown)">x</text></g>';

  const svg = standaloneSvg(g, { x: -24, y: -24, width: 400, height: 200 });

  expect(svg).toContain('viewBox="-24 -24 400 200"');
  expect(svg).toContain('width="400"');
  expect(svg).not.toContain('scale(0.4)');
  expect(svg).toContain('transform="translate(5,5)"');
  expect(svg).not.toContain('var(--');
  expect(svg).not.toContain('class=');
  expect(svg).not.toContain('<title>');
  expect(svg).toContain('fill="#ffffff"');
  expect(svg).toContain('stroke="#d3dce3"');
  expect(svg).toContain('fill="#15212c"');
  expect(svg).toContain('stroke="#4f6070"');
  expect(svg).toContain('font-family="Atkinson Hyperlegible, sans-serif"');
  // The page's own drawing is left alone.
  expect(g.getAttribute('transform')).toBe('translate(300,20) scale(0.4)');
});
