import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Download, FileText, Image, Maximize, Minus, Network, Plus, Upload } from 'lucide-react';
import type { MutableRefObject, RefObject } from 'react';
import { api, ApiError, errorMessage } from '../../api';
import { runFlow } from '../../api/run';
import type { DiagramFormat, SchemaFile, SchemaOutline, TableStructure } from '../../bindings';
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
import { diagramBounds, standaloneSvg } from '../../lib/diagramExport';
import { ResultStore } from '../../lib/resultStore';
import { spanText } from '../../lib/sqlText';
import { Button, ErrorNote, Select } from '../ui';
import { notify } from '../ui/toast';
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

const RADIUS = 6;

/** The two ends of an edge path, read from its first move and last point. */
export function edgeEnds(path: string): { start: Point; end: Point } | null {
  const numbers = path.match(/-?\d+(\.\d+)?/g)?.map(Number) ?? [];
  if (numbers.length < 4) return null;
  const [sx, sy] = numbers as [number, number];
  const ex = numbers[numbers.length - 2] as number;
  const ey = numbers[numbers.length - 1] as number;
  return { start: { x: sx, y: sy }, end: { x: ex, y: ey } };
}

function TableBox({
  table,
  at,
  onGrab,
  lit,
  dim,
  onHover,
}: {
  table: DiagramTable;
  at: Point;
  onGrab: (name: string, event: React.PointerEvent) => void;
  /** Hovered, or joined to the hovered table. */
  lit: boolean;
  /** Another table is hovered and this one is not joined to it. */
  dim: boolean;
  onHover: (name: string | null) => void;
}) {
  const foreign = new Set(table.keys.flatMap((key) => key.columns));
  const height = tableHeight(table);
  return (
    <g
      transform={`translate(${at.x},${at.y})`}
      className="cursor-move"
      data-table={table.name}
      opacity={dim ? 0.45 : 1}
      onPointerDown={(event) => onGrab(table.name, event)}
      onPointerEnter={() => onHover(table.name)}
      onPointerLeave={() => onHover(null)}
    >
      <title>{table.name}</title>
      <rect
        width={BOX_WIDTH}
        height={height}
        rx={RADIUS}
        fill="var(--raised)"
        stroke={lit ? 'var(--accent)' : 'var(--line)'}
        strokeWidth={lit ? 1.5 : 1}
      />
      <path
        d={`M0.5,${RADIUS} a${RADIUS - 0.5},${RADIUS - 0.5} 0 0 1 ${RADIUS - 0.5},${-(RADIUS - 0.5)} h${BOX_WIDTH - RADIUS * 2} a${RADIUS - 0.5},${RADIUS - 0.5} 0 0 1 ${RADIUS - 0.5},${RADIUS - 0.5} v${HEADER - RADIUS} h${-(BOX_WIDTH - 1)} z`}
        fill="var(--panel)"
      />
      <text x={12} y={18.5} fill="var(--ink)" fontWeight={600} fontSize={13}>
        {short(table.name, NAME_LENGTH + 6)}
      </text>
      <text x={BOX_WIDTH - 12} y={18.5} fill="var(--muted)" textAnchor="end" fontSize={11}>
        {table.columns.length}
      </text>
      <line x1={0} x2={BOX_WIDTH} y1={HEADER} y2={HEADER} stroke={lit ? 'var(--accent)' : 'var(--line)'} />
      {table.columns.map((column, index) => {
        const top = HEADER + index * ROW;
        const y = top + 14;
        const key = column.primaryKey;
        const mark = key ? 'PK' : foreign.has(column.name) ? 'FK' : '';
        return (
          <g key={column.name} fontSize={12} fontFamily="JetBrains Mono, monospace">
            <title>{`${column.name} ${column.type}`}</title>
            {index > 0 && <line x1={10} x2={BOX_WIDTH - 10} y1={top} y2={top} stroke="var(--line)" strokeOpacity={0.5} />}
            <text x={12} y={y} fill={key ? 'var(--accent)' : 'var(--muted)'} fontSize={9.5} fontWeight={700}>
              {mark}
            </text>
            <text x={36} y={y} fill="var(--ink)" fontWeight={key ? 600 : 400}>
              {short(column.name, NAME_LENGTH - 6)}
            </text>
            <text x={BOX_WIDTH - 12} y={y} fill="var(--muted)" textAnchor="end" fontSize={11}>
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
  drawingRef,
  positionsRef,
}: {
  tables: DiagramTable[];
  label: string;
  fitRequest: number;
  zoomRequest: { factor: number; id: number };
  onZoom: (k: number) => void;
  /** The drawn content, for saving as an image. */
  drawingRef: RefObject<SVGGElement | null>;
  positionsRef: MutableRefObject<Positions>;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const [positions, setPositions] = useState<Positions>({});
  positionsRef.current = positions;
  const [view, setView] = useState<View>({ x: 24, y: 24, k: 1 });
  const [hovered, setHovered] = useState<string | null>(null);
  const drag = useRef<Drag | null>(null);
  const grid = `er-grid-${useId().replace(/[^a-zA-Z0-9]/g, '')}`;

  // Which tables each edge joins, so hovering one table lights its relationships.
  const joins = useMemo(() => {
    const map = new Map<string, [string, string]>();
    for (const table of tables) {
      table.keys.forEach((key, index) => map.set(`${table.name}:${index}`, [table.name, key.table]));
    }
    return map;
  }, [tables]);
  const neighbours = useMemo(() => {
    if (!hovered) return null;
    const near = new Set([hovered]);
    for (const [from, to] of joins.values()) {
      if (from === hovered) near.add(to);
      if (to === hovered) near.add(from);
    }
    return near;
  }, [hovered, joins]);


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
      className="min-h-0 flex-1 cursor-grab touch-none select-none overflow-hidden bg-ground active:cursor-grabbing"
      onPointerDown={(event) => startDrag({ kind: 'pan', from: { x: event.clientX, y: event.clientY }, view })}
    >
      <svg width="100%" height="100%" role="img" aria-label={label}>
        {/* The dot grid moves and scales with the drawing, but is not part of a saved image. */}
        <defs>
          <pattern
            id={grid}
            width={20}
            height={20}
            patternUnits="userSpaceOnUse"
            patternTransform={`translate(${view.x},${view.y}) scale(${view.k})`}
          >
            <circle cx={1} cy={1} r={1} fill="var(--edge)" fillOpacity={0.45} />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill={`url(#${grid})`} />
        <g ref={drawingRef} transform={`translate(${view.x},${view.y}) scale(${view.k})`}>
          {edges(tables, positions).map((edge) => {
            const [from, to] = joins.get(edge.id) ?? ['', ''];
            const lit = Boolean(hovered) && (from === hovered || to === hovered);
            const dim = Boolean(hovered) && !lit;
            const ends = edgeEnds(edge.path);
            const stroke = lit ? 'var(--accent)' : 'var(--muted)';
            return (
              <g key={edge.id} opacity={dim ? 0.25 : 1}>
                <path d={edge.path} fill="none" stroke={stroke} strokeWidth={lit ? 2 : 1.25} />
                {ends && (
                  <>
                    <circle cx={ends.start.x} cy={ends.start.y} r={3} fill="var(--raised)" stroke={stroke} strokeWidth={1.25} />
                    <circle cx={ends.end.x} cy={ends.end.y} r={3} fill={stroke} />
                  </>
                )}
              </g>
            );
          })}
          {tables.map((table) => {
            const at = positions[table.name];
            return at ? (
              <TableBox
                key={table.name}
                table={table}
                at={at}
                onGrab={grab}
                lit={Boolean(neighbours?.has(table.name))}
                dim={Boolean(neighbours) && !neighbours?.has(table.name)}
                onHover={(name) => {
                  if (!drag.current) setHovered(name);
                }}
              />
            ) : null;
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
  const [saving, setSaving] = useState(false);
  const drawingRef = useRef<SVGGElement>(null);
  const positionsRef = useRef<Positions>({});

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
      notify.success(`Exported the structure of ${tab.schema}: ${objects} tables and views`);
      log.add('info', `Exported the structure of ${tab.schema}: ${objects} tables and views`);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    }
  };

  /** Rust draws the image and asks where to save it. */
  const exportImage = async (format: DiagramFormat) => {
    const drawing = drawingRef.current;
    const bounds = diagramBounds(tables, positionsRef.current);
    if (!drawing || !bounds) return;
    setError(null);
    setSaving(true);
    try {
      const name = file ? file.source.name.replace(/\.sql$/i, '') : tab.schema;
      await api.exportDiagram(standaloneSvg(drawing, bounds), format, name);
      log.add('info', `Saved the diagram of ${name} as ${format === 'Jpeg' ? 'JPEG' : format.toUpperCase()}`);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === 'Cancelled')) setError(errorMessage(e));
    } finally {
      setSaving(false);
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
      <div className="flex min-h-[38px] shrink-0 select-none flex-wrap items-center gap-1 border-b border-line bg-panel px-2 py-1.5">
        <span className="flex items-center gap-1.5 px-1 text-xs text-muted">
          <Network size={14} aria-hidden className="text-accent" />
          <span className="font-semibold text-ink">{file ? file.source.name : tab.schema}</span>
          <span className="tabular-nums">{tables.length === 1 ? '1 table' : `${tables.length} tables`}</span>
        </span>
        <span aria-hidden className="mx-1 h-4 w-px bg-line" />
        <Button tone="quiet" disabled={!session || Boolean(file)} onClick={() => void exportSchema()}>
          <Download size={14} aria-hidden /> Export schema
        </Button>
        <Button tone="quiet" disabled={!session} onClick={() => void importSchema()}>
          <Upload size={14} aria-hidden /> Import schema
        </Button>
        <span className="relative">
          <Image size={13} aria-hidden className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-muted" />
          <Select
            aria-label="Export image"
            className="w-36 pl-7"
            value=""
            disabled={tables.length === 0 || saving}
            onChange={(e) => e.target.value && void exportImage(e.target.value as DiagramFormat)}
          >
            <option value="">Export image</option>
            <option value="Png">PNG image</option>
            <option value="Jpeg">JPEG image</option>
            <option value="Pdf">PDF document</option>
          </Select>
        </span>
        <span className="ml-auto flex items-center gap-1">
          <span className="inline-flex items-center rounded border border-edge/70 bg-ground">
            <Button tone="quiet" className="h-6 w-6 rounded-r-none px-0" aria-label="Zoom out" onClick={() => zoomBy(0.8)}>
              <Minus size={13} aria-hidden />
            </Button>
            <output className="w-12 border-x border-line text-center text-xs tabular-nums">{Math.round(zoom * 100)}%</output>
            <Button tone="quiet" className="h-6 w-6 rounded-l-none px-0" aria-label="Zoom in" onClick={() => zoomBy(1.25)}>
              <Plus size={13} aria-hidden />
            </Button>
          </span>
          <Button tone="quiet" onClick={() => setFitRequest((n) => n + 1)} title="Fit every table on screen">
            <Maximize size={13} aria-hidden /> Fit
          </Button>
        </span>
      </div>
      {file && (
        <div
          role="status"
          className="flex shrink-0 flex-wrap items-center gap-2 border-b border-accent/30 bg-accent/10 px-3 py-1.5 text-sm"
        >
          <FileText size={14} aria-hidden className="shrink-0 text-accent" />
          <span className="min-w-0 flex-1">
            Preview of {file.source.name}: {file.outline.tables.length} tables. Nothing has been applied.
            {file.outline.unread > 0 && ` ${file.outline.unread} statements could not be read and are not drawn.`}
          </span>
          <Button tone="quiet" disabled={applying} onClick={() => setFile(null)}>
            Close preview
          </Button>
          <Button tone="change" disabled={applying} onClick={() => void apply()}>
            Apply to {destination}
          </Button>
        </div>
      )}
      <ErrorNote message={failure} />
      <WithLog log={log}>
        {tables.length === 0 && !failure && !live.isLoading && (
          <div className="flex flex-col items-center gap-3 px-3 pb-6 pt-16 text-center text-sm text-muted">
            <span className="flex h-10 w-10 items-center justify-center rounded-lg border border-line bg-panel text-accent">
              <Network size={18} aria-hidden />
            </span>
            <p>{file ? 'This file creates no tables.' : 'This schema has no tables. Import a schema file to add some.'}</p>
            {!file && (
              <Button disabled={!session} onClick={() => void importSchema()}>
                <Upload size={14} aria-hidden /> Import schema
              </Button>
            )}
          </div>
        )}
        {live.isLoading && !file && (
          <p className="pt-16 text-center text-sm text-muted">Reading the tables in {tab.schema}</p>
        )}
        <Canvas
          tables={tables}
          label={file ? `Tables in ${file.source.name}` : `Tables and relationships in ${tab.schema}`}
          fitRequest={fitRequest}
          zoomRequest={zoomRequest}
          onZoom={setZoom}
          drawingRef={drawingRef}
          positionsRef={positionsRef}
        />
      </WithLog>
    </div>
  );
}
