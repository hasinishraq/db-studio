import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { CellValue, ColumnMeta, FilterOperator, FilterRule, RowChange, TableRef, TableStructure, Value } from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import { cellText, ResultStore } from '../../lib/resultStore';
import { ImportWizard } from '../modals/ImportWizard';
import { Button, ErrorNote, Field, Input, Modal, Select } from '../ui';

type TableTabData = Extract<Tab, { kind: 'table' }>;

const PAGE_SIZE = 100;

const OPERATORS: { value: FilterOperator; label: string; unary?: boolean }[] = [
  { value: 'Eq', label: 'equals' },
  { value: 'NotEq', label: 'does not equal' },
  { value: 'Contains', label: 'contains' },
  { value: 'StartsWith', label: 'starts with' },
  { value: 'EndsWith', label: 'ends with' },
  { value: 'Gt', label: 'is greater than' },
  { value: 'Gte', label: 'is at least' },
  { value: 'Lt', label: 'is less than' },
  { value: 'Lte', label: 'is at most' },
  { value: 'IsNull', label: 'is empty', unary: true },
  { value: 'IsNotNull', label: 'is not empty', unary: true },
];

function rawValue(value: Value | undefined): string | null {
  return !value || value.t === 'Null' ? null : cellText(value);
}

function rowKey(columns: ColumnMeta[], row: Value[]): CellValue[] {
  return columns.map((column, index) => ({ column: column.name, value: rawValue(row[index]) }));
}

function RowEditor({
  columns,
  row,
  onSave,
  onClose,
}: {
  columns: ColumnMeta[];
  row: Value[] | null;
  onSave: (values: CellValue[]) => void;
  onClose: () => void;
}) {
  const [values, setValues] = useState<(string | null)[]>(columns.map((_, i) => (row ? rawValue(row[i]) : null)));
  const set = (index: number, value: string | null) =>
    setValues((list) => list.map((v, i) => (i === index ? value : v)));
  return (
    <Modal title={row ? 'Edit row' : 'Insert row'} onClose={onClose}>
      <div className="space-y-2">
        {columns.map((column, index) => (
          <Field key={column.name} label={`${column.name} (${column.typeName})`}>
            <div className="flex gap-2">
              <Input
                aria-label={column.name}
                value={values[index] ?? ''}
                placeholder="NULL"
                onChange={(e) => set(index, e.target.value)}
              />
              <Button onClick={() => set(index, null)}>NULL</Button>
            </div>
          </Field>
        ))}
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={onClose}>Cancel</Button>
        <Button
          tone="primary"
          onClick={() => {
            const changed = columns
              .map((column, index) => ({ column: column.name, value: values[index] ?? null }))
              .filter((cell, index) => !row || cell.value !== rawValue(row[index]));
            onSave(row ? changed : changed.filter((cell) => cell.value !== null));
          }}
        >
          Review change
        </Button>
      </div>
    </Modal>
  );
}

function FilterEditor({ columns, rules, onApply, onClose }: { columns: ColumnMeta[]; rules: FilterRule[]; onApply: (rules: FilterRule[]) => void; onClose: () => void }) {
  const [list, setList] = useState<FilterRule[]>(rules);
  const update = (index: number, change: Partial<FilterRule>) =>
    setList((all) => all.map((rule, i) => (i === index ? { ...rule, ...change } : rule)));
  return (
    <Modal title="Filter rows" onClose={onClose} wide>
      <div className="space-y-2">
        {list.map((rule, index) => (
          <div key={index} className="flex gap-2">
            <Select aria-label="Column" value={rule.column} onChange={(e) => update(index, { column: e.target.value })}>
              {columns.map((c) => (
                <option key={c.name}>{c.name}</option>
              ))}
            </Select>
            <Select
              aria-label="Condition"
              value={rule.operator}
              onChange={(e) => update(index, { operator: e.target.value as FilterOperator })}
            >
              {OPERATORS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </Select>
            {!OPERATORS.find((o) => o.value === rule.operator)?.unary && (
              <Input aria-label="Value" value={rule.value ?? ''} onChange={(e) => update(index, { value: e.target.value })} />
            )}
            <Button onClick={() => setList((all) => all.filter((_, i) => i !== index))}>Remove</Button>
          </div>
        ))}
        {list.length === 0 && <p className="text-sm text-muted">No filters. Add one to narrow the rows.</p>}
      </div>
      <div className="mt-4 flex justify-between">
        <Button
          onClick={() =>
            setList((all) => [...all, { column: columns[0]?.name ?? '', operator: 'Eq', value: '' }])
          }
        >
          Add a filter
        </Button>
        <div className="flex gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button tone="primary" onClick={() => onApply(list)}>
            Apply filters
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function StructureView({ structure }: { structure: TableStructure }) {
  const { metadata: m } = structure;
  const section = 'mb-1 mt-3 text-sm font-bold';
  return (
    <div className="h-full overflow-auto p-3 text-sm">
      <p className="text-muted">
        {[m.owner && `Owner ${m.owner}`, m.sizeBytes !== null && `${(m.sizeBytes / 1024).toFixed(0)} KB`, m.rowEstimate !== null && `about ${m.rowEstimate.toLocaleString()} rows`]
          .filter(Boolean)
          .join(', ')}
      </p>
      <h3 className={section}>Columns</h3>
      <table className="w-full text-left">
        <thead className="text-muted">
          <tr>
            <th className="font-normal">Name</th>
            <th className="font-normal">Type</th>
            <th className="font-normal">Empty allowed</th>
            <th className="font-normal">Default</th>
          </tr>
        </thead>
        <tbody className="font-mono text-xs">
          {structure.columns.map((c) => (
            <tr key={c.name} className="border-t border-line">
              <td>
                {c.name}
                {c.primaryKey ? ' (key)' : ''}
              </td>
              <td>{c.dataType}</td>
              <td>{c.nullable ? 'yes' : 'no'}</td>
              <td>{c.defaultValue}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {structure.indexes.length > 0 && <h3 className={section}>Indexes</h3>}
      <ul className="font-mono text-xs">
        {structure.indexes.map((i) => (
          <li key={i.name}>
            {i.name} ({i.columns.join(', ')}){i.unique ? ', unique' : ''}
          </li>
        ))}
      </ul>
      {structure.foreignKeys.length > 0 && <h3 className={section}>Foreign keys</h3>}
      <ul className="font-mono text-xs">
        {structure.foreignKeys.map((f) => (
          <li key={f.name}>
            {f.columns.join(', ')} references {f.referencedTable} ({f.referencedColumns.join(', ')})
          </li>
        ))}
      </ul>
      {structure.constraints.length > 0 && <h3 className={section}>Constraints</h3>}
      <ul className="font-mono text-xs">
        {structure.constraints.map((c) => (
          <li key={c.name}>
            {c.name}: {c.definition ?? c.kind}
          </li>
        ))}
      </ul>
      {structure.ddl && (
        <>
          <h3 className={section}>Definition</h3>
          <Button onClick={() => void navigator.clipboard.writeText(structure.ddl ?? '')}>Copy</Button>
          <pre className="mt-1 overflow-auto rounded bg-ground p-2 font-mono text-xs">{structure.ddl}</pre>
        </>
      )}
    </div>
  );
}

export function TableTab({ tab }: { tab: TableTabData }) {
  const { sessions, targetFor, confirm } = useStudio();
  const client = useQueryClient();
  const session = sessions[tab.profileId]?.sessionId;
  const [view, setView] = useState<'data' | 'structure'>('data');
  const [offset, setOffset] = useState(0);
  const [filters, setFilters] = useState<FilterRule[]>([]);
  const [sort, setSort] = useState<{ column: string; descending: boolean } | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<{ row: Value[] | null } | null>(null);
  const [filtering, setFiltering] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const table: TableRef = tab.table;
  const key = ['schema', tab.profileId, 'page', table, offset, filters, sort];

  const page = useQuery({
    queryKey: key,
    enabled: Boolean(session),
    queryFn: () =>
      api.tablePage(session ?? '', table, { limit: PAGE_SIZE, offset, sort: sort ? [sort] : [], filters }),
  });
  const count = useQuery({
    queryKey: ['schema', tab.profileId, 'count', table, filters],
    enabled: Boolean(session),
    queryFn: () => api.tableCount(session ?? '', table, filters),
  });
  const structure = useQuery({
    queryKey: ['schema', tab.profileId, 'structure', table],
    enabled: Boolean(session),
    queryFn: () => api.tableStructure(session ?? '', table),
  });

  /** Edits are planned in Rust, then go through the same review as typed SQL. */
  const apply = async (change: RowChange) => {
    const target = targetFor(tab.profileId);
    if (!target || !session) return;
    setError(null);
    try {
      const planned = await api.planRowChange(session, table, change);
      const result = await runFlow({
        target,
        sql: planned.sql,
        origin: 'Grid',
        confirm,
        runId: newId(),
        store: new ResultStore(),
        warnings: planned.warnings,
      });
      if (result.status === 'finished') {
        const failed = result.outcomes.find((o) => o.kind === 'error');
        if (failed?.kind === 'error') setError(failed.error.message);
        setEditing(null);
        setSelected(new Set());
        void client.invalidateQueries({ queryKey: ['schema', tab.profileId] });
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const columns = page.data?.columns ?? [];
  const rows = page.data?.rows ?? [];
  const total = count.data;

  const exportTable = async (format: string) => {
    if (!session || !format) return;
    try {
      await api.exportTable(session, table, format as 'Csv', newId(), () => {});
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-2 py-1">
        <Button tone={view === 'data' ? 'primary' : 'plain'} onClick={() => setView('data')}>
          Data
        </Button>
        <Button tone={view === 'structure' ? 'primary' : 'plain'} onClick={() => setView('structure')}>
          Structure
        </Button>
        {view === 'data' && (
          <>
            <Button onClick={() => setFiltering(true)}>Filter{filters.length ? ` (${filters.length})` : ''}</Button>
            <Button onClick={() => setEditing({ row: null })}>Insert row</Button>
            <Button
              disabled={selected.size === 0}
              onClick={() =>
                void apply({
                  kind: 'delete',
                  keys: [...selected].flatMap((i) => (rows[i] ? [rowKey(columns, rows[i])] : [])),
                })
              }
            >
              Delete {selected.size || ''} rows
            </Button>
            <Button onClick={() => setImporting(true)}>Import CSV</Button>
            <Select aria-label="Export table" className="w-28" value="" onChange={(e) => void exportTable(e.target.value)}>
              <option value="">Export</option>
              <option value="Csv">CSV</option>
              <option value="JsonLines">JSON lines</option>
              <option value="Xlsx">Excel</option>
              <option value="SqlInserts">SQL inserts</option>
            </Select>
            <span className="ml-auto text-sm text-muted">
              {offset + 1} to {offset + rows.length} of {total === null ? 'many' : (total?.toLocaleString() ?? '…')}
            </span>
            <Button disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}>
              Previous
            </Button>
            <Button disabled={rows.length < PAGE_SIZE} onClick={() => setOffset(offset + PAGE_SIZE)}>
              Next
            </Button>
          </>
        )}
      </div>
      <ErrorNote message={error ?? (page.error ? errorMessage(page.error) : null)} />
      {view === 'structure' ? (
        structure.data ? (
          <StructureView structure={structure.data} />
        ) : null
      ) : (
        <div className="min-h-0 flex-1 overflow-auto">
          <table className="border-collapse font-mono text-xs">
            <thead className="sticky top-0 bg-panel">
              <tr>
                <th className="border-b border-line" aria-label="Select" />
                {columns.map((column) => (
                  <th key={column.name} className="border-b border-r border-line px-2 text-left">
                    <button
                      type="button"
                      className="font-bold"
                      title={column.typeName}
                      onClick={() =>
                        setSort((s) => (s?.column === column.name ? { ...s, descending: !s.descending } : { column: column.name, descending: false }))
                      }
                    >
                      {column.name}
                      {sort?.column === column.name ? (sort.descending ? ' ▼' : ' ▲') : ''}
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, index) => (
                <tr key={index} className="hover:bg-raised" onDoubleClick={() => setEditing({ row })}>
                  <td className="border-b border-line/40 px-1">
                    <input
                      type="checkbox"
                      aria-label={`Select row ${offset + index + 1}`}
                      checked={selected.has(index)}
                      onChange={(e) =>
                        setSelected((set) => {
                          const next = new Set(set);
                          if (e.target.checked) next.add(index);
                          else next.delete(index);
                          return next;
                        })
                      }
                    />
                  </td>
                  {columns.map((column, cell) => (
                    <td
                      key={column.name}
                      className={`max-w-xs truncate border-b border-r border-line/40 px-2 leading-6 ${row[cell]?.t === 'Null' ? 'italic text-muted' : ''}`}
                    >
                      {cellText(row[cell])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length === 0 && !page.isLoading && <p className="p-3 text-sm text-muted">No rows match.</p>}
        </div>
      )}
      {editing && (
        <RowEditor
          columns={columns}
          row={editing.row}
          onClose={() => setEditing(null)}
          onSave={(values) =>
            void apply(
              editing.row
                ? { kind: 'update', key: rowKey(columns, editing.row), values }
                : { kind: 'insert', values },
            )
          }
        />
      )}
      {filtering && (
        <FilterEditor
          columns={columns}
          rules={filters}
          onClose={() => setFiltering(false)}
          onApply={(rules) => {
            setFilters(rules);
            setOffset(0);
            setFiltering(false);
          }}
        />
      )}
      {importing && session && (
        <ImportWizard
          columns={columns.map((c) => c.name)}
          table={table}
          profileId={tab.profileId}
          onClose={() => setImporting(false)}
        />
      )}
    </div>
  );
}
