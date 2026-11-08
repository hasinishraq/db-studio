import { useQueryClient } from '@tanstack/react-query';
import { api, ApiError } from '../api';
import type { ConnectionProfile } from '../bindings';
import { useStudio } from '../context/Studio';

/** Says, in a sentence, where a connection's queries and the assistant work. */
export function whereLabel(profile: ConnectionProfile, database: string | null, schema: string | null): string {
  if (profile.engine === 'Sqlite') return `${profile.name} › ${profile.database.split(/[\\/]/).pop() ?? profile.database}`;
  if (!database) return `${profile.name} (no database in use)`;
  return [profile.name, database, profile.engine === 'Postgres' ? schema : null].filter(Boolean).join(' › ');
}

function friendly(error: unknown, what: string): Error {
  if (error instanceof ApiError && error.code === 'DirtyTransaction') {
    return new Error(`Commit or roll back first. Then change ${what}.`);
  }
  return error instanceof Error ? error : new Error(String(error));
}

/**
 * Chooses the database and schema a connection works in. The side panel and the assistant share
 * this, so SQL tabs, table tabs and the assistant always agree on where they are.
 */
export function useNamespace(profile: ConnectionProfile | null) {
  const { sessions, setSession, tabs, closeTab } = useStudio();
  const client = useQueryClient();
  const session = profile ? sessions[profile.id] : undefined;

  const chooseDatabase = async (database: string) => {
    if (!profile || !session || database === session.database) return;
    try {
      const info = await api.switchDatabase(session.sessionId, database);
      // A PostgreSQL session is tied to its database, so tabs naming its objects close.
      if (profile.engine === 'Postgres') {
        tabs
          .filter((tab) => (tab.kind === 'table' || tab.kind === 'er') && tab.profileId === profile.id)
          .forEach((tab) => closeTab(tab.id, true));
      }
      setSession(profile.id, info);
    } catch (e) {
      throw friendly(e, 'database');
    }
  };

  const chooseSchema = async (schema: string) => {
    if (!profile || !session || schema === session.schema) return;
    try {
      setSession(profile.id, await api.switchSchema(session.sessionId, schema));
    } catch (e) {
      throw friendly(e, 'schema');
    }
  };

  /** Saves where the connection opens next time; `null` for both forgets it. */
  const remember = async (database: string | null, schema: string | null) => {
    if (!profile) return;
    await api.rememberDefault(profile.id, database, schema);
    await client.invalidateQueries({ queryKey: ['profiles'] });
  };

  return {
    session,
    database: session?.database ?? null,
    schema: session?.schema ?? null,
    chooseDatabase,
    chooseSchema,
    remember,
  };
}
