import type { SchemaOutline, TableStructure } from '../bindings';

export interface DiagramColumn {
  name: string;
  type: string;
  primaryKey: boolean;
}

export interface DiagramKey {
  columns: string[];
  table: string;
  referencedColumns: string[];
}

export interface DiagramTable {
  name: string;
  columns: DiagramColumn[];
  keys: DiagramKey[];
}

export interface Point {
  x: number;
  y: number;
}

export interface View extends Point {
  k: number;
}

export type Positions = Record<string, Point>;

export const BOX_WIDTH = 280;
export const HEADER = 28;
export const ROW = 20;
const GAP = 70;
const MIN_ZOOM = 0.05;
const MAX_ZOOM = 2;

export function fromStructures(structures: TableStructure[]): DiagramTable[] {
  return structures.map((structure) => ({
    name: structure.table.name,
    columns: structure.columns.map((c) => ({ name: c.name, type: c.dataType, primaryKey: c.primaryKey })),
    keys: structure.foreignKeys.map((k) => ({
      columns: k.columns,
      table: k.referencedTable,
      referencedColumns: k.referencedColumns,
    })),
  }));
}

export function fromOutline(outline: SchemaOutline): DiagramTable[] {
  return outline.tables.map((table) => ({
    name: table.name,
    columns: table.columns.map((c) => ({ name: c.name, type: c.dataType, primaryKey: c.primaryKey })),
    keys: table.foreignKeys.map((k) => ({
      columns: k.columns,
      table: k.referencedTable,
      referencedColumns: k.referencedColumns,
    })),
  }));
}

export function tableHeight(table: DiagramTable): number {
  return HEADER + Math.max(table.columns.length, 1) * ROW + 6;
}

/** Most-connected tables first, each dropped into the shortest column of a roughly square layout. */
export function arrange(tables: DiagramTable[]): Positions {
  const links = new Map<string, number>();
  const count = (name: string) => links.set(name, (links.get(name) ?? 0) + 1);
  for (const table of tables) {
    for (const key of table.keys) {
      count(table.name);
      count(key.table);
    }
  }
  const ordered = [...tables].sort((a, b) => (links.get(b.name) ?? 0) - (links.get(a.name) ?? 0));
  const heights = new Array<number>(Math.max(1, Math.ceil(Math.sqrt(tables.length)))).fill(0);
  const positions: Positions = {};
  for (const table of ordered) {
    const shortest = heights.indexOf(Math.min(...heights));
    positions[table.name] = { x: shortest * (BOX_WIDTH + GAP), y: heights[shortest] ?? 0 };
    heights[shortest] = (heights[shortest] ?? 0) + tableHeight(table) + GAP;
  }
  return positions;
}

function rowY(table: DiagramTable, top: number, column: string | undefined): number {
  const index = table.columns.findIndex((c) => c.name === column);
  return index < 0 ? top + HEADER / 2 : top + HEADER + index * ROW + ROW / 2;
}

export interface Edge {
  id: string;
  path: string;
}

/** One curve per foreign key, from the column to the column it references. */
export function edges(tables: DiagramTable[], positions: Positions): Edge[] {
  const byName = new Map(tables.map((table) => [table.name, table]));
  const found: Edge[] = [];
  for (const table of tables) {
    const from = positions[table.name];
    if (!from) continue;
    table.keys.forEach((key, index) => {
      const target = byName.get(key.table);
      const to = positions[key.table];
      if (!target || !to) return;
      const startY = rowY(table, from.y, key.columns[0]);
      const endY = rowY(target, to.y, key.referencedColumns[0]);
      const toRight = to.x >= from.x + BOX_WIDTH;
      const toLeft = to.x + BOX_WIDTH <= from.x;
      // Boxes in one column connect around their right side.
      const startX = toLeft ? from.x : from.x + BOX_WIDTH;
      const endX = toRight ? to.x : to.x + BOX_WIDTH;
      const reach = Math.max(40, Math.abs(endX - startX) / 2);
      const startBend = startX + (toLeft ? -reach : reach);
      const endBend = endX + (toRight ? -reach : reach);
      found.push({
        id: `${table.name}:${index}`,
        path: `M${startX},${startY} C${startBend},${startY} ${endBend},${endY} ${endX},${endY}`,
      });
    });
  }
  return found;
}

function clampZoom(k: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));
}

/** Zooms while keeping the point under the cursor still. */
export function zoomAt(view: View, factor: number, at: Point): View {
  const k = clampZoom(view.k * factor);
  const scale = k / view.k;
  return { k, x: at.x - (at.x - view.x) * scale, y: at.y - (at.y - view.y) * scale };
}

/** The view that shows every table. It never enlarges past actual size. */
export function fit(tables: DiagramTable[], positions: Positions, width: number, height: number): View {
  const margin = 24;
  let right = 0;
  let bottom = 0;
  for (const table of tables) {
    const at = positions[table.name];
    if (!at) continue;
    right = Math.max(right, at.x + BOX_WIDTH);
    bottom = Math.max(bottom, at.y + tableHeight(table));
  }
  if (right === 0 || width <= 0 || height <= 0) return { x: margin, y: margin, k: 1 };
  const k = clampZoom(Math.min(1, (width - margin * 2) / right, (height - margin * 2) / bottom));
  return { x: margin, y: margin, k };
}
