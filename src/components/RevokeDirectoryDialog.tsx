import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { SnapshotAction } from '../hooks/useSnapshot';
import { shortPath } from '../lib/format';
import type { Scope } from '../types';
import Button from './ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';

export function RevokeDirectoryDialog({ scope, action, onClose }: {
  scope: Scope;
  action: SnapshotAction;
  onClose: () => void;
}) {
  const [revoking, setRevoking] = useState(false);
  const revoke = () => {
    setRevoking(true);
    void action('remove_scope', { id: scope.id }).then(removed => { if (removed) onClose(); }).finally(() => setRevoking(false));
  };
  return <Dialog open onOpenChange={open => { if (!open && !revoking) onClose(); }}>
    <DialogContent hideClose={revoking} data-directory-dialog="revoke" onOpenAutoFocus={event => {
      event.preventDefault();
      document.querySelector<HTMLButtonElement>('[data-revoke-cancel]')?.focus();
    }}>
      <DialogHeader>
        <DialogTitle className="pr-8">撤销这个目录的远程授权？</DialogTitle>
        <DialogDescription className="break-words leading-relaxed">
          {shortPath(scope.path)} 将从可访问目录中移除。正在执行的本机命令会停止，已有文件改动保留。
        </DialogDescription>
      </DialogHeader>
      <div className="flex justify-end gap-2">
        <Button variant="outline" disabled={revoking} onClick={onClose} data-revoke-cancel>保留授权</Button>
        <Button variant="destructive" disabled={revoking} onClick={revoke} data-revoke-confirm>{revoking && <Loader2 className="animate-spin" />}撤销授权</Button>
      </div>
    </DialogContent>
  </Dialog>;
}
