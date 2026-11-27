import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { desktop } from '../lib/desktop';

type UpdateState =
  | { status: 'idle' | 'checking' | 'installing' | 'installed' }
  | { status: 'available'; update: Update };

/** 后台定期检查更新的间隔：启动时一次，之后每 6 小时一次。 */
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export function useAppUpdate(onOpen?: () => void) {
  const [state, setState] = useState<UpdateState>({ status: 'idle' });
  const busy = useRef(false);
  const announced = useRef<string | null>(null);
  const openRef = useRef(onOpen);
  openRef.current = onOpen;
  const stateRef = useRef(state);
  stateRef.current = state;

  const checkForUpdate = useCallback(async (manual = true) => {
    if (!desktop || busy.current) return;
    // 后台检查不覆盖已发现或正在安装的更新。
    if (!manual && stateRef.current.status !== 'idle') return;
    busy.current = true;
    setState({ status: 'checking' });
    try {
      const update = await check({ timeout: 15_000 });
      if (update) {
        setState({ status: 'available', update });
        if (!manual && announced.current !== update.version) {
          announced.current = update.version;
          toast.info(`CloudMCP v${update.version} 已发布`, {
            description: '安装后应用会重启并停止正在运行的执行。',
            duration: Infinity,
            action: openRef.current ? { label: '查看更新', onClick: () => openRef.current?.() } : undefined,
          });
        }
        return;
      }
      if (manual) toast.success('CloudMCP 已是最新版本');
    } catch (error) {
      if (manual) toast.error('检查更新失败', { description: String(error) });
    } finally {
      busy.current = false;
    }
    setState({ status: 'idle' });
  }, []);

  useEffect(() => {
    if (!import.meta.env.PROD) return;
    void checkForUpdate(false);
    const interval = window.setInterval(() => { void checkForUpdate(false); }, CHECK_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [checkForUpdate]);

  const installUpdate = async () => {
    if (state.status !== 'available' || busy.current) return;
    const { update } = state;
    busy.current = true;
    setState({ status: 'installing' });
    try {
      await update.downloadAndInstall(undefined, { timeout: 180_000 });
    } catch (error) {
      setState(state);
      toast.error('安装更新失败', { description: String(error) });
      return;
    } finally {
      busy.current = false;
    }

    setState({ status: 'installed' });
    void update.close().catch(() => {});
    try {
      await relaunch();
    } catch (error) {
      toast.error('更新已安装，请手动重启 CloudMCP', { description: String(error) });
    }
  };

  return { state, checkForUpdate, installUpdate };
}

export type AppUpdateController = ReturnType<typeof useAppUpdate>;
