import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  activeFilter, executionStatuses, executionSummary, filterExecutions, handoff, isActive, kindNames, runningCount,
  sessionLabel, sessionText, verifications, workspaceCounts,
} from '../src/lib/executions.ts';

const run = (extra = {}) => ({
  run_id: 'ac9da718-5d31-4d7d-a7e3-4fcfa209f9c3', workspace: 'cloudmcp', request_id: 'request-1', kind: 'shell',
  status: 'succeeded', session_id: null, summary: '', request_brief: '', ...extra,
});
const scopes = [{ id: 'scope-a', name: 'cloudmcp' }, { id: 'scope-b', name: '文档站点' }];

test('every execution status and verification has a label for the list and the detail dialog', () => {
  const statuses = ['starting', 'running', 'succeeded', 'failed', 'cancelled', 'timed_out', 'unknown', 'interrupted'];
  assert.deepEqual(Object.keys(executionStatuses).sort(), [...statuses].sort());
  for (const status of statuses) assert.ok(executionStatuses[status].label);
  assert.deepEqual(statuses.map(status => executionStatuses[status].tone),
    ['starting', 'running', 'idle', 'error', 'idle', 'error', 'warning', 'idle']);
  assert.deepEqual(Object.keys(verifications), ['unverified', 'completed', 'needs_work']);
  assert.deepEqual(Object.values(verifications).map(item => item.variant), ['outline', 'success', 'warning']);
  assert.deepEqual(kindNames, { codex: 'Codex', pi: 'Pi', shell: '命令' });
});

test('unknown executions stay stoppable but only starting and running count as running', () => {
  assert.deepEqual(['starting', 'running', 'unknown'].map(status => isActive({ status })), [true, true, true]);
  assert.deepEqual(['succeeded', 'failed', 'cancelled', 'timed_out', 'interrupted'].map(status => isActive({ status })),
    [false, false, false, false, false]);
  assert.equal(runningCount([{ status: 'starting' }, { status: 'running' }, { status: 'unknown' }, { status: 'failed' }]), 2);
  assert.equal(runningCount([]), 0);
});

test('directory tabs count executions by workspace name and filter by scope id', () => {
  const executions = [run(), run({ workspace: '文档站点' }), run({ run_id: 'second' }), run({ workspace: '已撤销目录' })];
  const counts = workspaceCounts(executions);
  assert.equal(counts.get('cloudmcp'), 2);
  assert.equal(counts.get('文档站点'), 1);
  assert.equal(counts.get('missing'), undefined);
  assert.equal(filterExecutions(executions, scopes, 'all').length, 4);
  assert.deepEqual(filterExecutions(executions, scopes, 'scope-a').map(item => item.run_id), [executions[0].run_id, 'second']);
  assert.deepEqual(filterExecutions(executions, scopes, 'scope-b').map(item => item.workspace), ['文档站点']);
  assert.deepEqual(filterExecutions(executions, scopes, 'revoked'), []);
  assert.notEqual(filterExecutions(executions, scopes, 'all'), executions);
});

test('a filter for a revoked directory falls back to all directories', () => {
  assert.equal(activeFilter('scope-b', scopes), 'scope-b');
  assert.equal(activeFilter('revoked', scopes), 'all');
  assert.equal(activeFilter('all', scopes), 'all');
  assert.equal(activeFilter('scope-a', []), 'all');
});

test('list rows prefer the summary, then the request brief, then the request id', () => {
  assert.equal(executionSummary(run({ summary: '已完成', request_brief: '整理表单' })), '已完成');
  assert.equal(executionSummary(run({ request_brief: '整理表单' })), '整理表单');
  assert.equal(executionSummary(run()), 'request-1');
});

test('session labels show the Codex session when registered and the run id otherwise', () => {
  assert.equal(sessionLabel(run({ session_id: '019a9a38-715e-721b-92c0-d91df4ec103a' })), 'session 019a9a38-715e…');
  assert.equal(sessionLabel(run()), 'ac9da718-5d31');
  assert.equal(sessionText(run()), '普通命令');
  assert.equal(sessionText(run({ kind: 'codex', session_id: 'abc' })), 'abc');
  assert.equal(sessionText(run({ kind: 'codex', status: 'starting' })), '等待会话创建事件');
  assert.equal(sessionText(run({ kind: 'codex', status: 'failed' })), '未创建会话');
});

test('handoff notes name the directory, the run and the exact session to resume', () => {
  const codex = handoff(run({ kind: 'codex', session_id: '019a9a38-715e-721b-92c0-d91df4ec103a' }));
  assert.match(codex, /授权目录「cloudmcp」/u);
  assert.match(codex, /执行 ac9da718-5d31-4d7d-a7e3-4fcfa209f9c3 的 execution_get/u);
  assert.match(codex, /原 Codex 会话 ID 为 019a9a38-715e-721b-92c0-d91df4ec103a。/u);
  assert.match(codex, /新的 request_id/u);
  assert.match(handoff(run({ kind: 'codex' })), /会话 ID 尚未登记，请先核实运行状态。/u);
  const shell = handoff(run());
  assert.match(shell, /这是普通命令执行；后续命令请使用新的 request_id。/u);
  assert.doesNotMatch(shell, /session_id/u);
  assert.match(shell, /不要重复启动。$/u);
});

test('workspace attention prefers running, then the newest result', async () => {
  const { workspaceAttention, focusExecution } = await import('../src/lib/executions.ts');
  assert.equal(workspaceAttention([]), undefined);
  assert.equal(workspaceAttention([{ status: 'succeeded', verification: 'completed' }, { status: 'running', verification: 'unverified' }]), 'running');
  assert.equal(workspaceAttention([{ status: 'failed', verification: 'unverified' }]), 'error');
  assert.equal(workspaceAttention([{ status: 'succeeded', verification: 'unverified' }]), 'warning');
  assert.equal(workspaceAttention([{ status: 'succeeded', verification: 'completed' }]), undefined);
  assert.equal(workspaceAttention([{ status: 'cancelled', verification: 'unverified' }]), undefined);
  const runs = [{ id: 1, status: 'succeeded' }, { id: 2, status: 'running' }];
  assert.equal(focusExecution(runs)?.id, 2);
  assert.equal(focusExecution([{ id: 3, status: 'failed' }])?.id, 3);
});
