import { ArrowUpRight, ListChecks } from 'lucide-react';
import { dateTime } from '../lib/format';
import { executionSummary, kindNames, sessionLabel } from '../lib/executions';
import type { Execution } from '../types';
import { ExecutionState, VerificationBadge } from './ExecutionState';
import Button from './ui/button';
import { Empty, EmptyDescription, EmptyTitle } from './ui/empty';

export function ExecutionTable({ executions, onOpen }: {
  executions: Execution[];
  onOpen: (run: Execution) => void;
}) {
  if (!executions.length) return <Empty className="flex-1" data-executions-empty>
    <ListChecks className="size-8 text-muted-foreground" aria-hidden="true" />
    <EmptyTitle>暂时没有执行记录</EmptyTitle>
    <EmptyDescription>远端 Agent 启动任务后，记录会显示在这里。</EmptyDescription>
  </Empty>;

  return <div className="h1-group h1-table-group">
    <table className="h1-table" aria-label="执行记录" data-executions>
      <thead>
        <tr>
          <th>执行</th>
          <th className="w-[96px]">执行状态</th>
          <th className="w-[77px]">验收</th>
          <th className="w-[97px]">开始时间</th>
          <th className="w-[48px]"><span className="sr-only">查看</span></th>
        </tr>
      </thead>
      <tbody>
        {executions.map(item => {
          const summary = executionSummary(item);
          return <tr key={item.run_id} data-execution={item.run_id} className="cursor-pointer" onClick={() => onOpen(item)}>
            <td>
              <div className="flex min-w-0 items-center gap-2">
                <span className="dash-truncate font-medium" title={summary}>{summary}</span>
                <span className="dash-kind">{kindNames[item.kind]}</span>
              </div>
              <p className="dash-truncate mt-0.5 text-meta text-muted-foreground" title={`${item.workspace} · ${item.session_id ?? item.run_id}`}>
                {item.workspace} · <code>{sessionLabel(item)}</code>
              </p>
            </td>
            <td><ExecutionState status={item.status} /></td>
            <td><VerificationBadge verification={item.verification} /></td>
            <td className="h1-table-mono">{dateTime(item.created_at)}</td>
            <td className="execution-open-cell">
              <Button type="button" variant="ghost" size="icon-sm" aria-label={`查看执行 ${summary}`} title="查看执行" data-execution-open={item.run_id}
                onClick={event => { event.stopPropagation(); onOpen(item); }}>
                <ArrowUpRight aria-hidden="true" />
              </Button>
            </td>
          </tr>;
        })}
      </tbody>
    </table>
  </div>;
}
