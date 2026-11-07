import { useQuery } from '@tanstack/react-query';
import { api, errorMessage } from '../../api';
import type { TableStructure } from '../../bindings';
import { useStudio, type Tab } from '../../context/Studio';
import { ErrorNote } from '../ui';

type ErTab = Extract<Tab, { kind: 'er' }>;

const BOX_WIDTH = 220;
const ROW = 18;
const HEADER = 24;
const GAP = 60;
const PER_ROW = 4;

function layout(tables: TableStructure[]) {
  const boxes = new Map<string, { x: number; y: number; height: number }>();
  let y = 20;
  for (let start = 0; start < tables.length; start += PER_ROW) {
    const row = tables.slice(start, start + PER_ROW);
    let tallest = 0;
    row.forEach((table, index) => {
      const height = HEADER + Math.min(table.columns.length, 12) * ROW + 6;
      tallest = Math.max(tallest, height);
      boxes.set(table.table.name, { x: 20 + index * (BOX_WIDTH + GAP), y, height });
    });
    y += tallest + GAP;
  }
  return { boxes, height: y, width: 20 + PER_ROW * (BOX_WIDTH + GAP) };
}

export function ErDiagram({ tab }: { tab: ErTab }) {
  const { sessions } = useStudio();
  const session = sessions[tab.profileId]?.sessionId;
  const { data: tables = [], error } = useQuery({
    queryKey: ['schema', tab.profileId, 'er', tab.schema],
    enabled: Boolean(session),
    queryFn: async () => {
      const objects = await api.listObjects(session ?? '', { name: tab.schema });
      const found = objects.filter((o) => o.kind === 'Table').slice(0, 60);
      return Promise.all(found.map((o) => api.tableStructure(session ?? '', { schema: o.schema, name: o.name })));
    },
  });
  const { boxes, width, height } = layout(tables);
  return (
    <div className="h-full overflow-auto">
      <ErrorNote message={error ? errorMessage(error) : null} />
      <svg width={width} height={height} role="img" aria-label={`Tables and relationships in ${tab.schema}`}>
        {tables.flatMap((table) =>
          table.foreignKeys.map((key) => {
            const from = boxes.get(table.table.name);
            const to = boxes.get(key.referencedTable);
            if (!from || !to) return null;
            const startX = from.x + BOX_WIDTH;
            const startY = from.y + HEADER / 2;
            const endY = to.y + HEADER / 2;
            const middle = (startX + to.x) / 2;
            return (
              <path
                key={`${table.table.name}${key.name}`}
                d={`M${startX},${startY} C${middle},${startY} ${middle},${endY} ${to.x},${endY}`}
                fill="none"
                stroke="var(--muted)"
              />
            );
          }),
        )}
        {tables.map((table) => {
          const box = boxes.get(table.table.name);
          if (!box) return null;
          return (
            <g key={table.table.name} transform={`translate(${box.x},${box.y})`}>
              <rect width={BOX_WIDTH} height={box.height} rx={4} fill="var(--raised)" stroke="var(--line)" />
              <text x={8} y={16} fill="var(--ink)" fontWeight={700} fontSize={13}>
                {table.table.name}
              </text>
              {table.columns.slice(0, 12).map((column, index) => (
                <text key={column.name} x={8} y={HEADER + 14 + index * ROW} fill="var(--muted)" fontSize={11} fontFamily="JetBrains Mono, monospace">
                  {column.primaryKey ? '* ' : ''}
                  {column.name}
                </text>
              ))}
            </g>
          );
        })}
      </svg>
      {tables.length === 0 && !error && <p className="p-3 text-sm text-muted">This schema has no tables.</p>}
    </div>
  );
}
