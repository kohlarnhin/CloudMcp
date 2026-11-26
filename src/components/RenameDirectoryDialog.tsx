import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { SnapshotAction } from '../hooks/useSnapshot';
import type { Scope } from '../types';
import Button from './ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';
import { Input } from './ui/input';
import { Label } from './ui/label';

export function RenameDirectoryDialog({ scope, action, onClose }: {
  scope: Scope;
  action: SnapshotAction;
  onClose: () => void;
}) {
  const [name, setName] = useState(scope.name);
  const [saving, setSaving] = useState(false);
  return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
    <DialogContent hideClose={saving} data-directory-dialog="rename">
      <DialogHeader>
        <DialogTitle>修改对外目录名称</DialogTitle>
        <DialogDescription>执行记录继续关联此目录；远端随后使用新名称查询。名称需在本机唯一。</DialogDescription>
      </DialogHeader>
      <form className="space-y-4" onSubmit={event => {
        event.preventDefault();
        if (saving) return;
        setSaving(true);
        void action('rename_scope', { id: scope.id, name }).then(saved => { if (saved) onClose(); }).finally(() => setSaving(false));
      }}>
        <div className="h1-group space-y-2 p-3">
          <Label htmlFor="workspace-name">目录名称</Label>
          <Input id="workspace-name" autoFocus required maxLength={80} value={name} disabled={saving} onChange={event => setName(event.target.value)} />
        </div>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button>
          <Button type="submit" disabled={saving}>{saving && <Loader2 className="animate-spin" />}{saving ? '保存中…' : '保存名称'}</Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>;
}
