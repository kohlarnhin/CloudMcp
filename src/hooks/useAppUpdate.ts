import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { desktop } from '../lib/desktop';

type UpdateState =
  | { status: 'idle' | 'checking' | 'installing' | 'installed' }
  | { status: 'available'; update: Update };

export function useAppUpdate() {
  const [state, setState] = useState<UpdateState>({ status: 'idle' });
  const busy = useRef(false);

  const checkForUpdate = useCallback(async (manual = true) => {
    if (!desktop || busy.current) return;
    busy.current = true;
    setState({ status: 'checking' });
    try {
      const update = await check({ timeout: 15_000 });
      if (update) {
        setState({ status: 'available', update });
        if (!manual) toast.info(`CloudMCP v${update.version} 已发布`, { description: '打开「设置 → 关于」安装更新。' });
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
    if (import.meta.env.PROD) void checkForUpdate(false);
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
