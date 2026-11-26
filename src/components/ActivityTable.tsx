import { Activity } from 'lucide-react';
import { actionLabel } from '../lib/activity';
import { dateTime, shortPath } from '../lib/format';
import type { Audit } from '../types';
import { Empty, EmptyDescription, EmptyTitle } from './ui/empty';

export function ActivityTable({ audit }: { audit: Audit[] }) {
  if (!audit.length) return <Empty className="flex-1" data-activity-empty>
    <Activity className="size-8 text-muted-foreground" aria-hidden="true" />
    <EmptyTitle>暂时没有访问记录</EmptyTitle>
    <EmptyDescription>连接与工具调用的记录会显示在这里。</EmptyDescription>
  </Empty>;

  return <div className="h1-group h1-table-group">
    <table className="h1-table" aria-label="访问记录" data-activity>
      <thead>
        <tr>
          <th className="w-[106px]">时间</th>
          <th className="w-[27%]">操作</th>
          <th>目标</th>
          <th className="w-[70px]">结果</th>
        </tr>
      </thead>
      <tbody>
        {audit.map(item => {
          const label = actionLabel(item.action);
          const target = item.target ? shortPath(item.target) : '—';
          return <tr key={item.id} data-audit={item.id}>
            <td className="h1-table-mono">{dateTime(item.time)}</td>
            <td className="dash-truncate" title={label}>{label}</td>
            <td className="h1-table-mono dash-truncate select-text" title={target}>{target}</td>
            <td>
              <span className="dash-state" data-tone={item.success ? 'idle' : 'error'}>
                <span className="h1-status-dot" data-tone={item.success ? 'running' : 'error'} aria-hidden="true" />{item.success ? '成功' : '失败'}
              </span>
            </td>
          </tr>;
        })}
      </tbody>
    </table>
  </div>;
}
