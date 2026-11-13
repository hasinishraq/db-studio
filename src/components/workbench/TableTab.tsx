import { EXPORT_LABELS, ROW_HEIGHTS, usePrefs } from '../../api/settings';
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  Download,
  Info,
  KeyRound,
  ListFilter,
  ListTree,
  Plus,
  RefreshCw,
  Rows3,
  Trash2,
  Upload,
  X,
} from 'lucide-react';
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
import { Button, cn, ErrorNote, Input, Modal, Select } from '../ui';
import { notify } from '../ui/toast';
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
  const review = () => {
    const changed = columns
      .map((column, index) => ({ column: column.name, value: values[index] ?? null }))
      .filter((cell, index) => !row || cell.value !== rawValue(row[index]));
    onSave(row ? changed : changed.filter((cell) => cell.value !== null));
  };
  return (
    <Modal
      title={row ? 'Edit row' : 'Insert row'}
      onClose={onClose}
      className="max-w-xl"
      footer={
        <>
          <span className="text-xs text-muted">Nothing changes until you review it.</span>
          <div className="ml-auto flex gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button tone="primary" onClick={review}>
              Review change
            </Button>
          </div>
        </>
      }
    >
      <div className="space-y-2">
        {columns.map((column, index) => {
          const empty = values[index] === null;
          return (
            <div key={column.name} className="grid grid-cols-[160px_minmax(0,1fr)] items-center gap-3">
              <div className="min-w-0 text-right">
                <p className="truncate text-sm">{column.name}</p>
                <p className="truncate font-mono text-2xs text-muted">{column.typeName}</p>
              </div>
              <div className="flex gap-1.5">
                <Input
                  aria-label={column.name}
                  className={cn('font-mono text-xs', empty && 'placeholder:italic')}
                  value={values[index] ?? ''}
                  placeholder="NULL"
                  onChange={(e) => set(index, e.target.value)}
                />
                <Button
                  tone={empty ? 'plain' : 'quiet'}
                  className={cn('px-2 font-mono text-2xs', empty && 'text-muted')}
                  aria-pressed={empty}
                  title="Set to NULL"
                  onClick={() => set(index, null)}
                >
                  NULL
                </Button>
              </div>
            </div>
          );
        })}
      </div>
    </Modal>
  );
}

function FilterEditor({ columns, rules, onApply, onClose }: { columns: ColumnMeta[]; rules: FilterRule[]; onApply: (rules: FilterRule[]) => void; onClose: () => void }) {
  const [list, setList] = useState<FilterRule[]>(rules);
  const update = (index: number, change: Partial<FilterRule>) =>
    setList((all) => all.map((rule, i) => (i === index ? { ...rule, ...change } : rule)));
  return (
    <Modal
      title="Filter rows"
      onClose={onClose}
      className="max-w-3xl"
      footer={
        <>
          <Button onClick={() => setList((all) => [...all, { column: columns[0]?.name ?? '', operator: 'Eq', value: '' }])}>
            <Plus size={14} aria-hidden /> Add a filter
          </Button>
          <div className="ml-auto flex gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button tone="primary" onClick={() => onApply(list)}>
              Apply filters
            </Button>
          </div>
        </>
      }
    >
      <div className="space-y-2">
        {list.map((rule, index) => (
          <div key={index} className="grid grid-cols-[44px_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_26px] items-center gap-2">
            <span className="text-right text-xs text-muted">{index === 0 ? 'Where' : 'and'}</span>
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
            {OPERATORS.find((o) => o.value === rule.operator)?.unary ? (
              <span />
            ) : (
              <Input aria-label="Value" value={rule.value ?? ''} onChange={(e) => update(index, { value: e.target.value })} />
            )}
            <Button
              tone="quiet"
              className="w-[26px] px-0 hover:text-danger"
              aria-label="Remove"
              title="Remove"
              onClick={() => setList((all) => all.filter((_, i) => i !== index))}
            >
              <Trash2 size={14} aria-hidden />
            </Button>
          </div>
        ))}
        {list.length === 0 && (
          <p className="py-6 text-center text-sm text-muted">No filters. Add one to narrow the rows.</p>
        )}
      </div>
    </Modal>
  );
}

/** A size in bytes, in the largest unit that keeps it above 1. */
export function formatBytes(bytes: number): string {
  const units = ['bytes', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${value} bytes` : `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** One block of the structure view: a heading with a count, then a table. */
function StructureBlock({ title, count, children }: { title: string; count: number; children: ReactNode }) {
  return (
    <section aria-label={title}>
      <h3 className="mb-2 flex items-center gap-2 text-xs font-semibold text-muted">
        {title}
        <span className="rounded bg-raised px-1.5 py-px text-2xs font-medium tabular-nums">{count}</span>
      </h3>
      <div className="overflow-auto rounded-md border border-line">{children}</div>
    </section>
  );
}

const TH = 'h-8 whitespace-nowrap border-b border-line bg-panel px-3 text-left text-xs font-semibold text-muted';
const TD = 'whitespace-nowrap border-b border-line/60 px-3 py-1.5';

function StructureView({ structure }: { structure: TableStructure }) {
  const { metadata: m } = structure;
  const [copied, setCopied] = useState(false);
  const facts: [string, string][] = [];
  if (m.rowEstimate !== null) facts.push(['Rows (estimate)', m.rowEstimate.toLocaleString()]);
  if (m.sizeBytes !== null) facts.push(['Size on disk', formatBytes(m.sizeBytes)]);
  if (m.owner) facts.push(['Owner', m.owner]);
  facts.push(['Columns', String(structure.columns.length)]);
  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-5xl space-y-6 p-5 text-sm">
        <dl className="flex flex-wrap gap-x-10 gap-y-3">
          {facts.map(([label, value]) => (
            <div key={label}>
              <dt className="text-xs text-muted">{label}</dt>
              <dd className="mt-0.5 text-base font-medium tabular-nums">{value}</dd>
            </div>
          ))}
        </dl>

        <StructureBlock title="Columns" count={structure.columns.length}>
          <table className="w-full border-collapse">
            <thead>
              <tr>
                <th className={TH}>Name</th>
                <th className={TH}>Type</th>
                <th className={TH}>Empty allowed</th>
                <th className={TH}>Default</th>
              </tr>
            </thead>
            <tbody className="font-mono text-xs">
              {structure.columns.map((c) => (
                <tr key={c.name} className="hover:bg-hover">
                  <td className={TD}>
                    <span className="flex items-center gap-1.5">
                      {c.primaryKey ? (
                        <KeyRound size={12} aria-label="primary key" className="text-accent" />
                      ) : (
                        <span aria-hidden className="w-3" />
                      )}
                      <span className={c.primaryKey ? 'font-semibold' : ''}>{c.name}</span>
                      {c.primaryKey && <span className="sr-only"> (key)</span>}
                    </span>
                  </td>
                  <td className={cn(TD, 'text-muted')}>{c.dataType}</td>
                  <td className={TD}>{c.nullable ? 'yes' : <span className="text-muted">no</span>}</td>
                  <td className={cn(TD, 'max-w-xs truncate text-muted')} title={c.defaultValue ?? undefined}>
                    {c.defaultValue}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </StructureBlock>

        {structure.indexes.length > 0 && (
          <StructureBlock title="Indexes" count={structure.indexes.length}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={TH}>Name</th>
                  <th className={TH}>Columns</th>
                  <th className={TH}>Unique</th>
                </tr>
              </thead>
              <tbody className="font-mono text-xs">
                {structure.indexes.map((i) => (
                  <tr key={i.name} className="hover:bg-hover">
                    <td className={TD}>{i.name}</td>
                    <td className={TD}>{i.columns.join(', ')}</td>
                    <td className={TD}>{i.unique ? 'yes' : <span className="text-muted">no</span>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </StructureBlock>
        )}

        {structure.foreignKeys.length > 0 && (
          <StructureBlock title="Foreign keys" count={structure.foreignKeys.length}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={TH}>Name</th>
                  <th className={TH}>Columns</th>
                  <th className={TH}>References</th>
                </tr>
              </thead>
              <tbody className="font-mono text-xs">
                {structure.foreignKeys.map((f) => (
                  <tr key={f.name} className="hover:bg-hover">
                    <td className={TD}>{f.name}</td>
                    <td className={TD}>{f.columns.join(', ')}</td>
                    <td className={TD}>
                      {f.referencedTable} ({f.referencedColumns.join(', ')})
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </StructureBlock>
        )}

        {structure.constraints.length > 0 && (
          <StructureBlock title="Constraints" count={structure.constraints.length}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={TH}>Name</th>
                  <th className={TH}>Definition</th>
                </tr>
              </thead>
              <tbody className="font-mono text-xs">
                {structure.constraints.map((c) => (
                  <tr key={c.name} className="hover:bg-hover">
                    <td className={TD}>{c.name}</td>
                    <td className={cn(TD, 'whitespace-normal')}>{c.definition ?? c.kind}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </StructureBlock>
        )}

        {structure.ddl && (
          <section aria-label="Definition">
            <h3 className="mb-2 text-xs font-semibold text-muted">Definition</h3>
            <div className="overflow-hidden rounded-md border border-line">
              <div className="flex h-8 items-center justify-between border-b border-line bg-panel pl-3 pr-1">
                <span className="text-xs text-muted">SQL</span>
                <Button
                  tone="quiet"
                  className="h-6 px-2 text-xs"
                  onClick={() => {
                    void navigator.clipboard.writeText(structure.ddl ?? '');
                    setCopied(true);
                    setTimeout(() => setCopied(false), 1200);
                  }}
                >
                  {copied ? <Check size={12} aria-hidden className="text-safe" /> : <Copy size={12} aria-hidden />}
                  {copied ? 'Copied' : 'Copy'}
                </Button>
              </div>
              <pre className="max-h-96 overflow-auto bg-ground p-3 font-mono text-xs leading-relaxed">{structure.ddl}</pre>
            </div>
          </section>
        )}
      </div>
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
      notify.success(`Exported ${rowCount(written)}${scope}`);
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
  const operatorLabel = (value: FilterOperator) => OPERATORS.find((o) => o.value === value);
  const removeFilter = (index: number) =>
    guarded(() => {
      setFilters((all) => all.filter((_, i) => i !== index));
      setOffset(0);
    });
  const firstRow = rows.length === 0 ? 0 : offset + 1;

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-[38px] shrink-0 select-none flex-wrap items-center gap-1 border-b border-line bg-panel px-2 py-1.5">
        <div role="group" aria-label="View" className="inline-flex rounded border border-edge/70 bg-ground p-0.5">
          {(['data', 'structure'] as const).map((choice) => {
            const Icon = choice === 'data' ? Rows3 : ListTree;
            return (
              <button
                key={choice}
                type="button"
                aria-pressed={view === choice}
                onClick={() => setView(choice)}
                className={cn(
                  'flex h-[22px] items-center gap-1.5 rounded-[3px] px-2.5 text-xs',
                  view === choice ? 'bg-raised text-ink shadow-[0_0_0_1px_var(--line)]' : 'text-muted hover:text-ink',
                )}
              >
                <Icon size={13} aria-hidden />
                {choice === 'data' ? 'Data' : 'Structure'}
              </button>
            );
          })}
        </div>
        {view === 'data' && (
          <>
            <span aria-hidden className="mx-1 h-4 w-px bg-line" />
            <Button tone="quiet" onClick={() => guarded(() => setFiltering(true))}>
              <ListFilter size={14} aria-hidden />
              Filter{filters.length ? ` (${filters.length})` : ''}
            </Button>
            <Button
              tone="quiet"
              disabled={readOnly}
              onClick={() => (editable ? setPending(addRow(pending)) : setEditing({ row: null }))}
            >
              <Plus size={14} aria-hidden />
              Insert row
            </Button>
            <Button
              tone="quiet"
              className="hover:text-danger"
              disabled={selected.size === 0 || readOnly}
              onClick={deleteSelected}
            >
              <Trash2 size={14} aria-hidden />
              Delete {selected.size || ''} rows
            </Button>
            <span aria-hidden className="mx-1 h-4 w-px bg-line" />
            <Button tone="quiet" disabled={unsaved} onClick={() => setImporting(true)}>
              <Upload size={14} aria-hidden />
              Import CSV
            </Button>
            <Button tone="quiet" title="Change the format in Settings" onClick={() => void exportTable(prefs.exportFormat)}>
              <Download size={14} aria-hidden />
              Export {selected.size > 0 ? `${selected.size} ` : ''}as {EXPORT_LABELS[prefs.exportFormat]}
            </Button>
            <Select aria-label="Export table" className="w-32" value="" onChange={(e) => void exportTable(e.target.value)}>
              <option value="">Other format</option>
              <option value="Csv">CSV</option>
              <option value="JsonLines">JSON lines</option>
              <option value="Xlsx">Excel</option>
              <option value="SqlInserts">SQL inserts</option>
            </Select>
            <span className="ml-auto flex items-center gap-1">
              <Button
                tone="quiet"
                className="w-[26px] px-0"
                aria-label="Refresh"
                title="Refresh"
                onClick={() => guarded(refresh)}
              >
                <RefreshCw size={14} aria-hidden className={page.isFetching ? 'animate-spin' : ''} />
              </Button>
              <span className="px-1 text-xs tabular-nums text-muted">
                {firstRow.toLocaleString()}–{(offset + rows.length).toLocaleString()} of{' '}
                {total === null ? 'many' : (total?.toLocaleString() ?? '…')}
              </span>
              <span className="inline-flex">
                <Button
                  className="w-[26px] rounded-r-none px-0"
                  aria-label="Previous"
                  title="Previous page"
                  disabled={offset === 0}
                  onClick={() => guarded(() => setOffset(Math.max(0, offset - PAGE_SIZE)))}
                >
                  <ChevronLeft size={14} aria-hidden />
                </Button>
                <Button
                  className="-ml-px w-[26px] rounded-l-none px-0"
                  aria-label="Next"
                  title="Next page"
                  disabled={rows.length < PAGE_SIZE}
                  onClick={() => guarded(() => setOffset(offset + PAGE_SIZE))}
                >
                  <ChevronRight size={14} aria-hidden />
                </Button>
              </span>
            </span>
          </>
        )}
      </div>
      {view === 'data' && filters.length > 0 && (
        <div aria-label="Filters" className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-line px-2 py-1.5">
          {filters.map((rule, index) => {
            const operator = operatorLabel(rule.operator);
            return (
              <span
                key={index}
                className="flex h-6 items-center gap-1 rounded-full border border-line bg-raised pl-2.5 pr-1 text-xs"
              >
                <span className="font-mono">{rule.column}</span>
                <span className="text-muted">{operator?.label ?? rule.operator}</span>
                {!operator?.unary && <span className="max-w-[12rem] truncate font-mono">{rule.value}</span>}
                <button
                  type="button"
                  aria-label={`Remove filter on ${rule.column}`}
                  className="rounded-full p-0.5 text-muted hover:bg-hover hover:text-ink"
                  onClick={() => removeFilter(index)}
                >
                  <X size={11} aria-hidden />
                </button>
              </span>
            );
          })}
          <Button
            tone="quiet"
            className="h-6 px-2 text-xs"
            onClick={() =>
              guarded(() => {
                setFilters([]);
                setOffset(0);
              })
            }
          >
            Clear filters
          </Button>
        </div>
      )}
      {view === 'data' && unsaved && (
        <div
          role="status"
          aria-label="Unsaved changes"
          className="flex shrink-0 flex-wrap items-center gap-2 border-b border-change/35 bg-change/10 px-3 py-1.5 text-sm"
        >
          <span aria-hidden className="h-1.5 w-1.5 rounded-full bg-change" />
          <span className="font-semibold">Not saved:</span>
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
          <Button tone="quiet" disabled={saving} onClick={() => setPending(emptyPending())}>
            Discard
          </Button>
          <Button tone="change" disabled={saving || tooMany} onClick={saveAll}>
            Review and save
          </Button>
        </div>
      )}
      {view === 'data' && offReason && editInfo.data && (
        <p className="flex shrink-0 items-center gap-1.5 border-b border-line px-3 py-1.5 text-xs text-muted">
          <Info size={13} aria-hidden className="shrink-0 text-accent" />
          {offReason}
        </p>
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
              <div className="flex flex-col items-center gap-2 px-3 py-12 text-sm text-muted">
                <p>{filters.length > 0 ? 'No rows match these filters.' : 'No rows match.'}</p>
                {filters.length > 0 && (
                  <Button
                    onClick={() => {
                      setFilters([]);
                      setOffset(0);
                    }}
                  >
                    Clear filters
                  </Button>
                )}
              </div>
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
        <Modal
          title="Discard unsaved changes?"
          onClose={() => setLeaving(null)}
          footer={
            <div className="ml-auto flex gap-2">
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
          }
        >
          <p className="text-sm">
            Your edits are tied to the rows on this page. Save them first, or discard them to continue.
          </p>
        </Modal>
      )}
    </div>
  );
}
