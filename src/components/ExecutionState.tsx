import { Loader2 } from 'lucide-react';
import { executionStatuses, verifications } from '../lib/executions';
import type { ExecutionStatus, ExecutionVerification } from '../types';
import { Badge } from './ui/badge';

export function ExecutionState({ status }: { status: ExecutionStatus }) {
  const { label, tone } = executionStatuses[status];
  return <span className="dash-state" data-tone={tone} data-execution-status={status}>
    {status === 'starting' ? <Loader2 size={12} className="animate-spin" aria-hidden="true" />
      : <span className="h1-status-dot" data-tone={tone === 'idle' ? undefined : tone} aria-hidden="true" />}
    {label}
  </span>;
}

export function VerificationBadge({ verification }: { verification: ExecutionVerification }) {
  const { label, variant } = verifications[verification];
  return <Badge variant={variant} data-verification={verification}>{label}</Badge>;
}
