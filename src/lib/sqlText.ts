import type { Span } from '../bindings';

/** Rust reports statement positions in bytes, and JavaScript strings count UTF-16 units. */
export function spanText(sql: string, span: Span): string {
  return new TextDecoder().decode(new TextEncoder().encode(sql).slice(span.start, span.end));
}

/** The character offset of a byte offset. */
export function charOffset(text: string, bytes: number): number {
  return new TextDecoder().decode(new TextEncoder().encode(text).slice(0, bytes)).length;
}

/** "1 row" or "5 rows". */
export function rowCount(count: number): string {
  return `${count.toLocaleString()} ${count === 1 ? 'row' : 'rows'}`;
}
