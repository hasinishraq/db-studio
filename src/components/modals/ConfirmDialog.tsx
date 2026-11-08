import { useState } from 'react';
import * as AlertDialog from '@radix-ui/react-alert-dialog';
import { clsx } from 'clsx';
import type { ConfirmRequest } from '../../api/run';
import type { StatementVerdict } from '../../bindings';
import { spanText } from '../../lib/sqlText';
import { Button, Callout, Input, OVERLAY, SURFACE } from '../ui';
import { CircleAlert, TriangleAlert } from 'lucide-react';

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

/** The review before a write. The icon and the Run button's colour say how much is at stake. */
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
  const statements = writes.map((verdict) => spanText(request.sql, verdict.span));

  return (
    <AlertDialog.Root open>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className={OVERLAY} />
        <AlertDialog.Content className={clsx(SURFACE, 'max-w-2xl')}>
          <AlertDialog.Title className="flex items-center gap-2 text-lg font-bold">
            <RiskIcon highRisk={highRisk} />
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
            <Callout key={warning} tone="change" className="mb-2">
              {warning}
            </Callout>
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

/** The mark beside a review's title: amber for a change, red for one that cannot be undone. */
export function RiskIcon({ highRisk }: { highRisk: boolean }) {
  const Icon = highRisk ? CircleAlert : TriangleAlert;
  return <Icon size={20} aria-hidden className={clsx('shrink-0', highRisk ? 'text-danger' : 'text-change')} />;
}
