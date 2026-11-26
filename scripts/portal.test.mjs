import assert from 'node:assert/strict';
import { test } from 'node:test';
import { clampPercent, portalDetail, portalTone, quotaTitle, quotaTone, resetIn } from '../src/lib/portal.ts';

const now = Date.parse('2026-11-20T15:00:00Z');
const at = minutes => new Date(now + minutes * 60_000).toISOString();
const account = (name, usable = true) => ({ name, usable, shortRemainingPercent: 42, shortResetsAt: at(125), longRemainingPercent: 71, longResetsAt: at(60 * 72) });

test('quota tone and percent clamping', () => {
  assert.equal(quotaTone(0), 'empty');
  assert.equal(quotaTone(19), 'low');
  assert.equal(quotaTone(20), 'ok');
  assert.equal(clampPercent(120), 100);
  assert.equal(clampPercent(-3), 0);
  assert.equal(clampPercent(Number.NaN), 0);
});

test('reset countdown', () => {
  assert.equal(resetIn(null, now), '');
  assert.equal(resetIn('bad', now), '');
  assert.equal(resetIn(at(-1), now), '即将');
  assert.equal(resetIn(at(3), now), '3 分钟后');
  assert.equal(resetIn(at(120), now), '2 小时后');
  assert.equal(resetIn(at(125), now), '2 小时 5 分钟后');
  assert.equal(resetIn(at(60 * 72), now), '3 天后');
  assert.equal(resetIn('2026-11-20T23:30:00+08:00', now), '30 分钟后');
});

test('portal detail and tone', () => {
  assert.equal(portalDetail({ current: account('a'), next: account('b'), allExhausted: false, earliestRecoveryAt: null }, now), '当前 a · 接替 b');
  assert.equal(portalDetail({ current: account('a', false), next: null, allExhausted: false, earliestRecoveryAt: null }, now), '当前 a · 额度已用完');
  assert.equal(portalDetail({ current: account('a', false), next: null, allExhausted: true, earliestRecoveryAt: at(45) }, now), '全部账号额度已用完 · 45 分钟后恢复');
  assert.equal(portalDetail({ current: null, next: null, allExhausted: false, earliestRecoveryAt: null }, now), '未选择当前账号');
  assert.equal(portalTone({ current: account('a'), next: null, allExhausted: false, earliestRecoveryAt: null }), 'running');
  assert.equal(portalTone({ current: account('a', false), next: null, allExhausted: false, earliestRecoveryAt: null }), 'error');
  assert.equal(portalTone({ current: null, next: null, allExhausted: false, earliestRecoveryAt: null }), undefined);
  assert.equal(quotaTitle('5h', account('a'), 'short', now), '5h剩余 42% · 2 小时 5 分钟后重置');
});
