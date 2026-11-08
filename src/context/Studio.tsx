import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../api';
import type { Confirm, ConfirmRequest, RunTarget } from '../api/run';
import type { ConnectionProfile, ProfileId, SessionInfo, TableRef } from '../bindings';
import { ConfirmDialog } from '../components/modals/ConfirmDialog';
import { Button, Modal } from '../components/ui';

export type Tab =
  | { id: string; kind: 'sql'; title: string; profileId: ProfileId | null; draftId: string | null; sql: string }
  | { id: string; kind: 'table'; title: string; profileId: ProfileId; table: TableRef }
  | { id: string; kind: 'er'; title: string; profileId: ProfileId; schema: string }
  | { id: string; kind: 'ai'; title: string }
  | { id: string; kind: 'audit'; title: string }
  | { id: string; kind: 'settings'; title: string };

interface StudioValue {
  profiles: ConnectionProfile[];
  sessions: Record<string, SessionInfo>;
  setSession: (profile: ProfileId, session: SessionInfo | null) => void;
  tabs: Tab[];
  activeId: string | null;
  openTab: (tab: Tab) => void;
  /** Closes a tab. One with unsaved changes asks first, unless `force` is set. */
  closeTab: (id: string, force?: boolean) => void;
  /** A tab reports whether it holds changes that would be lost on close. */
  setUnsaved: (id: string, unsaved: boolean) => void;
  activate: (id: string) => void;
  updateTab: (id: string, change: Partial<Tab>) => void;
  activeProfile: ConnectionProfile | null;
  confirm: Confirm;
  targetFor: (profile: ProfileId | null) => RunTarget | null;
}

const Context = createContext<StudioValue | null>(null);

export function useStudio(): StudioValue {
  const value = useContext(Context);
  if (!value) throw new Error('useStudio needs a StudioProvider');
  return value;
}

/** A tab id unique enough for this window. */
export const newId = (): string => crypto.randomUUID();

export function StudioProvider({ children }: { children: ReactNode }) {
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: api.getProfiles });
  const client = useQueryClient();
  const [sessions, setSessions] = useState<Record<string, SessionInfo>>({});
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [pending, setPending] = useState<ConfirmRequest | null>(null);
  const resolver = useRef<((confirmed: boolean) => void) | null>(null);

  const setSession = useCallback(
    (profile: ProfileId, session: SessionInfo | null) => {
      setSessions((current) => {
        const next = { ...current };
        if (session) next[profile] = session;
        else delete next[profile];
        return next;
      });
      void client.invalidateQueries({ queryKey: ['schema', profile] });
    },
    [client],
  );

  const openTab = useCallback((tab: Tab) => {
    setTabs((current) => (current.some((t) => t.id === tab.id) ? current : [...current, tab]));
    setActiveId(tab.id);
  }, []);

  const unsaved = useRef<Set<string>>(new Set());
  const [closing, setClosing] = useState<string | null>(null);

  const setUnsaved = useCallback((id: string, value: boolean) => {
    if (value) unsaved.current.add(id);
    else unsaved.current.delete(id);
  }, []);

  const closeTab = useCallback((id: string, force = false) => {
    if (!force && unsaved.current.has(id)) {
      setClosing(id);
      return;
    }
    unsaved.current.delete(id);
    setTabs((current) => {
      const index = current.findIndex((t) => t.id === id);
      const next = current.filter((t) => t.id !== id);
      setActiveId((active) => (active === id ? (next[Math.max(0, index - 1)]?.id ?? null) : active));
      return next;
    });
  }, []);

  const updateTab = useCallback((id: string, change: Partial<Tab>) => {
    setTabs((current) => current.map((t) => (t.id === id ? ({ ...t, ...change } as Tab) : t)));
  }, []);

  const confirm = useCallback<Confirm>(
    (request) =>
      new Promise((resolve) => {
        resolver.current = resolve;
        setPending(request);
      }),
    [],
  );

  const settle = useCallback((confirmed: boolean) => {
    resolver.current?.(confirmed);
    resolver.current = null;
    setPending(null);
  }, []);

  const targetFor = useCallback(
    (id: ProfileId | null): RunTarget | null => {
      const profile = profiles.find((p) => p.id === id);
      const session = id ? sessions[id] : undefined;
      if (!profile || !session) return null;
      return { session: session.sessionId, connectionName: profile.name, environment: profile.environment };
    },
    [profiles, sessions],
  );

  const activeProfile = useMemo(() => {
    const tab = tabs.find((t) => t.id === activeId);
    const id = tab && 'profileId' in tab ? tab.profileId : null;
    return profiles.find((p) => p.id === id) ?? null;
  }, [tabs, activeId, profiles]);

  const value: StudioValue = {
    profiles,
    sessions,
    setSession,
    tabs,
    activeId,
    openTab,
    closeTab,
    setUnsaved,
    activate: setActiveId,
    updateTab,
    activeProfile,
    confirm,
    targetFor,
  };

  return (
    <Context.Provider value={value}>
      {children}
      {pending && <ConfirmDialog request={pending} onSettle={settle} />}
      {closing && (
        <Modal title="Discard unsaved changes?" onClose={() => setClosing(null)}>
          <p className="text-sm">
            {tabs.find((t) => t.id === closing)?.title ?? 'This tab'} has edits that are not saved. Closing it
            throws them away.
          </p>
          <div className="mt-4 flex justify-end gap-2">
            <Button onClick={() => setClosing(null)}>Keep editing</Button>
            <Button
              tone="danger"
              onClick={() => {
                const id = closing;
                setClosing(null);
                closeTab(id, true);
              }}
            >
              Discard and close
            </Button>
          </div>
        </Modal>
      )}
    </Context.Provider>
  );
}
