import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Settings } from '../bindings';

const mocks = vi.hoisted(() => ({
  settingsGet: vi.fn(),
  settingsSet: vi.fn(),
  setSecret: vi.fn(),
  maskingList: vi.fn(),
  studio: { current: { profiles: [] } as Record<string, unknown> },
}));
vi.mock('@tauri-apps/api/core', () => ({ Channel: class {} }));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom: vi.fn() }) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({ commands: mocks, events: {} }));
vi.mock('../context/Studio', () => ({ newId: () => crypto.randomUUID(), useStudio: () => mocks.studio.current }));

import { SettingsTab } from '../components/workbench/SettingsTab';

const ok = <T,>(data: T) => Promise.resolve({ status: 'ok' as const, data });

const settings = {
  theme: 'Dark',
  aiProvider: 'Ollama',
  aiEndpoint: 'http://localhost:11434',
  aiModel: 'llama',
} as Settings;

const renderPage = async () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsTab />
    </QueryClientProvider>,
  );
  await screen.findByRole('heading', { name: 'General' });
};

describe('settings page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.settingsGet.mockReturnValue(ok(settings));
    mocks.settingsSet.mockReturnValue(ok(null));
    mocks.setSecret.mockReturnValue(ok(null));
  });

  it('shows one section at a time, chosen from the side list', async () => {
    await renderPage();
    expect(screen.queryByRole('heading', { name: 'Editor' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Editor' }));
    expect(screen.getByRole('heading', { name: 'Editor' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'General' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Editor' })).toHaveAttribute('aria-current', 'page');
  });

  it('finds a setting in any section from the search box', async () => {
    await renderPage();
    fireEvent.change(screen.getByLabelText('Search settings'), { target: { value: 'line numbers' } });
    expect(screen.getByRole('switch', { name: 'Show line numbers' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Runs kept in history')).toBeNull();
  });

  it('says so when nothing matches the search', async () => {
    await renderPage();
    fireEvent.change(screen.getByLabelText('Search settings'), { target: { value: 'zzzz' } });
    expect(await screen.findByText('No settings match "zzzz".')).toBeInTheDocument();
  });

  it('names each switch by its label and saves a change through Rust', async () => {
    await renderPage();
    vi.useFakeTimers();
    try {
      const toggle = screen.getByRole('switch', { name: 'Hide masked columns in results too' });
      expect(toggle).toHaveAttribute('aria-checked', 'false');
      fireEvent.click(toggle);
      expect(toggle).toHaveAttribute('aria-checked', 'true');
      await act(async () => {
        vi.advanceTimersByTime(500);
      });
      expect(mocks.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ maskInGrid: true }));
    } finally {
      vi.useRealTimers();
    }
  });

  it('picks the theme from pictures that work as radio buttons', async () => {
    await renderPage();
    const light = screen.getByRole('radio', { name: 'Light' }) as HTMLInputElement;
    expect((screen.getByRole('radio', { name: 'Dark' }) as HTMLInputElement).checked).toBe(true);
    fireEvent.click(light);
    expect(light.checked).toBe(true);
  });

  it('never shows a saved API key, and clears the field after saving one', async () => {
    await renderPage();
    fireEvent.click(screen.getByRole('button', { name: 'Assistant' }));
    const field = screen.getByLabelText('API key') as HTMLInputElement;
    expect(field.type).toBe('password');
    expect(field.value).toBe('');
    fireEvent.change(field, { target: { value: 'sk-test' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save key' }));
    await waitFor(() => expect(mocks.setSecret).toHaveBeenCalled());
    await waitFor(() => expect(field.value).toBe(''));
  });
});
