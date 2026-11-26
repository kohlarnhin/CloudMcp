import { useRef, useState } from 'react';
import { Ellipsis, FolderKey, Pencil, Plus, Trash2 } from 'lucide-react';
import { DropdownMenu } from 'radix-ui';
import type { SnapshotAction } from '../hooks/useSnapshot';
import { isRunning, workspaceCounts } from '../lib/executions';
import { shortPath } from '../lib/format';
import type { Scope, Snapshot } from '../types';
import { RenameDirectoryDialog } from './RenameDirectoryDialog';
import { RevokeDirectoryDialog } from './RevokeDirectoryDialog';
import Button from './ui/button';
import { Empty, EmptyTitle } from './ui/empty';

type DirectoryAction = 'rename' | 'revoke';

function DirectoryMenu({ name, onChoose }: {
  name: string;
  onChoose: (kind: DirectoryAction, trigger: HTMLButtonElement | null) => void;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  const openingDialog = useRef(false);
  const choose = (kind: DirectoryAction) => {
    openingDialog.current = true;
    onChoose(kind, trigger.current);
  };

  return <DropdownMenu.Root modal={false}>
    <DropdownMenu.Trigger asChild>
      <Button ref={trigger} type="button" variant="ghost" size="icon" aria-label={`管理目录 ${name}`}>
        <Ellipsis aria-hidden="true" />
      </Button>
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content className="h1-dialog directory-menu" align="end" sideOffset={4} collisionPadding={12}
        onCloseAutoFocus={event => {
          if (openingDialog.current) event.preventDefault();
          openingDialog.current = false;
        }}>
        <DropdownMenu.Item className="directory-menu-item" onSelect={() => choose('rename')}>
          <Pencil size={14} aria-hidden="true" />重命名
        </DropdownMenu.Item>
        <DropdownMenu.Item className="directory-menu-item text-destructive" onSelect={() => choose('revoke')}>
          <Trash2 size={14} aria-hidden="true" />撤销授权
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>;
}

export function DirectoriesPage({ data, action, onAdd }: { data: Snapshot; action: SnapshotAction; onAdd: () => void }) {
  const [dialog, setDialog] = useState<{ kind: DirectoryAction; scope: Scope; trigger: HTMLButtonElement | null } | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const running = workspaceCounts(data.executions.filter(isRunning));
  const closeDialog = () => {
    const trigger = dialog?.trigger;
    setDialog(null);
    window.requestAnimationFrame(() => (trigger?.isConnected ? trigger : addButton.current)?.focus());
  };
  const add = <Button ref={addButton} type="button" size="sm" aria-label="添加目录" onClick={onAdd}>
    <Plus aria-hidden="true" />添加
  </Button>;

  return <>
    <header className="dash-page-heading">
      <div>
        <h1>开放给 Agent 的目录</h1>
        <p className="dash-page-description">Agent 对这些目录有完整读写权限</p>
      </div>
      {data.scopes.length > 0 && add}
    </header>

    {data.scopes.length ? <>
      <ul className="h1-group directory-list" aria-label="授权目录">
        {data.scopes.map(scope => {
          const active = running.has(scope.name);
          return <li key={scope.id} className="directory-row" data-directory={scope.id}>
            <div className="directory-name">
              {active && <span className="h1-status-dot" data-tone="working" role="img" aria-label="有任务运行中" />}
              <span className="dash-truncate" title={scope.name}>{scope.name}</span>
            </div>
            <code className="dash-truncate" title={scope.path}>{shortPath(scope.path)}</code>
            <DirectoryMenu name={scope.name} onChoose={(kind, trigger) => setDialog({ kind, scope, trigger })} />
          </li>;
        })}
      </ul>
      <p className="directory-legend">
        <span className="h1-status-dot" data-tone="working" aria-hidden="true" />有任务运行中
      </p>
    </> : <Empty data-directories-empty>
      <FolderKey className="size-7 text-muted-foreground" aria-hidden="true" />
      <EmptyTitle>还没有开放的目录</EmptyTitle>
      {add}
    </Empty>}

    {dialog?.kind === 'rename' && <RenameDirectoryDialog scope={dialog.scope} action={action} onClose={closeDialog} />}
    {dialog?.kind === 'revoke' && <RevokeDirectoryDialog scope={dialog.scope} action={action} onClose={closeDialog} />}
  </>;
}
