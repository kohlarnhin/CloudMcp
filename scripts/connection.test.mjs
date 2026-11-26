import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  clientConfig, connectionStage, connectionStageLabels, localEndpoint, publicEndpoint, TOKEN_PLACEHOLDER, tunnelTone,
} from '../src/lib/connection.ts';

test('endpoints use the loopback port locally and the /mcp path publicly', () => {
  assert.equal(localEndpoint(8787), '127.0.0.1:8787');
  assert.equal(publicEndpoint('https://example.trycloudflare.com'), 'https://example.trycloudflare.com/mcp');
  assert.equal(publicEndpoint(null), null);
});

test('the displayed client configuration never contains the access token', () => {
  const shown = JSON.parse(clientConfig('https://example.trycloudflare.com'));
  assert.deepEqual(shown, { mcpServers: { cloudmcp: {
    type: 'http', url: 'https://example.trycloudflare.com/mcp', headers: { Authorization: 'Bearer ' + TOKEN_PLACEHOLDER },
  } } });
  assert.equal(JSON.parse(clientConfig(null)).mcpServers.cloudmcp.url, 'https://<连接隧道后获取地址>/mcp');
  assert.equal(clientConfig(null).split('\n').length, 11);
});

test('the copied client configuration carries the token only when one is passed', () => {
  const copied = JSON.parse(clientConfig('https://example.trycloudflare.com', 'secret-token'));
  assert.equal(copied.mcpServers.cloudmcp.headers.Authorization, 'Bearer secret-token');
  assert.ok(!clientConfig('https://example.trycloudflare.com', 'secret-token').includes(TOKEN_PLACEHOLDER));
});

test('the connection stage follows the public address before the local service', () => {
  assert.equal(connectionStage({ public_url: 'https://example.com', server_running: true }), 'public');
  assert.equal(connectionStage({ public_url: null, server_running: true }), 'local');
  assert.equal(connectionStage({ public_url: null, server_running: false }), 'idle');
  assert.deepEqual(connectionStageLabels, { public: '公网入口已建立', local: '本机已就绪', idle: '等待连接' });
});

test('the tunnel dot turns green only once a public address exists', () => {
  assert.equal(tunnelTone({ public_url: 'https://example.com', tunnel_running: true, tunnel_error: null }), 'running');
  assert.equal(tunnelTone({ public_url: null, tunnel_running: true, tunnel_error: null }), 'starting');
  assert.equal(tunnelTone({ public_url: null, tunnel_running: false, tunnel_error: '隧道已退出：exit status: 1' }), 'error');
  assert.equal(tunnelTone({ public_url: null, tunnel_running: false, tunnel_error: null }), undefined);
});
