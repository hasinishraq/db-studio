import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';
import { clsx } from 'clsx';

export interface MenuItem {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** Draws a line above the item. */
  separated?: boolean;
}

/** A right-click menu. Arrow keys move, Enter picks, Escape or a click elsewhere closes. */
export function ContextMenu({ at, items, label, onClose }: { at: { x: number; y: number }; items: MenuItem[]; label: string; onClose: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    box.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }, []);
  const move = (event: KeyboardEvent) => {
    const buttons = [...(box.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const step = event.key === 'ArrowDown' ? 1 : -1;
      buttons[(index + step + buttons.length) % buttons.length]?.focus();
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      onClose();
    }
  };
  // Keep the menu on screen.
  const left = Math.min(at.x, window.innerWidth - 240);
  const top = Math.min(at.y, window.innerHeight - items.length * 32 - 16);
  return (
    <div
      className="fixed inset-0 z-50"
      onClick={onClose}
      onContextMenu={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <div
        ref={box}
        role="menu"
        aria-label={label}
        className="fixed min-w-[13rem] origin-top-left animate-pop-in select-none rounded-md border border-line bg-raised p-1 text-sm shadow-pop"
        style={{ left, top }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={move}
      >
        {items.map((item) => (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            className={clsx(
              'flex h-7 w-full items-center rounded px-2 text-left hover:bg-accent hover:text-on-accent focus:bg-accent focus:text-on-accent focus:outline-none disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-ink',
              item.separated && 'relative mt-[9px] before:absolute before:-top-[5px] before:left-1 before:right-1 before:h-px before:bg-line',
            )}
            onClick={() => {
              onClose();
              item.onSelect();
            }}
          >
            {item.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Wires a row to a context menu: right-click, the Menu key, or Shift+F10. */
export function useContextMenu(label: string, items: () => MenuItem[]): { triggers: { onContextMenu: (e: MouseEvent) => void; onKeyDown: (e: KeyboardEvent<HTMLElement>) => void }; menu: ReactNode } {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  return {
    triggers: {
      onContextMenu: (e) => {
        e.preventDefault();
        setAt({ x: e.clientX, y: e.clientY });
      },
      onKeyDown: (e) => {
        if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
          e.preventDefault();
          const rect = e.currentTarget.getBoundingClientRect();
          setAt({ x: rect.left + 16, y: rect.bottom });
        }
      },
    },
    menu: at ? <ContextMenu at={at} items={items()} label={label} onClose={() => setAt(null)} /> : null,
  };
}
