import { useEffect, useState } from 'react';
import { native } from '../lib/desktop';
import type { Execution } from '../types';

/** Poll one execution's full record (prompt, output tails) every two seconds while it is shown. */
export function useExecutionDetail(workspace: string | undefined, runId: string | undefined) {
  const [detail, setDetail] = useState<Execution | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDetail(null);
    setError(null);
    if (!workspace || !runId) return;
    let disposed = false;
    const load = async () => {
      try {
        const run = await native<Execution>('execution_detail', { workspace, runId });
        if (!disposed) {
          setDetail(run);
          setError(null);
        }
      } catch (reason) {
        if (!disposed) setError(String(reason));
      }
    };
    void load();
    const interval = window.setInterval(() => { void load(); }, 2000);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [workspace, runId]);

  return { detail, error, setDetail };
}
