import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Download, Maximize, Minus, Plus, Upload } from 'lucide-react';
import { api, ApiError, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { SchemaFile, SchemaOutline, TableStructure } from '../../bindings';
import { newId, useStudio, type Tab } from '../../context/Studio';
import {
  arrange,
  BOX_WIDTH,
  edges,
  fit,
  fromOutline,
  fromStructures,
  HEADER,
  ROW,
  tableHeight,
  zoomAt,
  type DiagramTable,
  type Point,
  type Positions,
  type View,
} from '../../lib/diagram';
import { ResultStore } from '../../lib/resultStore';
import { spanText } from '../../lib/sqlText';
import { Button, ErrorNote } from '../ui';
import { useRunLog, WithLog } from './RunLog';

type ErTab = Extract<Tab, { kind: 'er' }>;

const AT_ONCE = 8;
const NAME_LENGTH = 26;
const TYPE_LENGTH = 16;

function short(text: string, length: number): string {
  return text.length > length ? `${text.slice(0, length - 1)}…` : text;
}

/** Reads every table, a few at a time so a big schema does not take every connection. */
async function readTables(session: string, schema: string): Promise<TableStructure[]> {
  const objects = await api.listObjects(session, { name: schema });
  const found = objects.filter((object) => object.kind === 'Table');
  const tables: TableStructure[] = [];
  for (let start = 0; start < found.length; start += AT_ONCE) {
    const batch = found.slice(start, start + AT_ONCE);
    tables.push(
      ...(await Promise.all(batch.map((o) => api.tableStructure(session, { schema: o.schema, name: o.name })))),
    );
  }
  return tables;
}

function TableBox({
  table,
  at,
  onGrab,
}: {
  table: DiagramTable;
  at: Point;
  onGrab: (name: string, event: React.PointerEvent) => void;
}) {
  const foreign = new Set(table.keys.flatMap((key) => key.columns));
  return (
    <g
      transform={`translate(${at.x},${at.y})`}
      className="cursor-move"
      data-table={table.name}
      onPointerDown={(event) => onGrab(table.name, event)}
    >
      <title>{table.name}</title>
      <rect width={BOX_WIDTH} height={tableHeight(table)} rx={4} fill="var(--raised)" stroke="var(--line)" />
      <text x={10} y={19} fill="var(--ink)" fontWeight={700} fontSize={14}>
        {short(table.name, NAME_LENGTH + 6)}
      </text>
      <line x1={0} x2={BOX_WIDTH} y1={HEADER} y2={HEADER} stroke="var(--line)" />
      {table.columns.map((column, index) => {
        const y = HEADER + index * ROW + 14;
        const mark = column.primaryKey ? 'PK' : foreign.has(column.name) ? 'FK' : '';
        return (
          <g key={column.name} fontSize={12} fontFamily="JetBrains Mono, monospace">
            <title>{`${column.name} ${column.type}`}</title>
            <text x={10} y={y} fill="var(--muted)" fontSize={10} fontWeight={700}>
              {mark}
            </text>
            <text x={32} y={y} fill="var(--ink)" fontWeight={column.primaryKey ? 700 : 400}>
              {short(column.name, NAME_LENGTH - 6)}
            </text>
            <text x={BOX_WIDTH - 10} y={y} fill="var(--muted)" textAnchor="end" fontSize={11}>
              {short(column.type, TYPE_LENGTH)}
            </text>
          </g>
        );
      })}
    </g>
  );
}

type Drag =
  | { kind: 'pan'; from: Point; view: View }
  | { kind: 'table'; name: string; from: Point; at: Point };

/** Pan by dragging the background, drag tables to arrange them, scroll to zoom. */
function Canvas({
  tables,
  label,
  fitRequest,
  onZoom,
  zoomRequest,
}: {
  tables: DiagramTable[];
  label: string;
  fitRequest: number;
  zoomRequest: { factor: number; id: number };
  onZoom: (k: number) => void;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const [positions, setPositions] = useState<Positions>({});
  const [view, setView] = useState<View>({ x: 24, y: 24, k: 1 });
  const drag = useRef<Drag | null>(null);

  const fitAll = useCallback(
    (at: Positions) => {
      const box = frame.current;
      if (box) setView(fit(tables, at, box.clientWidth, box.clientHeight));
    },
    [tables],
  );

  useEffect(() => {
    const next = arrange(tables);
    setPositions(next);
    fitAll(next);
  }, [tables, fitAll]);

  useEffect(() => {
    if (fitRequest > 0) setPositions((at) => (fitAll(at), at));
  }, [fitRequest, fitAll]);

  useEffect(() => {
    const box = frame.current;
    if (zoomRequest.id === 0 || !box) return;
    const middle = { x: box.clientWidth / 2, y: box.clientHeight / 2 };
    setView((v) => zoomAt(v, zoomRequest.factor, middle));
  }, [zoomRequest]);

  useEffect(() => onZoom(view.k), [view.k, onZoom]);

  // React's wheel listener is passive, and this one must stop the page from scrolling.
  useEffect(() => {
    const box = frame.current;
    if (!box) return undefined;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const edge = box.getBoundingClientRect();
      const at = { x: event.clientX - edge.left, y: event.clientY - edge.top };
      setView((v) => zoomAt(v, Math.exp(-event.deltaY * 0.0015), at));
    };
    box.addEventListener('wheel', onWheel, { passive: false });
    return () => box.removeEventListener('wheel', onWheel);
  }, []);

  const startDrag = (next: Drag) => {
    drag.current = next;
    const move = (event: PointerEvent) => {
      const current = drag.current;
      if (!current) return;
      const dx = event.clientX - current.from.x;
      const dy = event.clientY - current.from.y;
      if (current.kind === 'pan') {
        setView({ ...current.view, x: current.view.x + dx, y: current.view.y + dy });
      } else {
        setPositions((at) => ({
          ...at,
          [current.name]: { x: current.at.x + dx / view.k, y: current.at.y + dy / view.k },
        }));
      }
    };
    const up = () => {
      drag.current = null;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const grab = (name: string, event: React.PointerEvent) => {
    event.stopPropagation();
    const at = positions[name];
    if (at) startDrag({ kind: 'table', name, from: { x: event.clientX, y: event.clientY }, at });
  };

  return (
    <div
      ref={frame}
      className="min-h-0 flex-1 cursor-grab touch-none select-none overflow-hidden"
      onPointerDown={(event) => startDrag({ kind: 'pan', from: { x: event.clientX, y: event.clientY }, view })}
    >
      <svg width="100%" height="100%" role="img" aria-label={label}>
        <g transform={`translate(${view.x},${view.y}) scale(${view.k})`}>
          {edges(tables, positions).map((edge) => (
            <path key={edge.id} d={edge.path} fill="none" stroke="var(--muted)" strokeWidth={1.5} />
          ))}
          {tables.map((table) => {
            const at = positions[table.name];
            return at ? <TableBox key={table.name} table={table} at={at} onGrab={grab} /> : null;
          })}
        </g>
      </svg>
    </div>
  );
}

export function ErDiagram({ tab }: { tab: ErTab }) {
  const { sessions, targetFor, confirm, setSession } = useStudio();
  const client = useQueryClient();
  const open = sessions[tab.profileId];
  const session = open?.sessionId;
  const log = useRunLog();
  const [error, setError] = useState<string | null>(null);
  const [file, setFile] = useState<{ source: SchemaFile; outline: SchemaOutline } | null>(null);
  const [applying, setApplying] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [fitRequest, setFitRequest] = useState(0);
  const [zoomRequest, setZoomRequest] = useState({ factor: 1, id: 0 });

  const live = useQuery({
    queryKey: ['schema', tab.profileId, 'er', session, tab.schema],
    enabled: Boolean(session),
    queryFn: () => readTables(session ?? '', tab.schema),
  });
  const tables = useMemo(
    () => (file ? fromOutline(file.outline) : fromStructures(live.data ?? [])),
    [file, live.data],
  );

  const zoomBy = (factor: number) => setZoomRequest((z) => ({ factor, id: z.id + 1 }));

  const exportSchema = async () => {
    if (!session) return;
    setError(null);
    try {
      const objects = await api.exportSchema(session, tab.schema);
      log.add('info', `Exported the structure of ${tab.schema}: ${objects} tables and views`);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  const importSchema = async () => {
    if (!open) return;
    setError(null);
    try {
      const source = await api.openSchemaFile();
      setFile({ source, outline: await api.outlineSchema(source.sql, open.engine) });
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  const destination = open?.engine === 'MySql' ? tab.schema : (open?.database ?? tab.schema);

  const apply = async () => {
    const target = targetFor(tab.profileId);
    if (!file || !open || !target) return;
    const sql = file.source.sql;
    setError(null);
    setApplying(true);
    try {
      // Unqualified names land in the default database, so that must be the one on screen.
      if (open.engine === 'MySql' && open.database !== tab.schema) {
        setSession(tab.profileId, await api.switchDatabase(open.sessionId, tab.schema));
      }
      const result = await runFlow({
        target,
        sql,
        origin: 'Editor',
        confirm,
        runId: newId(),
        store: new ResultStore(),
        warnings: ['If a statement fails, the statements before it stay applied.'],
      });
      if (result.status === 'declined') {
        log.add('info', `${file.source.name} not applied: confirmation declined`);
        return;
      }
      let failed = 0;
      result.outcomes.forEach((outcome, index) => {
        if (outcome.kind !== 'error') return;
        failed += 1;
        const verdict = result.verdicts[index];
        log.add('error', outcome.error.message, verdict ? spanText(sql, verdict.span) : null);
      });
      const ran = result.outcomes.length - failed;
      log.add(failed ? 'error' : 'change', `${file.source.name}: ${ran} of ${result.verdicts.length} statements ran`);
      if (failed === 0) setFile(null);
      void client.invalidateQueries({ queryKey: ['schema', tab.profileId] });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setApplying(false);
    }
  };

  const failure = error ?? (live.error ? errorMessage(live.error) : null);
  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-2 py-1">
        <Button disabled={!session || Boolean(file)} onClick={() => void exportSchema()}>
          <Download size={13} aria-hidden /> Export schema
        </Button>
        <Button disabled={!session} onClick={() => void importSchema()}>
          <Upload size={13} aria-hidden /> Import schema
        </Button>
        <span className="ml-auto flex items-center gap-1">
          <Button tone="quiet" aria-label="Zoom out" onClick={() => zoomBy(0.8)}>
            <Minus size={13} aria-hidden />
          </Button>
          <output className="w-12 text-center text-sm">{Math.round(zoom * 100)}%</output>
          <Button tone="quiet" aria-label="Zoom in" onClick={() => zoomBy(1.25)}>
            <Plus size={13} aria-hidden />
          </Button>
          <Button tone="quiet" onClick={() => setFitRequest((n) => n + 1)}>
            <Maximize size={13} aria-hidden /> Fit
          </Button>
        </span>
      </div>
      {file && (
        <div className="flex flex-wrap items-center gap-2 border-b border-line px-2 py-1 text-sm">
          <span>
            Preview of {file.source.name}: {file.outline.tables.length} tables. Nothing has been applied.
            {file.outline.unread > 0 && ` ${file.outline.unread} statements could not be read and are not drawn.`}
          </span>
          <Button tone="change" disabled={applying} onClick={() => void apply()}>
            Apply to {destination}
          </Button>
          <Button disabled={applying} onClick={() => setFile(null)}>
            Close preview
          </Button>
        </div>
      )}
      <ErrorNote message={failure} />
      <WithLog log={log}>
        {tables.length === 0 && !failure && !live.isLoading && (
          <p className="p-3 text-sm text-muted">
            {file ? 'This file creates no tables.' : 'This schema has no tables. Import a schema file to add some.'}
          </p>
        )}
        <Canvas
          tables={tables}
          label={file ? `Tables in ${file.source.name}` : `Tables and relationships in ${tab.schema}`}
          fitRequest={fitRequest}
          zoomRequest={zoomRequest}
          onZoom={setZoom}
        />
      </WithLog>
    </div>
  );
}
