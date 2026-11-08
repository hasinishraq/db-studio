import { BOX_WIDTH, tableHeight, type DiagramTable, type Positions } from './diagram';

/** Exports are always light on white, which prints well. Values match the light theme. */
export const EXPORT_COLORS: Record<string, string> = {
  ground: '#f6f8fa',
  raised: '#ffffff',
  line: '#d3dce3',
  ink: '#15212c',
  muted: '#4f6070',
  panel: '#e6ecf1',
  series: '#2f73b0',
  'chart-1': '#2a78d6',
  'chart-2': '#eb6834',
  'chart-3': '#1baf7a',
  'chart-4': '#eda100',
  'chart-5': '#e87ba4',
  'chart-other': '#8a96a3',
  'chart-surface': '#ffffff',
};

const PADDING = 24;
const SVG_NS = 'http://www.w3.org/2000/svg';

export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The area every table covers, with a margin. */
export function diagramBounds(tables: DiagramTable[], positions: Positions): Bounds | null {
  let left = Infinity;
  let top = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const table of tables) {
    const at = positions[table.name];
    if (!at) continue;
    left = Math.min(left, at.x);
    top = Math.min(top, at.y);
    right = Math.max(right, at.x + BOX_WIDTH);
    bottom = Math.max(bottom, at.y + tableHeight(table));
  }
  if (!Number.isFinite(left)) return null;
  return {
    x: Math.floor(left - PADDING),
    y: Math.floor(top - PADDING),
    width: Math.ceil(right - left + PADDING * 2),
    height: Math.ceil(bottom - top + PADDING * 2),
  };
}

/** Replaces `var(--name)` with a fixed colour; unknown names become ink. */
function fixed(value: string): string {
  return value.replace(/var\(--([a-z0-9-]+)\)/g, (_, name: string) => EXPORT_COLORS[name] ?? EXPORT_COLORS.ink ?? '#000');
}

/**
 * A standalone SVG of the whole diagram, not just the part on screen: no pan or zoom, theme
 * colours written in, a white background, and nothing that refers outside the file.
 */
export function standaloneSvg(drawing: SVGGElement, bounds: Bounds): string {
  const content = drawing.cloneNode(true) as SVGGElement;
  content.removeAttribute('transform');
  content.setAttribute('font-family', 'Atkinson Hyperlegible, sans-serif');
  for (const element of [content, ...Array.from(content.querySelectorAll('*'))]) {
    element.removeAttribute('class');
    element.removeAttribute('style');
    for (const name of ['fill', 'stroke']) {
      const value = element.getAttribute(name);
      if (value) element.setAttribute(name, fixed(value));
    }
  }
  for (const title of Array.from(content.querySelectorAll('title'))) title.remove();

  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('xmlns', SVG_NS);
  svg.setAttribute('width', String(bounds.width));
  svg.setAttribute('height', String(bounds.height));
  svg.setAttribute('viewBox', `${bounds.x} ${bounds.y} ${bounds.width} ${bounds.height}`);
  const background = document.createElementNS(SVG_NS, 'rect');
  for (const [name, value] of Object.entries({
    x: bounds.x,
    y: bounds.y,
    width: bounds.width,
    height: bounds.height,
    fill: '#ffffff',
  })) {
    background.setAttribute(name, String(value));
  }
  svg.append(background, content);
  return new XMLSerializer().serializeToString(svg);
}
