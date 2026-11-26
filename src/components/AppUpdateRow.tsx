import { Loader2 } from 'lucide-react';
import type { AppUpdateController } from '../hooks/useAppUpdate';
import { desktop } from '../lib/desktop';
import { Button } from './ui/button';
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from './ui/dialog';

export function AppUpdateRow({ controller }: { controller: AppUpdateController }) {
  const { state, checkForUpdate, installUpdate } = controller;
  const busy = state.status === 'checking' || state.status === 'installing';
  const actionLabel = state.status === 'checking' ? '正在检查' : state.status === 'installing' ? '正在安装' : '检查更新';

  return <div className="h1-list-row">
    <div className="min-w-0">
      <p className="text-sm font-medium">CloudMCP <span className="ml-1 tabular-nums text-muted-foreground">v{__APP_VERSION__}</span></p>
      <div role="status">
        {state.status === 'available' && <p className="settings-help">可升级到 v{state.update.version}</p>}
        {state.status === 'installed' && <p className="settings-help">更新已安装，重启后生效。</p>}
      </div>
    </div>
    {desktop && state.status === 'available' && <Dialog>
      <DialogTrigger asChild>
        <button type="button" className="h1-button">安装更新</button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="pr-8">更新到 CloudMCP v{state.update.version}？</DialogTitle>
          <DialogDescription className="leading-relaxed">安装完成后应用会重启，断开当前 MCP 连接并停止正在运行的执行。</DialogDescription>
        </DialogHeader>
        <div className="flex justify-end gap-2">
          <DialogClose asChild><Button variant="outline">取消</Button></DialogClose>
          <DialogClose asChild><Button onClick={() => { void installUpdate(); }}>安装并重启</Button></DialogClose>
        </div>
      </DialogContent>
    </Dialog>}
    {desktop && state.status !== 'available' && state.status !== 'installed' &&
      <button type="button" className="h1-button" disabled={busy} onClick={() => { void checkForUpdate(); }}>
        {busy && <Loader2 size={13} className="animate-spin" aria-hidden="true" />}{actionLabel}
      </button>}
  </div>;
}
