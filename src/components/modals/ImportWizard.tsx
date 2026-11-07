import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { api, ApiError, errorMessage } from '../../api';
import type { ImportMapping, ImportPreview, ProfileId, TableRef } from '../../bindings';
import { newId, useStudio } from '../../context/Studio';
import { Button, ErrorNote, Field, Input, Modal, Select } from '../ui';

/** Choose a file, map columns, preview, and review once for the whole import. */
export function ImportWizard({
  columns,
  table,
  profileId,
  onClose,
}: {
  columns: string[];
  table: TableRef;
  profileId: ProfileId;
  onClose: () => void;
}) {
  const { targetFor, confirm } = useStudio();
  const client = useQueryClient();
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [mapping, setMapping] = useState<Record<string, number | null>>({});
  const [batch, setBatch] = useState(1000);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const pick = async () => {
    setError(null);
    try {
      const chosen = await api.importPickFile();
      setPreview(chosen);
      const guess: Record<string, number | null> = {};
      columns.forEach((column) => {
        const found = chosen.headers.findIndex((h) => h.toLowerCase() === column.toLowerCase());
        guess[column] = found >= 0 ? found : null;
      });
      setMapping(guess);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  const start = async () => {
    const target = targetFor(profileId);
    if (!preview || !target) return;
    const mappings: ImportMapping[] = Object.entries(mapping).flatMap(([column, csvColumn]) =>
      csvColumn === null || csvColumn === undefined ? [] : [{ csvColumn, column }],
    );
    setBusy(true);
    setError(null);
    try {
      const plan = await api.importPlan(target.session, preview.fileId, table, mappings);
      const verdicts = await api.prepareQuery(target.session, plan.reviewSql);
      const confirmed = await confirm({
        sql: plan.reviewSql,
        verdicts,
        connectionName: target.connectionName,
        environment: target.environment,
        warnings: [`This imports ${plan.rows.toLocaleString()} rows. One bad row undoes the whole import.`],
      });
      if (!confirmed) return;
      const grant = await api.approveQuery(target.session, plan.reviewSql, 'Import');
      const rows = await api.importCsv(target.session, preview.fileId, table, mappings, batch, grant.token, newId());
      setDone(rows);
      void client.invalidateQueries({ queryKey: ['schema', profileId] });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title={`Import into ${table.name}`} onClose={onClose} wide>
      <ErrorNote message={error} />
      {done !== null ? (
        <>
          <p className="mb-3">Imported {done.toLocaleString()} rows.</p>
          <Button tone="primary" onClick={onClose}>
            Close
          </Button>
        </>
      ) : !preview ? (
        <Button tone="primary" onClick={() => void pick()}>
          Choose a CSV file
        </Button>
      ) : (
        <>
          <p className="mb-2 text-sm text-muted">{preview.fileName}</p>
          <div className="mb-3 grid grid-cols-2 gap-2">
            {columns.map((column) => (
              <Field key={column} label={column}>
                <Select
                  value={mapping[column] ?? ''}
                  onChange={(e) =>
                    setMapping((m) => ({ ...m, [column]: e.target.value === '' ? null : Number(e.target.value) }))
                  }
                >
                  <option value="">Skip this column</option>
                  {preview.headers.map((header, index) => (
                    <option key={index} value={index}>
                      {header}
                    </option>
                  ))}
                </Select>
              </Field>
            ))}
          </div>
          <Field label="Rows per batch">
            <Input type="number" min={1} max={10000} value={batch} onChange={(e) => setBatch(Number(e.target.value))} />
          </Field>
          <div className="mt-3 max-h-48 overflow-auto rounded bg-ground">
            <table className="font-mono text-xs">
              <thead>
                <tr>
                  {preview.headers.map((header, index) => (
                    <th key={index} className="px-2 text-left">
                      {header}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {preview.rows.map((row, index) => (
                  <tr key={index}>
                    {row.map((cell, column) => (
                      <td key={column} className="px-2">
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="mt-4 flex justify-end gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button tone="primary" disabled={busy} onClick={() => void start()}>
              Review import
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
}
