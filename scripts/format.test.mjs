import assert from 'node:assert/strict';
import { test } from 'node:test';
import { actionLabel } from '../src/lib/activity.ts';
import { dateTime, shortPath } from '../src/lib/format.ts';
import { directoryName, workspaceAvatar } from '../src/lib/workspaces.ts';

test('home directory prefixes are shown as ~ on macOS and Linux', () => {
  assert.equal(shortPath('/Users/someone/Projects/cloudmcp'), '~/Projects/cloudmcp');
  assert.equal(shortPath('/home/someone/work'), '~/work');
  assert.equal(shortPath('/Users/someone'), '~');
  assert.equal(shortPath('~/Projects/cloudmcp'), '~/Projects/cloudmcp');
  assert.equal(shortPath('/opt/homebrew/bin/codex'), '/opt/homebrew/bin/codex');
  assert.equal(shortPath('/var/Users/someone/data'), '/var/Users/someone/data');
});

test('timestamps are formatted as month/day and 24-hour time', () => {
  assert.match(dateTime('2026-11-18T14:37:00+08:00'), /^\d{2}\/\d{2} \d{2}:\d{2}$/u);
  const local = new Date(2026, 10, 18, 9, 5);
  assert.equal(dateTime(local.toISOString()), '11/18 09:05');
  assert.throws(() => dateTime('not a date'), RangeError);
});

test('workspace avatars are stable, case-insensitive and use the first character', () => {
  assert.deepEqual(workspaceAvatar('cloudmcp'), workspaceAvatar('  CloudMCP '));
  assert.equal(workspaceAvatar('cloudmcp').initial, 'C');
  assert.equal(workspaceAvatar('文档站点').initial, '文');
  assert.equal(workspaceAvatar('𠀀项目').initial, '𠀀');
  assert.equal(workspaceAvatar('   ').initial, '?');
  assert.match(workspaceAvatar('docs').color, /^#[0-9a-f]{6}$/u);
});

test('a picked folder suggests its last path segment as the directory name', () => {
  assert.equal(directoryName('/Users/someone/Projects/cloudmcp'), 'cloudmcp');
  assert.equal(directoryName('/Users/someone/Projects/docs site/'), 'docs site');
  assert.equal(directoryName('/'), 'workspace');
});

test('audit actions use their Chinese names and unknown actions stay readable', () => {
  assert.equal(actionLabel('scope.add'), '添加目录');
  assert.equal(actionLabel('codex.exec'), '调用本机 Codex');
  assert.equal(actionLabel('execution.review'), '记录验收结果');
  assert.equal(actionLabel('server.error'), 'server.error');
});

test('elapsed time is shown in seconds, minutes, then hours', async () => {
  const { elapsed } = await import('../src/lib/format.ts');
  const start = '2026-11-20T10:00:00Z';
  assert.equal(elapsed(start, '2026-11-20T10:00:42Z'), '42 秒');
  assert.equal(elapsed(start, '2026-11-20T10:06:10Z'), '6 分钟');
  assert.equal(elapsed(start, '2026-11-20T12:00:00Z'), '2 小时');
  assert.equal(elapsed(start, '2026-11-20T13:05:00Z'), '3 小时 5 分钟');
  assert.equal(elapsed('not a date', null), '');
});

test('audit entries match a workspace by name or by a path inside it', async () => {
  const { auditMatches } = await import('../src/lib/activity.ts');
  const scope = { name: 'cloudmcp', path: '/Users/me/Software/cloudmcp' };
  assert.equal(auditMatches('cloudmcp', scope), true);
  assert.equal(auditMatches('/Users/me/Software/cloudmcp', scope), true);
  assert.equal(auditMatches('/Users/me/Software/cloudmcp/src/App.tsx', scope), true);
  assert.equal(auditMatches('/Users/me/Software/cloudmcp-old/a', scope), false);
  assert.equal(auditMatches('apphub', scope), false);
  assert.equal(auditMatches('', scope), false);
});
