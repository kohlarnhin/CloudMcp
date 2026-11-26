import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { desktop, initialSnapshot, native, preview } from '../lib/desktop';
import type { Snapshot } from '../types';

export function useSnapshot() {
  const [data, setData] = useState<Snapshot>(initialSnapshot);
  const [busy, setBusy] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!desktop && !preview) return;
    try {
      setData(await native<Snapshot>('snapshot'));
      setLoadError(null);
    } catch (error) {
      setLoadError(String(error));
    }
  }, []);

  useEffect(() => {
    void refresh();
    if (!desktop && !preview) return;
    const interval = window.setInterval(() => { void refresh(); }, 2000);
    return () => window.clearInterval(interval);
  }, [refresh]);

  const action = useCallback(async (command: string, args?: Record<string, unknown>) => {
    setBusy(command);
    try {
      await native(command, args);
      await refresh();
      return true;
    } catch (error) {
      toast.error(String(error));
      return false;
    } finally {
      setBusy(null);
    }
  }, [refresh]);

  return { data, busy, loadError, refresh, action };
}

export type SnapshotAction = ReturnType<typeof useSnapshot>['action'];
