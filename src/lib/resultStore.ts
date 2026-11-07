import type { ColumnMeta, RowBatch, StatementOutcome, Value } from '../bindings';

export type StatementStatus = 'running' | 'done' | 'error' | 'cancelled';

/** One statement's streaming result. Rows stay in the chunks they arrived in. */
export class StatementResult {
  columns: ColumnMeta[] = [];
  status: StatementStatus = 'running';
  outcome: StatementOutcome | null = null;
  rowCount = 0;
  private readonly chunks: Value[][][] = [];
  private readonly starts: number[] = [];

  /** Appends a batch without copying earlier rows, so a million rows stay cheap. */
  append(batch: RowBatch): void {
    this.starts.push(this.rowCount);
    this.chunks.push(batch.rows);
    this.rowCount += batch.rows.length;
  }

  /** The row at an index, found by binary search over chunk offsets. */
  rowAt(index: number): Value[] | undefined {
    let low = 0;
    let high = this.starts.length - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const start = this.starts[middle] ?? 0;
      if (index < start) high = middle - 1;
      else if (index >= start + (this.chunks[middle]?.length ?? 0)) low = middle + 1;
      else return this.chunks[middle]?.[index - start];
    }
    return undefined;
  }
}

/** Every statement of one run, with a subscription for the grid. */
export class ResultStore {
  readonly statements = new Map<number, StatementResult>();
  private readonly listeners = new Set<() => void>();
  private frame: ReturnType<typeof setTimeout> | null = null;
  private version = 0;

  statement(index: number): StatementResult {
    let found = this.statements.get(index);
    if (!found) {
      found = new StatementResult();
      this.statements.set(index, found);
    }
    return found;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  snapshot = (): number => this.version;

  /** Coalesces updates, so a fast stream does not render once per batch. */
  notify(): void {
    if (this.frame !== null) return;
    this.frame = setTimeout(() => {
      this.frame = null;
      this.version += 1;
      this.listeners.forEach((listener) => listener());
    }, 50);
  }
}

/** The text shown for a cell. */
export function cellText(value: Value | undefined): string {
  if (!value) return '';
  switch (value.t) {
    case 'Null':
      return 'NULL';
    case 'Bool':
      return value.v ? 'true' : 'false';
    case 'Int':
    case 'Float':
      return String(value.v);
    case 'Bytes':
      return `0x${value.v.preview_hex}${value.v.len > 64 ? '…' : ''} (${value.v.len} bytes)`;
    case 'Unsupported':
      return `<${value.v}>`;
    default:
      return value.v;
  }
}
