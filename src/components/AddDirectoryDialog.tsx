import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { SnapshotAction } from '../hooks/useSnapshot';
import { shortPath } from '../lib/format';
import { directoryName } from '../lib/workspaces';
import Button from './ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';
import { Input } from './ui/input';
import { Label } from './ui/label';

export function AddDirectoryDialog({ path, action, onClose }: {
  path: string;
  action: SnapshotAction;
  onClose: () => void;
}) {
  const [name, setName] = useState(() => directoryName(path));
  const [saving, setSaving] = useState(false);
  return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
    <DialogContent hideClose={saving} data-directory-dialog="add">
      <DialogHeader>
        <DialogTitle>添加授权目录</DialogTitle>
        <DialogDescription className="break-all font-mono text-xs">{shortPath(path)}</DialogDescription>
      </DialogHeader>
      <form className="space-y-4" onSubmit={event => {
        event.preventDefault();
        if (saving) return;
        setSaving(true);
        void action('add_scope', { path, name }).then(saved => { if (saved) onClose(); }).finally(() => setSaving(false));
      }}>
        <div className="h1-group space-y-2 p-3">
          <Label htmlFor="directory-name">对外目录名称</Label>
          <Input id="directory-name" autoFocus required maxLength={80} value={name} disabled={saving}
            aria-describedby="directory-name-help" onChange={event => setName(event.target.value)} />
          <p id="directory-name-help" className="text-xs leading-relaxed text-muted-foreground">远端 Agent 用此唯一名称查询文件、执行记录和 Codex 会话。可为同名文件夹设置不同名称。</p>
        </div>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button>
          <Button type="submit" disabled={saving}>{saving && <Loader2 className="animate-spin" />}{saving ? '添加中…' : '授权此目录'}</Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>;
}
