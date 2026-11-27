import type { Execution, ExecutionKind, ExecutionStatus, ExecutionVerification, Scope } from '../types.ts';

export type StatusTone = 'running' | 'starting' | 'warning' | 'error' | 'idle';

export const executionStatuses: Record<ExecutionStatus, { label: string; tone: StatusTone }> = {
  starting: { label: '启动中', tone: 'starting' },
  running: { label: '运行中', tone: 'running' },
  succeeded: { label: '本轮已结束', tone: 'idle' },
  failed: { label: '执行失败', tone: 'error' },
  cancelled: { label: '已停止', tone: 'idle' },
  timed_out: { label: '达到时限', tone: 'error' },
  unknown: { label: '状态待核实', tone: 'warning' },
  interrupted: { label: '已核实停止', tone: 'idle' },
};

export const verifications: Record<ExecutionVerification, { label: string; variant: 'outline' | 'success' | 'warning' }> = {
  unverified: { label: '待验收', variant: 'outline' },
  completed: { label: '验收完成', variant: 'success' },
  needs_work: { label: '需要继续', variant: 'warning' },
};

export const kindNames: Record<ExecutionKind, string> = { codex: 'Codex', pi: 'Pi', shell: '命令' };

export function isActive(run: Pick<Execution, 'status'>) {
  return ['starting', 'running', 'unknown'].includes(run.status);
}

export function runningCount(executions: readonly Pick<Execution, 'status'>[]) {
  return executions.filter(isRunning).length;
}

export function workspaceCounts(executions: readonly Pick<Execution, 'workspace'>[]) {
  const counts = new Map<string, number>();
  for (const run of executions) counts.set(run.workspace, (counts.get(run.workspace) ?? 0) + 1);
  return counts;
}

export function activeFilter(filter: string, scopes: readonly Pick<Scope, 'id'>[]) {
  return scopes.some(scope => scope.id === filter) ? filter : 'all';
}

export function filterExecutions<T extends Pick<Execution, 'workspace'>>(executions: readonly T[], scopes: readonly Pick<Scope, 'id' | 'name'>[], filter: string) {
  if (filter === 'all') return [...executions];
  const workspace = scopes.find(scope => scope.id === filter)?.name;
  return executions.filter(run => run.workspace === workspace);
}

export function executionSummary(run: Pick<Execution, 'summary' | 'request_brief' | 'request_id'>) {
  return run.summary || run.request_brief || run.request_id;
}

export function sessionLabel(run: Pick<Execution, 'session_id' | 'run_id'>) {
  return run.session_id ? `session ${run.session_id.slice(0, 13)}…` : run.run_id.slice(0, 13);
}

export function sessionText(run: Pick<Execution, 'kind' | 'session_id' | 'status'>) {
  if (run.kind === 'shell') return '普通命令';
  return run.session_id ?? (isActive(run) ? '等待会话创建事件' : '未创建会话');
}

export function handoff(run: Pick<Execution, 'kind' | 'session_id' | 'workspace' | 'run_id'>) {
  const session = run.kind !== 'shell'
    ? `${run.session_id ? `原 ${kindNames[run.kind]} 会话 ID 为 ${run.session_id}。` : '会话 ID 尚未登记，请先核实运行状态。'}需要继续时使用明确的 session_id，并为新一轮提供新的 request_id。`
    : '这是普通命令执行；后续命令请使用新的 request_id。';
  return `请通过 CloudMCP 接手授权目录「${run.workspace}」。先调用 execution_list，再检查执行 ${run.run_id} 的 execution_get。${session}运行中请等待结果；已结束请检查验收条件和测试证据。状态待核实时先核实本机进程，不要重复启动。`;
}

export function isRunning(run: Pick<Execution, 'status'>) {
  return run.status === 'starting' || run.status === 'running';
}

export type WorkspaceAttention = 'running' | 'error' | 'warning' | undefined;

/** Summarize a directory's recent executions, sorted newest first. */
export function workspaceAttention(runs: readonly Pick<Execution, 'status' | 'verification'>[]): WorkspaceAttention {
  if (runs.some(isRunning)) return 'running';
  const latest = runs[0];
  if (!latest) return undefined;
  if (latest.status === 'failed' || latest.status === 'timed_out') return 'error';
  if (latest.status === 'unknown' || latest.verification === 'needs_work') return 'warning';
  if (latest.verification === 'unverified' && latest.status === 'succeeded') return 'warning';
  return undefined;
}

/** Pick the execution a workspace overview shows output for: the newest running one, else the newest. */
export function focusExecution<T extends Pick<Execution, 'status'>>(runs: readonly T[]): T | undefined {
  return runs.find(isRunning) ?? runs[0];
}
