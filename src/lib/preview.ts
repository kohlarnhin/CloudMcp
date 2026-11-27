import type { Audit, Dependency, Execution, PortalStatus, Scope, Snapshot } from '../types';

const publicUrl = 'https://preview.cloudmcp.invalid';
const scopes: Scope[] = [
  {
    id: 'cloudmcp',
    name: 'CloudMCP',
    path: '~/Projects/cloudmcp',
    created_at: '2026-11-16T09:10:00+08:00',
  },
  {
    id: 'docs',
    name: '文档站点',
    path: '~/Projects/docs-site',
    created_at: '2026-11-16T09:30:00+08:00',
  },
  {
    id: 'design-system',
    name: '跨端组件库与设计规范 · Cross-platform Design System and Component Library',
    path: '~/Projects/client-workspaces/platform/frontend/packages/shared-components/design-system',
    created_at: '2026-11-17T10:25:00+08:00',
  },
];

function execution(
  number: number,
  scope: Scope,
  status: Execution['status'],
  time: string,
  fields: Partial<Execution>,
): Execution {
  const active = status === 'starting' || status === 'running' || status === 'unknown';
  return {
    run_id: `ac9da71${number}-5d31-4d7d-a7e3-4fcfa209f9c3`,
    workspace: scope.name,
    request_id: `local-workspace-${number}`,
    kind: 'shell',
    status,
    session_id: null,
    verification: 'unverified',
    created_at: time,
    updated_at: time,
    heartbeat_at: time,
    ended_at: active ? null : time,
    timeout_seconds: 3600,
    summary: '',
    request_brief: '',
    acceptance_criteria: '',
    verification_note: '',
    evidence_paths: [],
    error: null,
    full_access: false,
    ...fields,
  };
}

const [app, docs, design] = scopes;
const runs = [
  execution(8, app, 'succeeded', '2026-11-18T14:37:00+08:00', {
    kind: 'codex',
    session_id: '019a9a38-715e-721b-92c0-d91df4ec103a',
    verification: 'completed',
    full_access: true,
    prompt: '整理设置页的表单，核对连接方式和端口字段的帮助文字。',
    request_brief: '整理设置页表单',
    acceptance_criteria: '两种隧道模式的字段可正确切换，现有配置可继续使用，构建通过。',
    summary: '已整理设置页的表单布局，并通过类型检查与构建。',
    verification_note: '已检查 1160 × 800 和 900 × 640 窗口尺寸。',
    evidence_paths: ['artifacts/settings-preview.png', 'artifacts/build.log'],
    stdout: '$ pnpm build\n\n> cloudmcp@0.3.0 build\n> tsc -b && vite build\n\n✓ 1927 modules transformed.\n✓ built in 1.34s',
    stderr: '构建环境提示：已使用本机缓存。',
  }),
  execution(7, docs, 'running', '2026-11-18T14:25:00+08:00', {
    command: 'pnpm build',
    request_brief: '生成文档站点',
    stdout: '正在生成静态页面…',
  }),
  execution(6, design, 'starting', '2026-11-18T14:16:00+08:00', {
    kind: 'codex',
    full_access: true,
    prompt: '核对按钮与徽标的样式示例。',
    request_brief: '核对组件库的按钮与徽标示例',
  }),
  execution(5, app, 'failed', '2026-11-18T13:54:00+08:00', {
    command: 'pnpm test',
    verification: 'needs_work',
    summary: '设置页交互测试有一项断言失败。',
    verification_note: '复查长期隧道模式下的帮助文字。',
    error: '命令退出，状态码 1。',
    stderr: 'AssertionError: tunnel mode description',
  }),
  execution(4, docs, 'timed_out', '2026-11-18T13:28:00+08:00', {
    command: 'pnpm lint',
    timeout_seconds: 30,
    summary: '文档检查超过执行时限。',
  }),
  execution(3, design, 'unknown', '2026-11-18T11:42:00+08:00', {
    command: 'pnpm build',
    summary: '构建任务的心跳已超时。',
  }),
  execution(2, app, 'cancelled', '2026-11-18T11:03:00+08:00', {
    command: 'pnpm build',
    summary: '旧构建任务已停止。',
  }),
  execution(1, docs, 'interrupted', '2026-11-18T10:11:00+08:00', {
    command: 'pnpm lint',
    summary: '工作站重启后已核实文档检查进程停止。',
  }),
];

const audit: Audit[] = [
  { id: 10, time: runs[0].created_at, action: 'execution.review', target: app.name, success: true },
  { id: 9, time: runs[1].created_at, action: 'execution.start', target: docs.name, success: true },
  { id: 8, time: runs[2].created_at, action: 'codex.exec', target: design.name, success: true },
  { id: 7, time: runs[3].created_at, action: 'execution.end', target: app.name, success: false },
  { id: 6, time: runs[4].created_at, action: 'execution.end', target: docs.name, success: false },
  { id: 5, time: runs[5].created_at, action: 'fs.list', target: design.path, success: true },
  { id: 4, time: runs[6].created_at, action: 'execution.cancel', target: app.name, success: true },
  { id: 3, time: runs[7].created_at, action: 'fs.read', target: `${docs.path}/drafts/setup.md`, success: false },
  { id: 2, time: '2026-11-18T09:31:00+08:00', action: 'tunnel.start', target: publicUrl, success: true },
  { id: 1, time: '2026-11-18T09:30:00+08:00', action: 'server.start', target: '127.0.0.1:8787', success: true },
];

const snapshot: Snapshot = {
  settings: {
    port: 8787,
    cloudflared_path: 'cloudflared',
    tunnel_mode: 'named',
    tunnel_name: 'cloudmcp-workstation',
    public_url: publicUrl,
    result_chars: 6000,
    command_timeout_seconds: 3600,
    local_agent: 'codex',
    codex_enabled: true,
    codex_path: 'codex',
    pi_path: 'pi',
  },
  scopes,
  audit,
  executions: runs.map(({ command, prompt, stdout, stderr, ...run }) => run),
  server_running: true,
  tunnel_running: true,
  public_url: publicUrl,
  tunnel_error: null,
  data_dir: '~/Library/Application Support/dev.cloudmcp.desktop',
  platform: 'macos',
  cloudflare: { logged_in: false, managed_tunnel: false },
};

const dependencies: Dependency[] = [
  { name: 'cloudflared', path: '/opt/homebrew/bin/cloudflared', available: true, version: 'cloudflared version 2026.10.0' },
  { name: 'codex', path: '/opt/homebrew/bin/codex', available: true, version: 'codex-cli 0.92.0' },
];

function previewPortal(): PortalStatus {
  const at = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();
  return {
    current: { name: 'work@example.com', usable: true, shortRemainingPercent: 42, shortResetsAt: at(134), longRemainingPercent: 71, longResetsAt: at(60 * 24 * 3) },
    next: { name: 'side@example.com', usable: true, shortRemainingPercent: 88, shortResetsAt: at(212), longRemainingPercent: 15, longResetsAt: at(60 * 30) },
    allExhausted: false,
    earliestRecoveryAt: null,
  };
}

export function previewNative(command: string, args?: Record<string, unknown>) {
  switch (command) {
    case 'snapshot':
      return { ...snapshot, settings: { ...snapshot.settings }, cloudflare: { ...snapshot.cloudflare } };
    case 'save_settings':
      Object.assign(snapshot.settings, args?.settings);
      snapshot.settings.codex_enabled = snapshot.settings.local_agent === 'codex';
      return null;
    case 'stop_server':
      Object.assign(snapshot, { server_running: false, tunnel_running: false, public_url: null });
      return null;
    case 'cloudflare_login':
      return new Promise(resolve => setTimeout(() => { snapshot.cloudflare.logged_in = true; resolve(null); }, 1200));
    case 'cloudflare_logout':
      snapshot.cloudflare = { logged_in: false, managed_tunnel: false };
      return null;
    case 'cloudflare_setup': {
      const hostname = String(args?.hostname ?? '').trim().toLowerCase();
      Object.assign(snapshot.settings, { tunnel_mode: 'named', tunnel_name: 'cloudmcp-' + hostname.replaceAll('.', '-'), public_url: 'https://' + hostname });
      snapshot.cloudflare.managed_tunnel = true;
      return { hostname };
    }
    case 'start_server':
      snapshot.server_running = true;
      return null;
    case 'execution_detail': {
      const run = runs.find(({ workspace, run_id }) => workspace === args?.workspace && run_id === args?.runId);
      if (!run) throw new Error('找不到执行记录');
      return run;
    }
    case 'dependencies':
      return dependencies;
    case 'portal_status':
      return previewPortal();
    case 'get_token':
      return 'cloudmcp-preview-token';
    default:
      throw new Error('预览模式不执行本机操作');
  }
}
