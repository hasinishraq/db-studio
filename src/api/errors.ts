import type { AppError, DbErrorDetail, ErrorCode, HostKeyDetail } from '../bindings';

/** A failed command, with the stable code from Rust. */
export class ApiError extends Error {
  readonly code: ErrorCode;
  readonly detail: DbErrorDetail | null;
  readonly hostKey: HostKeyDetail | null;

  constructor(error: AppError) {
    super(error.message);
    this.name = 'ApiError';
    this.code = error.code;
    this.detail = error.detail;
    this.hostKey = error.hostKey ?? null;
  }
}

type Outcome<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };

/** Turns the generated result shape into a value or a thrown {@link ApiError}. */
export async function unwrap<T>(call: Promise<Outcome<T>>): Promise<T> {
  const result = await call;
  if (result.status === 'error') throw new ApiError(result.error);
  return result.data;
}

/** A message for display, whatever was thrown. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
