import type { Theme } from '../bindings';

const KEY = 'studio.theme';

function resolved(theme: Theme): 'light' | 'dark' {
  if (theme !== 'System') return theme === 'Light' ? 'light' : 'dark';
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

/** Applies a theme and remembers it, so the next start paints correctly before any IPC. */
export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = resolved(theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Remembering the theme is a convenience.
  }
}

export function applyCachedTheme(): void {
  let theme: Theme = 'System';
  try {
    theme = (localStorage.getItem(KEY) as Theme | null) ?? 'System';
  } catch {
    // Fall through to the system theme.
  }
  applyTheme(theme);
}
