import { useState } from 'react';
import type { Environment } from '../../bindings';
import type { QueryPart } from '../../lib/chat';
import { Guards, RiskIcon, useGuards } from '../modals/ConfirmDialog';
import { Button } from '../ui';

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
    <div className="my-2 rounded-lg border border-line bg-panel p-3">
      <p className="flex items-center gap-2 font-bold">
        <RiskIcon highRisk={highRisk} />
        {highRisk ? 'This cannot be undone' : 'The assistant wants to run this'}
      </p>
      <p className="text-sm text-muted">
        On {connectionName} ({environment}). {query.reason}
      </p>
      <pre className="my-2 max-h-48 overflow-auto rounded bg-ground p-2 font-mono text-sm">{query.sql}</pre>
      {highRisk && (
        <ul className="mb-2 list-disc pl-5 text-sm">
          {query.riskReasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
      )}
      <Guards guards={guards} highRisk={highRisk} production={production} connectionName={connectionName} />
      <div className="mt-2 flex gap-2">
        <Button tone={highRisk ? 'danger' : 'change'} disabled={!guards.ready || decided} onClick={() => decide(true)}>
          Approve
        </Button>
        <Button disabled={decided} onClick={() => decide(false)}>
          Decline
        </Button>
      </div>
    </div>
  );
}
