import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ Channel: class {}, invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('../bindings', () => ({ commands: {}, events: {} }));
vi.mock('../context/Studio', () => ({ newId: () => crypto.randomUUID(), useStudio: () => ({}) }));

import { Palette, type PaletteAction } from '../components/shell/Palette';
import { Button, Kbd, Modal } from '../components/ui';
import { Count } from '../components/ui/Count';
import { notify, Toaster } from '../components/ui/toast';

describe('toasts', () => {
  it('shows a lost connection as an error that stays until dismissed', async () => {
    render(<Toaster />);
    act(() => {
      notify.error('A connection was lost. Connect again to continue.');
    });
    expect(await screen.findByText('A connection was lost. Connect again to continue.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /close/i })).toBeInTheDocument();
  });

  it('shows a keychain warning', async () => {
    render(<Toaster />);
    act(() => {
      notify.warning('The system keychain is locked.');
    });
    expect(await screen.findByText('The system keychain is locked.')).toBeInTheDocument();
  });
});

describe('command palette', () => {
  const actions = (bigger: () => void): PaletteAction[] => [
    { group: 'Actions', label: 'New query', run: vi.fn(), shortcut: ['Ctrl', 'T'] },
    { group: 'Open tabs', label: 'orders', run: vi.fn() },
    { group: 'View', label: 'Make everything bigger', run: bigger },
  ];

  it('puts commands under their group headings with shortcut keys', () => {
    render(<Palette onClose={vi.fn()} actions={actions(vi.fn())} />);
    expect(screen.getByText('Actions')).toBeInTheDocument();
    expect(screen.getByText('Open tabs')).toBeInTheDocument();
    expect(screen.getByText('View')).toBeInTheDocument();
    expect(screen.getByText('Ctrl')).toBeInTheDocument();
  });

  it('filters as you type, runs the picked command and closes', () => {
    const bigger = vi.fn();
    const close = vi.fn();
    render(<Palette onClose={close} actions={actions(bigger)} />);
    const input = screen.getByPlaceholderText('Type a command or search');
    fireEvent.change(input, { target: { value: 'bigger' } });
    expect(screen.queryByText('New query')).toBeNull();
    fireEvent.click(screen.getByText('Make everything bigger'));
    expect(bigger).toHaveBeenCalledOnce();
    expect(close).toHaveBeenCalledOnce();
  });

  it('says so when nothing matches', () => {
    render(<Palette onClose={vi.fn()} actions={actions(vi.fn())} />);
    fireEvent.change(screen.getByPlaceholderText('Type a command or search'), { target: { value: 'zzzz' } });
    expect(screen.getByText('No matching command.')).toBeInTheDocument();
  });

  it('closes on Escape', () => {
    const close = vi.fn();
    render(<Palette onClose={close} actions={actions(vi.fn())} />);
    fireEvent.keyDown(screen.getByPlaceholderText('Type a command or search'), { key: 'Escape' });
    expect(close).toHaveBeenCalled();
  });
});

describe('primitives', () => {
  it('gives a modal a title and a close button', () => {
    const close = vi.fn();
    render(
      <Modal title="New table" onClose={close}>
        <p>body</p>
      </Modal>,
    );
    expect(screen.getByRole('heading', { name: 'New table' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(close).toHaveBeenCalled();
  });

  it('keeps a button a plain button unless told otherwise', () => {
    render(<Button>Run</Button>);
    expect(screen.getByRole('button', { name: 'Run' })).toHaveAttribute('type', 'button');
  });

  it('draws a shortcut as a key cap', () => {
    render(<Kbd>Ctrl K</Kbd>);
    expect(screen.getByText('Ctrl K').tagName).toBe('KBD');
  });

  it('renders a count without crashing outside a real browser', () => {
    const { container } = render(<Count value={1234} />);
    expect(container.firstChild).not.toBeNull();
  });
});
