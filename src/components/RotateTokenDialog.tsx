import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { native } from '../lib/desktop';
import Button from './ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';

export function RotateTokenDialog({ onClose }: { onClose: () => void }) {
  const [rotating, setRotating] = useState(false);
  const rotate = async () => {
    setRotating(true);
    try {
      await native('rotate_token');
      toast.success('访问令牌已更新，请同步 Hark 的连接配置');
      onClose();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setRotating(false);
    }
  };
  return <Dialog open onOpenChange={open => { if (!open && !rotating) onClose(); }}>
    <DialogContent hideClose={rotating} data-rotate-token onOpenAutoFocus={event => {
      event.preventDefault();
      document.querySelector<HTMLButtonElement>('[data-rotate-cancel]')?.focus();
    }}>
      <DialogHeader>
        <DialogTitle className="pr-8">更新远程访问令牌？</DialogTitle>
        <DialogDescription className="leading-relaxed">旧令牌将立即失效，本机运行的命令会停止。请将新令牌更新到客户端。</DialogDescription>
      </DialogHeader>
      <div className="flex justify-end gap-2">
        <Button variant="outline" disabled={rotating} onClick={onClose} data-rotate-cancel>取消</Button>
        <Button disabled={rotating} onClick={() => { void rotate(); }} data-rotate-confirm>{rotating && <Loader2 className="animate-spin" />}更新令牌</Button>
      </div>
    </DialogContent>
  </Dialog>;
}
