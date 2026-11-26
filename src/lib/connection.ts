import type { Snapshot } from '../types.ts';

export const TOKEN_PLACEHOLDER = '<本机访问令牌>';

export function localEndpoint(port: number) {
  return '127.0.0.1:' + port;
}

export function publicEndpoint(publicUrl: string | null) {
  return publicUrl ? publicUrl + '/mcp' : null;
}

export function clientConfig(publicUrl: string | null, token = TOKEN_PLACEHOLDER) {
  return JSON.stringify(
    {
      mcpServers: {
        cloudmcp: {
          type: 'http',
          url: publicEndpoint(publicUrl) ?? 'https://<连接隧道后获取地址>/mcp',
          headers: { Authorization: 'Bearer ' + token },
        },
      },
    },
    null,
    2,
  );
}

export type ConnectionStage = 'public' | 'local' | 'idle';

export const connectionStageLabels: Record<ConnectionStage, string> = {
  public: '公网入口已建立',
  local: '本机已就绪',
  idle: '等待连接',
};

export function connectionStage(snapshot: Pick<Snapshot, 'public_url' | 'server_running'>): ConnectionStage {
  return snapshot.public_url ? 'public' : snapshot.server_running ? 'local' : 'idle';
}

export function tunnelTone(snapshot: Pick<Snapshot, 'public_url' | 'tunnel_running' | 'tunnel_error'>) {
  if (snapshot.public_url) return 'running';
  if (snapshot.tunnel_running) return 'starting';
  return snapshot.tunnel_error ? 'error' : undefined;
}
