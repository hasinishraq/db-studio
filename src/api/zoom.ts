import { useSyncExternalStore } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { nextZoom, ZOOM_STEPS, type ZoomAction } from '../lib/zoom';

const KEY = 'studio.zoom';

let current = 1;
const listeners = new Set<() => void>();

function remembered(): number {
  try {
    const value = Number(localStorage.getItem(KEY));
    return ZOOM_STEPS.includes(value) ? value : 1;
  } catch {
    return 1;
  }
}

function apply(level: number): void {
  current = level;
  void getCurrentWebview().setZoom(level);
  listeners.forEach((listener) => listener());
}

/** Scales the whole window, editor included, and remembers the level. */
export function changeZoom(action: ZoomAction): void {
  apply(nextZoom(current, action));
  try {
    localStorage.setItem(KEY, String(current));
  } catch {
    // Remembering the level is a convenience.
  }
}

export function restoreZoom(): void {
  const level = remembered();
  if (level !== 1) apply(level);
}

/** The current level, for the Settings control. */
export function useZoom(): number {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}
