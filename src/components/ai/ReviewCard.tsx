import { useState } from 'react';
import type { Environment } from '../../bindings';
import type { QueryPart } from '../../lib/chat';
import { Guards, RiskIcon, useGuards } from '../modals/ConfirmDialog';
import { Button, cn } from '../ui';

/** A statement waiting for a person. It runs only if Approve is clicked. */
export function ReviewCard({
  query,
  connectionName,
  environment,
  onDecide,
}: {
  query: QueryPart;
  connectionName: string;
  environment: Environment;
  onDecide: (approved: boolean) => void;
}) {
  const highRisk = query.riskReasons.length > 0;
  const production = environment === 'Production';
  const guards = useGuards(highRisk, production, connectionName);
  const [decided, setDecided] = useState(false);
  const decide = (approved: boolean) => {
    setDecided(true);
    onDecide(approved);
  };
  return (
    <div
      className={cn(
        'my-3 overflow-hidden rounded-md border',
        highRisk ? 'border-danger/45 bg-danger/5' : 'border-change/45 bg-change/5',
      )}
    >
      <div className="flex items-start gap-2.5 border-b border-line/70 px-3 py-2.5">
        <RiskIcon highRisk={highRisk} />
        <div className="min-w-0">
          <p className="text-sm font-semibold">{highRisk ? 'This cannot be undone' : 'The assistant wants to run this'}</p>
          <p className="text-xs text-muted">
            On {connectionName} ({environment}). {query.reason}
          </p>
        </div>
      </div>
      <pre className="max-h-48 overflow-auto bg-ground px-3 py-2 font-mono text-xs leading-relaxed">{query.sql}</pre>
      <div className="space-y-2 border-t border-line/70 px-3 py-2.5">
        {highRisk && (
          <ul className="list-disc space-y-0.5 pl-5 text-sm">
            {query.riskReasons.map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        )}
        <Guards guards={guards} highRisk={highRisk} production={production} connectionName={connectionName} />
        <div className="flex gap-2 pt-1">
          <Button tone={highRisk ? 'danger' : 'change'} disabled={!guards.ready || decided} onClick={() => decide(true)}>
            Approve
          </Button>
          <Button disabled={decided} onClick={() => decide(false)}>
            Decline
          </Button>
        </div>
      </div>
    </div>
  );
}
