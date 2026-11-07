import { useState } from 'react';
import * as AlertDialog from '@radix-ui/react-alert-dialog';
import { clsx } from 'clsx';
import type { ConfirmRequest } from '../../api/run';
import type { StatementVerdict } from '../../bindings';
import { Button, Input } from '../ui';

/** Friendly names for statement categories. */
const KIND: Record<StatementVerdict['kind'], string> = {
  Select: 'Read',
  Show: 'Read',
  Explain: 'Explain',
  Insert: 'Insert',
  Update: 'Update',
  Delete: 'Delete',
  Merge: 'Merge',
  Ddl: 'Structure change',
  Dcl: 'Permission change',
  Tcl: 'Transaction control',
  Other: 'Other statement',
};

/** What must be done before Run unlocks. Shared with the AI review card. */
export function useGuards(highRisk: boolean, production: boolean, connectionName: string) {
  const [understood, setUnderstood] = useState(false);
  const [typed, setTyped] = useState('');
  const ready = (!highRisk || understood) && (!production || typed === connectionName);
  return { ready, understood, setUnderstood, typed, setTyped };
}

export function Guards({
  guards,
  highRisk,
  production,
  connectionName,
}: {
  guards: ReturnType<typeof useGuards>;
  highRisk: boolean;
  production: boolean;
  connectionName: string;
}) {
  return (
    <div className="space-y-2">
      {highRisk && (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={guards.understood}
            onChange={(event) => guards.setUnderstood(event.target.checked)}
          />
          I understand this cannot be undone
        </label>
      )}
      {production && (
        <label className="block text-sm">
          <span className="mb-1 block">
            This is a production database. Type <strong className="font-mono">{connectionName}</strong> to run
            it.
          </span>
          <Input
            aria-label="Connection name"
            value={guards.typed}
            onChange={(event) => guards.setTyped(event.target.value)}
          />
        </label>
      )}
    </div>
  );
}

/** The review before a write. Its colour says how much is at stake. */
export function ConfirmDialog({
  request,
  onSettle,
}: {
  request: ConfirmRequest;
  onSettle: (confirmed: boolean) => void;
}) {
  const writes = request.verdicts.filter((verdict) => !verdict.readOnly);
  const reasons = writes.flatMap((verdict) => verdict.riskReasons);
  const highRisk = reasons.length > 0;
  const production = request.environment === 'Production';
  const guards = useGuards(highRisk, production, request.connectionName);
  const statements = writes.map((verdict) => request.sql.slice(verdict.span.start, verdict.span.end));

  return (
    <AlertDialog.Root open>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="fixed inset-0 z-40 bg-black/60" />
        <AlertDialog.Content
          className={clsx(
            'fixed left-1/2 top-1/2 z-50 max-h-[85vh] w-[92vw] max-w-2xl -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-md border-l-8 bg-panel p-4',
            highRisk ? 'border-danger' : 'border-change',
          )}
        >
          <AlertDialog.Title className="text-xl font-bold">
            {highRisk ? 'This cannot be undone' : 'Review this change'}
          </AlertDialog.Title>
          <AlertDialog.Description className="mb-3 text-sm text-muted">
            It will run on {request.connectionName} ({request.environment}).
          </AlertDialog.Description>

          <ul className="mb-3 space-y-2">
            {writes.map((verdict, index) => (
              <li key={verdict.span.start}>
                <span className="text-sm font-bold">{KIND[verdict.kind]}</span>
                <pre className="mt-1 max-h-48 overflow-auto rounded bg-ground p-2 font-mono text-sm">
                  {statements[index]}
                </pre>
              </li>
            ))}
          </ul>

          {reasons.length > 0 && (
            <ul className="mb-3 list-disc pl-5 text-sm">
              {reasons.map((reason) => (
                <li key={reason}>{reason}</li>
              ))}
            </ul>
          )}
          {request.warnings.map((warning) => (
            <p key={warning} className="mb-2 border-l-4 border-change pl-2 text-sm">
              {warning}
            </p>
          ))}

          <Guards guards={guards} highRisk={highRisk} production={production} connectionName={request.connectionName} />

          <div className="mt-4 flex justify-end gap-2">
            <AlertDialog.Cancel asChild>
              <Button onClick={() => onSettle(false)}>Don't run</Button>
            </AlertDialog.Cancel>
            <Button tone={highRisk ? 'danger' : 'change'} disabled={!guards.ready} onClick={() => onSettle(true)}>
              Run on {request.connectionName}
            </Button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
