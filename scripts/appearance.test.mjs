import assert from 'node:assert/strict';
import { test } from 'node:test';
import { APPEARANCE_KEY, appearances, parseAppearance, resolveColorScheme } from '../src/lib/appearance.ts';

test('only light and dark override the system appearance', () => {
  assert.equal(parseAppearance('light'), 'light');
  assert.equal(parseAppearance('dark'), 'dark');
  for (const value of ['system', '', 'Dark', 'auto', null, undefined, true, 1]) assert.equal(parseAppearance(value), 'system');
});

test('the system appearance resolves through the media query', () => {
  assert.equal(resolveColorScheme('system', true), 'dark');
  assert.equal(resolveColorScheme('system', false), 'light');
  assert.equal(resolveColorScheme('light', true), 'light');
  assert.equal(resolveColorScheme('dark', false), 'dark');
});

test('the settings control offers exactly the three stored choices', () => {
  assert.deepEqual(appearances.map(item => item.value), ['system', 'light', 'dark']);
  assert.deepEqual(appearances.map(item => item.label), ['跟随系统', '浅色', '深色']);
  assert.equal(APPEARANCE_KEY, 'cloudmcp.appearance');
});
