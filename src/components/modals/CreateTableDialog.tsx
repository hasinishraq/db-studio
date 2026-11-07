import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { api, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { NewColumn, ProfileId } from '../../bindings';
import { newId, useStudio } from '../../context/Studio';
import { ResultStore } from '../../lib/resultStore';
import { Button, ErrorNote, Field, Input, Modal } from '../ui';

const blank = (): NewColumn => ({ name: '', dataType: 'TEXT', nullable: true, primaryKey: false, defaultValue: null });

/** The DDL is generated in Rust and reviewed like any other change. */
export function CreateTableDialog({
  profileId,
  schema,
  onClose,
}: {
  profileId: ProfileId;
  schema: string;
  onClose: () => void;
}) {
  const { targetFor, confirm } = useStudio();
  const client = useQueryClient();
  const [name, setName] = useState('');
  const [columns, setColumns] = useState<NewColumn[]>([
    { ...blank(), name: 'id', dataType: 'INTEGER', nullable: false, primaryKey: true },
  ]);
  const [error, setError] = useState<string | null>(null);
  const update = (index: number, change: Partial<NewColumn>) =>
    setColumns((all) => all.map((c, i) => (i === index ? { ...c, ...change } : c)));

  const create = async () => {
    const target = targetFor(profileId);
    if (!target) return;
    setError(null);
    try {
      const planned = await api.planCreateTable(target.session, { schema, name }, columns);
      const result = await runFlow({
        target,
        sql: planned.sql,
        origin: 'Editor',
        confirm,
        runId: newId(),
        store: new ResultStore(),
      });
      if (result.status === 'declined') return;
      const failed = result.outcomes.find((outcome) => outcome.kind === 'error');
      if (failed?.kind === 'error') {
        setError(failed.error.message);
        return;
      }
      await client.invalidateQueries({ queryKey: ['schema', profileId] });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <Modal title={`New table in ${schema}`} onClose={onClose} wide>
      <Field label="Table name">
        <Input value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <div className="mt-3 space-y-2">
        {columns.map((column, index) => (
          <div key={index} className="flex items-center gap-2 text-sm">
            <Input aria-label="Column name" placeholder="Column name" value={column.name} onChange={(e) => update(index, { name: e.target.value })} />
            <Input aria-label="Type" placeholder="Type" value={column.dataType} onChange={(e) => update(index, { dataType: e.target.value })} />
            <label className="flex items-center gap-1 whitespace-nowrap">
              <input
                type="checkbox"
                checked={column.primaryKey}
                onChange={(e) => update(index, { primaryKey: e.target.checked, nullable: !e.target.checked })}
              />
              Key
            </label>
            <label className="flex items-center gap-1 whitespace-nowrap">
              <input type="checkbox" checked={column.nullable} onChange={(e) => update(index, { nullable: e.target.checked })} />
              Can be empty
            </label>
            <Button onClick={() => setColumns((all) => all.filter((_, i) => i !== index))}>Remove</Button>
          </div>
        ))}
      </div>
      <div className="mt-3">
        <ErrorNote message={error} />
      </div>
      <div className="mt-4 flex justify-between">
        <Button onClick={() => setColumns((all) => [...all, blank()])}>Add a column</Button>
        <div className="flex gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button tone="primary" disabled={!name.trim()} onClick={() => void create()}>
            Review table
          </Button>
        </div>
      </div>
    </Modal>
  );
}
