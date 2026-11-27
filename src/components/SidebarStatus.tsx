import { tunnelTone } from '../lib/connection';
import { runningCount } from '../lib/executions';
import type { Snapshot } from '../types';

function overview(data: Snapshot) {
  if (!data.server_running) return { tone: undefined, title: '离线', detail: '本机服务未启动' };
  const tunnel = tunnelTone(data);
  if (tunnel === 'running') return { tone: 'running', title: '在线', detail: data.public_url?.replace(/^https?:\/\//, '') ?? '' };
  if (tunnel === 'starting') return { tone: 'starting', title: '连接中', detail: '正在等待隧道地址' };
  if (tunnel === 'error') return { tone: 'error', title: '隧道异常', detail: '查看服务状态' };
  return { tone: 'starting', title: '仅本机', detail: '隧道未连接' };
}

/** 侧栏底部：服务与执行模式速览，点击回到服务状态。 */
export function SidebarStatus({ data, onOpen }: { data: Snapshot; onOpen: () => void }) {
  const state = overview(data);
  const running = runningCount(data.executions);
  return <button type="button" className="sidebar-status" onClick={onOpen} data-sidebar-status>
    <span className="flex items-center gap-2">
      <span className="h1-status-dot" data-tone={state.tone} aria-hidden="true" />
      <span className="text-[13px] font-semibold">{state.title}</span>
      <span className="sidebar-status-mode">{data.settings.codex_enabled ? 'Codex 委派' : '调用方直连'}</span>
    </span>
    <span className="sidebar-status-detail dash-truncate" title={state.detail}>{state.detail}</span>
    <span className="sidebar-status-detail">
      {data.scopes.length} 个目录 · {running ? `${running} 个任务运行中` : '没有运行中的任务'}
    </span>
  </button>;
}
