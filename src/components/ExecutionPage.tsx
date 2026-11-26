import { useState } from 'react';
import { ArrowLeft, ClipboardCheck, Loader2, Square } from 'lucide-react';
import { toast } from 'sonner';
import { useExecutionDetail } from '../hooks/useExecutionDetail';
import { native } from '../lib/desktop';
import { dateTime, elapsed } from '../lib/format';
import { executionSummary, handoff, isActive, isRunning, kindNames, sessionText } from '../lib/executions';
import type { Execution } from '../types';
import { CopyButton } from './CopyButton';
import { ExecutionState, VerificationBadge } from './ExecutionState';
import { OutputPanel, executionStreams } from './OutputPanel';
import { WorkspaceAvatar } from './WorkspaceAvatar';
import { Alert, AlertDescription } from './ui/alert';
import { Badge } from './ui/badge';
import { Empty, EmptyDescription, EmptyTitle } from './ui/empty';
import { SegmentedControl } from './ui/segmented-control';

type Tab = 'output' | 'request' | 'review';

function RecordText({ title, text }: { title: string; text: string }) {
  return <section className="h1-form-section">
    <h3 className="h1-group-title">{title}</h3>
    <p className="h1-group dash-text">{text}</p>
  </section>;
}

export function ExecutionPage({ workspace, runId, snapshotRun, refresh, backLabel, onBack }: {
  workspace: string;
  runId: string;
  snapshotRun?: Execution;
  refresh: () => Promise<void>;
  backLabel: string;
  onBack: () => void;
}) {
  const [tab, setTab] = useState<Tab>('output');
  const [stopping, setStopping] = useState(false);
  const { detail, error, setDetail } = useExecutionDetail(workspace, runId);
  const run = detail ?? snapshotRun;
  const title = run ? executionSummary(run) : '执行记录';

  const stop = async (target: Execution) => {
    setStopping(true);
    try {
      setDetail(await native<Execution>('cancel_execution', { workspace: target.workspace, runId: target.run_id }));
      await refresh();
      toast.success('已请求停止，正在保存执行结果');
    } catch (reason) {
      toast.error(String(reason));
    } finally {
      setStopping(false);
    }
  };

  const tabs = [
    { value: 'output' as const, label: '输出' },
    { value: 'request' as const, label: run?.kind === 'shell' ? '命令' : '要求' },
    { value: 'review' as const, label: '验收' },
  ];

  return <>
    <button type="button" className="h1-button self-start" onClick={onBack} data-execution-back>
      <ArrowLeft size={13} aria-hidden="true" />{backLabel}
    </button>
    <header className="dash-page-heading">
      <div className="min-w-0">
        <h1>{title}</h1>
        <p className="execution-workspace" title={workspace}>
          <WorkspaceAvatar name={workspace} className="[--workspace-avatar-size:20px]" />
          <span className="dash-truncate">{workspace}</span>
        </p>
      </div>
      {run && <ExecutionState status={run.status} />}
    </header>
    {error && <Alert variant="destructive"><AlertDescription className="select-text">{error}</AlertDescription></Alert>}
    {!run && !error && <p role="status" className="flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground">
      <Loader2 className="size-4 animate-spin" aria-hidden="true" />读取中…
    </p>}
    {run && <>
      <div className="h1-group dash-summary" data-execution-facts={run.run_id}>
        <div className="dash-facts">
          <span>类型 <b>{kindNames[run.kind]}</b></span>
          <span className="flex min-w-0 items-center">{run.kind === 'codex' ? '会话' : '执行'}
            <b className="dash-truncate font-mono font-medium select-text" title={run.session_id ?? run.run_id}>
              {run.kind === 'codex' ? sessionText(run) : run.run_id.slice(0, 8)}
            </b>
            {run.kind === 'codex' && run.session_id && <CopyButton text={run.session_id} label="复制" className="ml-2" />}
          </span>
          <span className="wrap-anywhere">请求 <b className="font-mono font-medium select-text">{run.request_id}</b></span>
          <span>时限 <b>{elapsed('1970-01-01T00:00:00Z', new Date(run.timeout_seconds * 1000).toISOString())}</b></span>
          <span>开始 <b>{dateTime(run.created_at)}</b></span>
          <span>{isRunning(run) ? '已运行' : '用时'} <b>{elapsed(run.created_at, isRunning(run) ? null : run.ended_at)}</b></span>
          <span className="dash-right flex items-center gap-2">
            {run.full_access && <Badge variant="outline">完整权限</Badge>}
            <VerificationBadge verification={run.verification} />
          </span>
        </div>
        {run.error && <section className="dash-alert" role="alert">
          <span className="h1-status-dot" data-tone="error" aria-hidden="true" />
          <p className="select-text"><strong>{run.error}</strong></p>
        </section>}
      </div>

      <div className="execution-controls">
        <SegmentedControl className="dash-segments" label="执行页面" options={tabs} value={tab} onChange={setTab} />
        <div className="flex flex-wrap items-center gap-2">
          <CopyButton button label="复制接手说明" text={handoff(run)} />
          {isActive(run) && <button type="button" className="h1-button h1-button-primary" disabled={stopping || run.cancel_requested}
            onClick={() => { void stop(run); }} data-execution-stop>
            {stopping ? <Loader2 size={13} className="animate-spin" aria-hidden="true" /> : <Square size={10} fill="currentColor" aria-hidden="true" />}
            {run.cancel_requested ? '正在停止' : '停止'}
          </button>}
        </div>
      </div>

      {tab === 'output' && <OutputPanel className="min-h-[320px]" streams={executionStreams(detail)}
        empty={detail ? (isActive(run) ? '等待输出…' : '这次执行没有输出') : '读取中…'}
        aside={<span className="dash-log-state" data-tone={isRunning(run) ? 'running' : undefined}>
          {isRunning(run) ? '跟随最新' : '最近心跳 ' + dateTime(run.heartbeat_at)}
        </span>} />}

      {tab === 'request' && <div className="flex flex-col gap-4">
        {detail && (detail.prompt || detail.command) ? <RecordText title={detail.kind === 'codex' ? '原始要求' : '执行命令'} text={detail.prompt || detail.command || ''} />
          : <RecordText title="要求摘要" text={run.request_brief || '远端没有提供要求摘要。'} />}
        {run.acceptance_criteria && <RecordText title="验收条件" text={run.acceptance_criteria} />}
      </div>}

      {tab === 'review' && (run.summary || run.verification_note || run.evidence_paths.length ? <div className="flex flex-col gap-4">
        {run.summary && <RecordText title="最近结果" text={run.summary} />}
        {run.verification_note && <RecordText title="验收说明" text={run.verification_note} />}
        {run.evidence_paths.length > 0 && <RecordText title="证据文件（目录内相对路径）" text={run.evidence_paths.join('\n')} />}
      </div> : <Empty className="flex-1">
        <ClipboardCheck className="size-8 text-muted-foreground" aria-hidden="true" />
        <EmptyTitle>还没有验收记录</EmptyTitle>
        <EmptyDescription>远端 Agent 检查结果后，通过 execution_review 记录验收结论、说明和证据文件。</EmptyDescription>
      </Empty>)}
    </>}
  </>;
}
