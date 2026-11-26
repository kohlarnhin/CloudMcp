import type { CSSProperties } from 'react';
import { workspaceAvatar } from '../lib/workspaces';
import { cn } from '../lib/utils';

export function WorkspaceAvatar({ name, className }: { name: string; className?: string }) {
  const avatar = workspaceAvatar(name);
  return <span className={cn('workspace-avatar grid shrink-0 place-items-center font-semibold', className)}
    style={{ '--avatar-ink': avatar.color } as CSSProperties} aria-hidden="true">
    {avatar.initial}
  </span>;
}
