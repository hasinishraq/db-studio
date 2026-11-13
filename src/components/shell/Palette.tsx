import { Command } from 'cmdk';
import { Search, type LucideIcon } from 'lucide-react';
import { Kbd } from '../ui';

/** One command in the palette. `group` decides the heading it sits under. */
export interface PaletteAction {
  label: string;
  group: 'Actions' | 'Open tabs' | 'Connections' | 'View';
  run: () => void;
  icon?: LucideIcon;
  shortcut?: string[];
}

export const GROUPS: PaletteAction['group'][] = ['Actions', 'Connections', 'Open tabs', 'View'];

export function Palette({ onClose, actions }: { onClose: () => void; actions: PaletteAction[] }) {
  return (
    <div className="fixed inset-0 z-50 animate-fade-in bg-black/40" onClick={onClose}>
      <Command
        label="Commands"
        loop
        className="mx-auto mt-[12vh] w-[92vw] max-w-xl origin-top animate-pop-in overflow-hidden rounded-lg border border-line bg-raised shadow-pop"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === 'Escape' && onClose()}
      >
        <div className="flex items-center gap-2 border-b border-line px-3">
          <Search size={15} aria-hidden className="shrink-0 text-muted" />
          <Command.Input
            autoFocus
            placeholder="Type a command or search"
            className="h-11 w-full bg-transparent text-sm outline-none placeholder:text-muted"
          />
          <Kbd>Esc</Kbd>
        </div>
        <Command.List className="max-h-[min(60vh,420px)] overflow-y-auto p-1.5">
          <Command.Empty className="px-3 py-6 text-center text-sm text-muted">No matching command.</Command.Empty>
          {GROUPS.map((group) => {
            const items = actions.filter((action) => action.group === group);
            if (items.length === 0) return null;
            return (
              <Command.Group
                key={group}
                heading={group}
                className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted"
              >
                {items.map((action) => {
                  const Icon = action.icon;
                  return (
                    <Command.Item
                      key={`${group}:${action.label}`}
                      value={`${group} ${action.label}`}
                      onSelect={() => {
                        action.run();
                        onClose();
                      }}
                      className="flex h-8 cursor-pointer items-center gap-2.5 rounded px-2 text-sm text-ink data-[selected=true]:bg-pick"
                    >
                      {Icon ? (
                        <Icon size={15} aria-hidden className="shrink-0 text-muted" />
                      ) : (
                        <span aria-hidden className="w-[15px]" />
                      )}
                      <span className="flex-1 truncate">{action.label}</span>
                      {action.shortcut && (
                        <span className="flex gap-1">
                          {action.shortcut.map((key) => (
                            <Kbd key={key}>{key}</Kbd>
                          ))}
                        </span>
                      )}
                    </Command.Item>
                  );
                })}
              </Command.Group>
            );
          })}
        </Command.List>
      </Command>
    </div>
  );
}
