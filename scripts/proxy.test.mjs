import assert from 'node:assert/strict';
import test from 'node:test';
import { emptyProxyServer, linesToMap, mapToLines } from '../src/lib/proxy.ts';

test('environment and header lines round-trip', () => {
  assert.deepEqual(linesToMap('A=1\n\nB = two=2\nbad\n=x', '='), { A: '1', B: 'two=2' });
  assert.deepEqual(linesToMap('Authorization: Bearer ${T}\nX-Id:7', ':'), { Authorization: 'Bearer ${T}', 'X-Id': '7' });
  assert.equal(mapToLines({ A: '1', B: '2' }, '='), 'A=1\nB=2');
  assert.equal(emptyProxyServer().transport, 'stdio');
});
