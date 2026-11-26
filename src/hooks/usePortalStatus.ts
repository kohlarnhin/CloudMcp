import { useEffect, useState } from 'react';
import { native } from '../lib/desktop';
import { PORTAL_POLL_MS } from '../lib/portal';
import type { PortalStatus } from '../types';

/** 只读轮询 Codex Portal 额度：挂载时读取一次，之后每 30 秒一次；从不触发切换。 */
export function usePortalStatus(enabled = true) {
  const [status, setStatus] = useState<PortalStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    const load = async () => {
      try {
        const next = await native<PortalStatus>('portal_status');
        if (disposed) return;
        setStatus(next);
        setError(null);
      } catch (reason) {
        if (disposed) return;
        setStatus(null);
        setError(String(reason));
      } finally {
        if (!disposed) setLoaded(true);
      }
    };
    void load();
    const interval = window.setInterval(() => { void load(); }, PORTAL_POLL_MS);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [enabled]);

  return { status, error, loaded };
}
