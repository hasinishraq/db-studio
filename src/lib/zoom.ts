/** Zoom levels, the same kind of steps a browser uses. */
export const ZOOM_STEPS = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

export type ZoomAction = 'in' | 'out' | 'reset';

/** The next level. A stored value between steps moves to the nearest step in that direction. */
export function nextZoom(current: number, action: ZoomAction): number {
  if (action === 'reset') return 1;
  if (action === 'in') return ZOOM_STEPS.find((step) => step > current + 0.001) ?? current;
  return [...ZOOM_STEPS].reverse().find((step) => step < current - 0.001) ?? current;
}

/** Which zoom shortcut a key press is, if any. Plus and underscore are the shifted forms of = and -. */
export function zoomAction(event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'altKey'>): ZoomAction | null {
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return null;
  if (event.key === '=' || event.key === '+' || event.code === 'NumpadAdd') return 'in';
  if (event.key === '-' || event.key === '_' || event.code === 'NumpadSubtract') return 'out';
  if (event.key === '0' || event.code === 'Numpad0') return 'reset';
  return null;
}
