import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, ApiError, errorMessage } from '../../api';
import type { AuditQuery, ExportFormat, Origin } from '../../bindings';
import { useStudio } from '../../context/Studio';
import { Button, ErrorNote, Select } from '../ui';

export function AuditTab() {
  const { profiles } = useStudio();
  const [query, setQuery] = useState<AuditQuery>({ profileId: null, origin: null, kind: null, from: null, to: null, limit: 500 });
  const [integrity, setIntegrity] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { data: records = [] } = useQuery({ queryKey: ['audit', query], queryFn: () => api.auditList(query) });

  const verify = async () => {
    const broken = await api.auditVerify();
    setIntegrity(broken === null ? 'The log is intact.' : `The log was altered at entry ${broken}.`);
  };

  const exportLog = async (format: ExportFormat) => {
    try {
      await api.auditExport(query, format);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-2 py-1">
        <Select
          aria-label="Connection"
          className="w-44"
          value={query.profileId ?? ''}
          onChange={(e) => setQuery({ ...query, profileId: e.target.value || null })}
        >
          <option value="">All connections</option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </Select>
        <Select
          aria-label="Source"
          className="w-32"
          value={query.origin ?? ''}
          onChange={(e) => setQuery({ ...query, origin: (e.target.value || null) as Origin | null })}
        >
          <option value="">All sources</option>
          <option value="Editor">Editor</option>
          <option value="Grid">Grid edits</option>
          <option value="Import">Imports</option>
          <option value="Ai">Assistant</option>
        </Select>
        <Button onClick={() => void verify()}>Verify integrity</Button>
        <Select
          aria-label="Export log"
          className="w-28"
          value=""
          onChange={(e) => void exportLog(e.target.value as ExportFormat)}
        >
          <option value="">Export</option>
          <option value="Csv">CSV</option>
          <option value="JsonLines">JSON lines</option>
        </Select>
        {integrity && <span className="text-sm">{integrity}</span>}
      </div>
      <ErrorNote message={error} />
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {records.length === 0 && <p className="text-sm text-muted">Every change made through the app is recorded here.</p>}
        <table className="w-full text-left text-sm">
          <tbody>
            {records.map((r) => (
              <tr key={r.id} className="border-b border-line align-top">
                <td className="whitespace-nowrap pr-3 text-muted">{new Date(r.entry.at).toLocaleString()}</td>
                <td className="pr-3">{r.entry.origin}</td>
                <td className="pr-3">{r.entry.outcome}</td>
                <td className="pr-3">{r.entry.rowsAffected ?? ''}</td>
                <td className="font-mono text-xs">{r.entry.sqlText}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
