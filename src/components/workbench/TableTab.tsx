import { EXPORT_LABELS, ROW_HEIGHTS, usePrefs } from '../../api/settings';
import { useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, errorMessage } from '../../api';
import { runChanges } from '../../api/run';
import type {
  CellValue,
  ColumnMeta,
  ExportFormat,
  FilterOperator,
  FilterRule,
  GridEditInfo,
  PlannedSql,
  RowChange,
  TablePage,
  TableRef,
  TableStructure,
  Value,
} from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import {
  addRow,
  counts,
  emptyPending,
  hasPending,
  rawValue,
  toChangeSet,
  toggleDeletes,
  type Pending,
} from '../../lib/pendingEdits';
import { rowCount } from '../../lib/sqlText';
import { ImportWizard } from '../modals/ImportWizard';
import { Button, ErrorNote, Field, Input, Modal, Select } from '../ui';
import { EditableGrid, type Editable } from './EditableGrid';
import { useRunLog, WithLog } from './RunLog';

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

/** Why inline editing is off, or `null` when it is on. */
function editOffReason(info: GridEditInfo | undefined, readOnly: boolean): string | null {
  if (readOnly) return 'This connection is read-only, so rows cannot be changed.';
  if (!info) return null;
  if (info.primaryKey.length === 0) {
    return 'This table has no primary key, so cells cannot be edited in place. Double-click a row to edit it in a form.';
  }
  const hidden = info.primaryKey.find((name) => info.locked.some((l) => l.name === name && l.reason === 'Masked'));
  if (hidden) return `The key column ${hidden} is masked, so rows cannot be edited in place.`;
  return null;
}

export function TableTab({ tab }: { tab: TableTabData }) {
  const { sessions, targetFor, confirm, profiles, setUnsaved } = useStudio();
  const client = useQueryClient();
  const session = sessions[tab.profileId]?.sessionId;
  const readOnly = profiles.find((p) => p.id === tab.profileId)?.readOnly ?? false;
  const [view, setView] = useState<'data' | 'structure'>('data');
  const [offset, setOffset] = useState(0);
  const [filters, setFilters] = useState<FilterRule[]>([]);
  const [sort, setSort] = useState<{ column: string; descending: boolean } | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<{ row: Value[] | null } | null>(null);
  const [filtering, setFiltering] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPendingState] = useState<Pending>(emptyPending);
  // While edits are pending the grid shows the rows they were made on, even if a refresh lands.
  const [frozen, setFrozen] = useState<TablePage | null>(null);
  const [leaving, setLeaving] = useState<(() => void) | null>(null);
  const [saving, setSaving] = useState(false);
  const log = useRunLog();
  const prefs = usePrefs();
  const table: TableRef = tab.table;
  const key = ['schema', tab.profileId, 'page', table, offset, filters, sort];
  const unsaved = hasPending(pending);

  // The selection and pending edits are by position on the page, so they end when the page changes.
  useEffect(() => setSelected(new Set()), [offset, filters, sort]);
  useEffect(() => setUnsaved(tab.id, unsaved), [setUnsaved, tab.id, unsaved]);
  useEffect(() => () => setUnsaved(tab.id, false), [setUnsaved, tab.id]);

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
  const editInfo = useQuery({
    queryKey: ['schema', tab.profileId, 'edit-info', table],
    enabled: Boolean(session),
    queryFn: () => api.gridEditInfo(session ?? '', table),
  });

  const offReason = editOffReason(editInfo.data, readOnly);
  const editable: Editable | null = useMemo(
    () =>
      editInfo.data && !offReason
        ? { locked: new Map(editInfo.data.locked.map((l) => [l.name, l.reason] as const)) }
        : null,
    [editInfo.data, offReason],
  );

  const shown = frozen ?? page.data;
  const columns = shown?.columns ?? [];
  const rows = shown?.rows ?? [];

  const setPending = (next: Pending) => {
    setPendingState(next);
    if (!hasPending(next)) setFrozen(null);
    else if (!frozen) setFrozen(page.data ?? null);
  };
  const total = count.data;
  const tally = counts(pending);

  /** Runs `action` now, or after the user agrees to drop unsaved edits. */
  const guarded = (action: () => void) => {
    if (unsaved) setLeaving(() => action);
    else action();
  };

  const refresh = () => void client.invalidateQueries({ queryKey: ['schema', tab.profileId] });

  /** Rust plans the SQL; the user reviews it; it runs all or nothing through the grid's door. */
  const save = async (plan: () => Promise<PlannedSql>, done: () => void) => {
    const target = targetFor(tab.profileId);
    if (!target || !session) return;
    setError(null);
    setSaving(true);
    let sql: string | undefined;
    try {
      const planned = await plan();
      sql = planned.sql;
      const result = await runChanges({ target, sql, confirm, runId: newId(), warnings: planned.warnings });
      if (result.status === 'declined') {
        log.add('info', 'Change not made: confirmation declined', sql);
        return;
      }
      log.add('change', `Saved ${result.saved === 1 ? '1 change' : `${result.saved} changes`}`, sql);
      done();
      refresh();
    } catch (e) {
      setError(errorMessage(e));
      log.add('error', errorMessage(e), sql);
    } finally {
      setSaving(false);
    }
  };

  const saveAll = () =>
    void save(
      () => api.planRowChanges(session ?? '', table, toChangeSet(pending, columns, rows, editInfo.data?.primaryKey ?? [])),
      () => {
        setPending(emptyPending());
        setSelected(new Set());
      },
    );

  /** The form path, for tables without a primary key. */
  const apply = (change: RowChange) =>
    void save(
      () => api.planRowChange(session ?? '', table, change),
      () => {
        setEditing(null);
        setSelected(new Set());
      },
    );

  const exportTable = async (format: string) => {
    if (!session || !format) return;
    setError(null);
    try {
      const picked = [...selected].sort((a, b) => a - b).flatMap((i) => (rows[i] ? [rows[i]] : []));
      const written =
        picked.length > 0
          ? await api.exportRows(session, columns, picked, format as ExportFormat, table)
          : await api.exportTable(session, table, filters, format as ExportFormat, newId(), () => {});
      const scope = picked.length > 0 ? ' (selected)' : filters.length > 0 ? ' (filtered)' : '';
      log.add('info', `Exported ${rowCount(written)}${scope}`);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  const deleteSelected = () => {
    const picked = [...selected].sort((a, b) => a - b);
    if (editable) {
      setPending(toggleDeletes(pending, picked));
      setSelected(new Set());
    } else {
      apply({ kind: 'delete', keys: picked.flatMap((i) => (rows[i] ? [rowKey(columns, rows[i])] : [])) });
    }
  };

  const tooMany = editInfo.data ? tally.changed + tally.added + tally.deleted > editInfo.data.maxChanges : false;

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-2 py-1">
        <div role="group" aria-label="View" className="flex overflow-hidden rounded border border-edge">
          {(['data', 'structure'] as const).map((choice) => (
            <button
              key={choice}
              type="button"
              aria-pressed={view === choice}
              onClick={() => setView(choice)}
              className={`px-3 py-1 text-sm ${view === choice ? 'here-top bg-pick font-bold' : 'bg-raised text-muted hover:text-ink'}`}
            >
              {choice === 'data' ? 'Data' : 'Structure'}
            </button>
          ))}
        </div>
        {view === 'data' && (
          <>
            <Button onClick={() => guarded(() => setFiltering(true))}>Filter{filters.length ? ` (${filters.length})` : ''}</Button>
            <Button
              disabled={readOnly}
              onClick={() => (editable ? setPending(addRow(pending)) : setEditing({ row: null }))}
            >
              Insert row
            </Button>
            <Button disabled={selected.size === 0 || readOnly} onClick={deleteSelected}>
              Delete {selected.size || ''} rows
            </Button>
            <Button disabled={unsaved} onClick={() => setImporting(true)}>
              Import CSV
            </Button>
            <Button title="Change the format in Settings" onClick={() => void exportTable(prefs.exportFormat)}>
              Export {selected.size > 0 ? `${selected.size} ` : ''}as {EXPORT_LABELS[prefs.exportFormat]}
            </Button>
            <Select aria-label="Export table" className="w-32" value="" onChange={(e) => void exportTable(e.target.value)}>
              <option value="">Other format</option>
              <option value="Csv">CSV</option>
              <option value="JsonLines">JSON lines</option>
              <option value="Xlsx">Excel</option>
              <option value="SqlInserts">SQL inserts</option>
            </Select>
            <span className="ml-auto text-sm text-muted">
              {offset + 1} to {offset + rows.length} of {total === null ? 'many' : (total?.toLocaleString() ?? '…')}
            </span>
            <Button disabled={offset === 0} onClick={() => guarded(() => setOffset(Math.max(0, offset - PAGE_SIZE)))}>
              Previous
            </Button>
            <Button disabled={rows.length < PAGE_SIZE} onClick={() => guarded(() => setOffset(offset + PAGE_SIZE))}>
              Next
            </Button>
          </>
        )}
      </div>
      {view === 'data' && unsaved && (
        <div role="status" aria-label="Unsaved changes" className="flex flex-wrap items-center gap-2 border-b border-line bg-change/10 px-2 py-1 text-sm">
          <span className="font-bold">Not saved:</span>
          <span>
            {[
              tally.changed && `${tally.changed} changed`,
              tally.added && `${tally.added} new`,
              tally.deleted && `${tally.deleted} deleted`,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
          {tooMany && <span className="text-danger">One save holds at most {editInfo.data?.maxChanges} changes.</span>}
          <span className="ml-auto" />
          <Button disabled={saving} onClick={() => setPending(emptyPending())}>
            Discard
          </Button>
          <Button tone="change" disabled={saving || tooMany} onClick={saveAll}>
            Review and save
          </Button>
        </div>
      )}
      {view === 'data' && offReason && editInfo.data && (
        <p className="border-b border-line px-2 py-1 text-xs text-muted">{offReason}</p>
      )}
      <ErrorNote message={error ?? (page.error ? errorMessage(page.error) : null)} />
      <WithLog log={log}>
        {view === 'structure' ? (
          structure.data ? (
            <StructureView structure={structure.data} />
          ) : null
        ) : (
          <div className="min-h-0 flex-1 overflow-auto">
            <EditableGrid
              columns={columns}
              rows={rows}
              offset={offset}
              nullText={prefs.nullText}
              rowHeight={ROW_HEIGHTS[prefs.rowDensity]}
              pending={pending}
              onPending={setPending}
              editable={editable}
              selected={selected}
              onSelected={setSelected}
              sort={sort}
              onSort={(column) =>
                guarded(() =>
                  setSort((s) => (s?.column === column ? { ...s, descending: !s.descending } : { column, descending: false })),
                )
              }
              onRowOpen={(row) => editInfo.data && !editable && !readOnly && setEditing({ row })}
            />
            {rows.length === 0 && pending.inserts.length === 0 && !page.isLoading && (
              <p className="p-3 text-sm text-muted">No rows match.</p>
            )}
          </div>
        )}
      </WithLog>
      {editing && (
        <RowEditor
          columns={columns}
          row={editing.row}
          onClose={() => setEditing(null)}
          onSave={(values) =>
            apply(
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
      {leaving && (
        <Modal title="Discard unsaved changes?" onClose={() => setLeaving(null)}>
          <p className="text-sm">
            Your edits are tied to the rows on this page. Save them first, or discard them to continue.
          </p>
          <div className="mt-4 flex justify-end gap-2">
            <Button onClick={() => setLeaving(null)}>Keep editing</Button>
            <Button
              tone="danger"
              onClick={() => {
                const action = leaving;
                setLeaving(null);
                setPending(emptyPending());
                action();
              }}
            >
              Discard and continue
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}
