import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { api, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { Engine, ProfileId } from '../../bindings';
import { newId, useStudio } from '../../context/Studio';
import { ResultStore } from '../../lib/resultStore';
import { Button, ErrorNote, Field, Input, Modal, Select } from '../ui';

const CHARSETS = ['utf8mb4', 'utf8mb3', 'latin1', 'ascii'];

/** The statement is written in Rust and reviewed like any other change. */
export function CreateDatabaseDialog({
  profileId,
  engine,
  onCreated,
  onClose,
}: {
  profileId: ProfileId;
  engine: Engine;
  onCreated: (name: string, useNow: boolean) => Promise<void>;
  onClose: () => void;
}) {
  const { targetFor, confirm } = useStudio();
  const client = useQueryClient();
  const [name, setName] = useState('');
  const [charset, setCharset] = useState('utf8mb4');
  const [useNow, setUseNow] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    const target = targetFor(profileId);
    if (!target) return;
    setError(null);
    setBusy(true);
    try {
      const planned = await api.planCreateDatabase(target.session, name.trim(), engine === 'MySql' ? charset : null);
      const result = await runFlow({
        target,
        sql: planned.sql,
        origin: 'Editor',
        confirm,
        runId: newId(),
        store: new ResultStore(),
        warnings: planned.warnings,
      });
      if (result.status === 'declined') return;
      const failed = result.outcomes.find((outcome) => outcome.kind === 'error');
      if (failed?.kind === 'error') {
        setError(failed.error.message);
        return;
      }
      await client.invalidateQueries({ queryKey: ['schema', profileId] });
      await onCreated(name.trim(), useNow);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="New database" onClose={onClose}>
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <Field label="Name">
          <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
        {engine === 'MySql' && (
          <Field label="Character set">
            <Select value={charset} onChange={(e) => setCharset(e.target.value)}>
              {CHARSETS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </Select>
          </Field>
        )}
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={useNow} onChange={(e) => setUseNow(e.target.checked)} />
          Use it now
        </label>
        <ErrorNote message={error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" tone="primary" disabled={busy || !name.trim()}>
            Create
          </Button>
        </div>
      </form>
    </Modal>
  );
}
