import { useMemo } from 'react';

export interface PlanNode {
  label: string;
  relation: string | null;
  cost: number | null;
  rows: number | null;
  children: PlanNode[];
}

function fromPostgres(node: Record<string, unknown>): PlanNode {
  const children = (node.Plans as Record<string, unknown>[] | undefined) ?? [];
  return {
    label: String(node['Node Type'] ?? 'Step'),
    relation: (node['Relation Name'] as string | undefined) ?? null,
    cost: (node['Total Cost'] as number | undefined) ?? null,
    rows: (node['Plan Rows'] as number | undefined) ?? null,
    children: children.map(fromPostgres),
  };
}

/** Walks MySQL's nested plan, turning each object holding table or operation facts into a node. */
function fromMySql(value: unknown, label: string): PlanNode[] {
  if (Array.isArray(value)) return value.flatMap((item) => fromMySql(item, label));
  if (typeof value !== 'object' || value === null) return [];
  const object = value as Record<string, unknown>;
  const children = Object.entries(object).flatMap(([key, child]) => fromMySql(child, key));
  const table = object.table_name as string | undefined;
  const costs = object.cost_info as Record<string, string> | undefined;
  const cost = Number(costs?.query_cost ?? costs?.read_cost ?? NaN);
  const interesting = table !== undefined || label === 'query_block';
  if (!interesting) return children;
  return [
    {
      label: table ? String(object.access_type ?? 'scan') : 'Query',
      relation: table ?? null,
      cost: Number.isNaN(cost) ? null : cost,
      rows: (object.rows_examined_per_scan as number | undefined) ?? null,
      children,
    },
  ];
}

function fromSqlite(nodes: { id: number; parent: number; detail: string }[]): PlanNode {
  const build = (parent: number): PlanNode[] =>
    nodes
      .filter((n) => n.parent === parent)
      .map((n) => ({ label: n.detail, relation: null, cost: null, rows: null, children: build(n.id) }));
  return { label: 'Query', relation: null, cost: null, rows: null, children: build(0) };
}

/** Reads any engine's JSON plan into one tree. */
export function parsePlan(text: string): PlanNode | null {
  try {
    const parsed: unknown = JSON.parse(text);
    if (Array.isArray(parsed)) {
      const first = parsed[0] as Record<string, unknown> | undefined;
      if (first?.Plan) return fromPostgres(first.Plan as Record<string, unknown>);
      return fromSqlite(parsed as { id: number; parent: number; detail: string }[]);
    }
    return fromMySql(parsed, 'root')[0] ?? null;
  } catch {
    return null;
  }
}

function maxCost(node: PlanNode): number {
  return Math.max(node.cost ?? 0, ...node.children.map(maxCost));
}

function Node({ node, top }: { node: PlanNode; top: number }) {
  // A hot spot is a real cost of change: it is what to look at first.
  const hot = top > 0 && (node.cost ?? 0) >= top * 0.6;
  return (
    <li className="my-1">
      <div
        className={`flex items-baseline gap-1.5 rounded border px-2 py-1 text-sm ${hot ? 'border-change/50 bg-change/10' : 'border-line bg-ground'}`}
      >
        {hot && <span className="text-xs font-bold text-change">Costly</span>}
        <strong>{node.label}</strong>
        {node.relation && <span className="font-mono"> {node.relation}</span>}
        <span className="ml-2 text-xs text-muted">
          {node.cost !== null && `cost ${node.cost.toLocaleString()}`}
          {node.rows !== null && `, ${node.rows.toLocaleString()} rows`}
        </span>
      </div>
      {node.children.length > 0 && (
        <ul className="ml-4">
          {node.children.map((child, index) => (
            <Node key={index} node={child} top={top} />
          ))}
        </ul>
      )}
    </li>
  );
}

export function PlanView({ text }: { text: string }) {
  const plan = useMemo(() => parsePlan(text), [text]);
  if (!plan) return <pre className="overflow-auto p-2 font-mono text-xs">{text}</pre>;
  return (
    <ul className="h-full overflow-auto p-2">
      <Node node={plan} top={maxCost(plan)} />
    </ul>
  );
}
