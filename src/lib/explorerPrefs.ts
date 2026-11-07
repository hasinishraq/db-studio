/** Side panel preferences. They are conveniences, so losing them is harmless. */

const ORDER = 'studio.connectionOrder';
const COLLAPSED = 'studio.collapsedConnections';

function read(key: string): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? '[]');
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : [];
  } catch {
    return [];
  }
}

function write(key: string, value: string[]): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage can be unavailable; the panel still works for this session.
  }
}

export const loadOrder = () => read(ORDER);
export const saveOrder = (ids: string[]) => write(ORDER, ids);
export const loadCollapsed = () => new Set(read(COLLAPSED));
export const saveCollapsed = (ids: Set<string>) => write(COLLAPSED, [...ids]);

/** Items in the saved order. Anything new keeps its place after the ones already placed. */
export function inOrder<T extends { id: string }>(items: T[], order: string[]): T[] {
  const rank = new Map(order.map((id, index) => [id, index]));
  const placed = items.filter((item) => rank.has(item.id));
  placed.sort((a, b) => (rank.get(a.id) ?? 0) - (rank.get(b.id) ?? 0));
  return [...placed, ...items.filter((item) => !rank.has(item.id))];
}

/** Moves one id so it lands before position `to` of the list as it was, or at the end. */
export function moveTo(ids: string[], id: string, to: number): string[] {
  const from = ids.indexOf(id);
  if (from < 0) return ids;
  const rest = ids.filter((other) => other !== id);
  const target = Math.max(0, Math.min(rest.length, to > from ? to - 1 : to));
  return [...rest.slice(0, target), id, ...rest.slice(target)];
}
