import { useState } from 'react';
import { Check, Folder, Loader2, Minus, Play, Square, TriangleAlert } from 'lucide-react';
import type { SnapshotAction } from '../hooks/useSnapshot';
import { clientConfig, publicEndpoint, tunnelTone } from '../lib/connection';
import { native } from '../lib/desktop';
import { runningCount } from '../lib/executions';
import type { Snapshot } from '../types';
import { CopyButton } from './CopyButton';

const PUBLIC_URL_KEY = 'cloudmcp.connection.last-public-url';

function readPublicUrl() {
  try { return localStorage.getItem(PUBLIC_URL_KEY); }
  catch { return null; }
}

function serviceStatus(data: Snapshot, addressChanged: boolean) {
  if (!data.server_running) {
    return { title: '离线', tone: 'idle', reason: '本机服务未启动。' };
  }

  const tunnel = tunnelTone(data);
  if (!data.tunnel_running || tunnel !== 'running') {
    if (data.tunnel_running && data.tunnel_error) {
      return { title: '隧道重连中', tone: 'warning', reason: data.tunnel_error };
    }
    const reason = data.tunnel_running ? '正在等待隧道分配公网地址。' : '连接隧道后可使用公网地址。';
    return { title: '隧道未连接', tone: data.tunnel_error ? 'error' : 'idle', reason: data.tunnel_error ?? reason };
  }

  return { title: addressChanged ? '在线，地址已变化' : '在线', tone: addressChanged ? 'warning' : 'running' };
}

function ServiceCheck({ label, ready }: { label: string; ready: boolean }) {
  return <li data-ready={ready}>
    {ready ? <Check size={14} aria-hidden="true" /> : <Minus size={14} aria-hidden="true" />}
    {label}<span className="sr-only">{ready ? '可用' : '未就绪'}</span>
  </li>;
}

export function StatusPage({ data, busy, action }: { data: Snapshot; busy: string | null; action: SnapshotAction }) {
  const [lastUrl, setLastUrl] = useState(readPublicUrl);
  const publicUrl = data.public_url;
  const addressChanged = Boolean(publicUrl && lastUrl && publicUrl !== lastUrl);
  const status = serviceStatus(data, addressChanged);
  const online = data.server_running && data.tunnel_running && tunnelTone(data) === 'running';
  const publicAddress = online ? publicEndpoint(publicUrl) : null;
  const running = runningCount(data.executions);
  const serverAction = data.server_running ? 'stop_server' : 'start_server';
  const tunnelAction = data.tunnel_running ? 'stop_tunnel' : 'start_tunnel';

  const rememberPublicUrl = () => {
    if (!publicUrl) return;
    setLastUrl(publicUrl);
    try { localStorage.setItem(PUBLIC_URL_KEY, publicUrl); } catch { /* Keep the acknowledgement for this view. */ }
  };

  return <>
    <section className="status-overview" aria-labelledby="status-title">
      <header className="dash-page-heading">
        <div className="status-identity" role="status">
          <span className="h1-status-dot status-light" data-tone={status.tone} aria-hidden="true" />
          <h1 id="status-title">{status.title}</h1>
        </div>
        <button type="button" className={`h1-button${data.server_running ? '' : ' h1-button-primary'}`} disabled={!!busy}
          onClick={() => { void action(serverAction); }} data-service-action={serverAction}>
          {busy === serverAction ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
            : data.server_running ? <Square size={10} fill="currentColor" aria-hidden="true" /> : <Play size={12} fill="currentColor" aria-hidden="true" />}
          {data.server_running ? '停止服务' : '启动服务'}
        </button>
      </header>
      {status.reason && <p className="dash-page-description status-reason select-text" data-tone={status.tone}>{status.reason}</p>}
      <div className="status-context">
        <ul className="status-checks" aria-label="服务状态">
          <ServiceCheck label="本机服务" ready={data.server_running} />
          <ServiceCheck label="隧道" ready={online} />
          <li><Folder size={14} aria-hidden="true" />{data.scopes.length} 个目录</li>
        </ul>
        {data.server_running && <button type="button" className="h1-tag" disabled={!!busy}
          onClick={() => { void action(tunnelAction); }} data-service-action={tunnelAction}>
          {busy === tunnelAction && <Loader2 size={12} className="animate-spin" aria-hidden="true" />}
          {data.tunnel_running ? '断开隧道' : '连接隧道'}
        </button>}
      </div>
    </section>

    <section className="h1-group" aria-labelledby="public-address-title">
      <div className="h1-list-row status-address-row">
        <div className="min-w-0">
          <h2 className="text-sm font-medium" id="public-address-title">公网地址</h2>
          {publicAddress ? <code className="status-address" data-public-address>{publicAddress}</code>
            : <p className="dash-page-description">连接隧道后显示</p>}
        </div>
        <CopyButton text={publicAddress ?? undefined} disabled={!publicAddress} onCopied={rememberPublicUrl} />
      </div>
      <div className="status-config-row">
        <CopyButton button className="h1-button-primary" label="复制配置与令牌" disabled={!publicUrl || !online}
          resolve={async () => clientConfig(publicUrl, await native<string>('get_token'))} onCopied={rememberPublicUrl} />
      </div>
    </section>

    {addressChanged && <div className="status-reminder" data-address-reminder>
      <TriangleAlert size={16} aria-hidden="true" />
      <p role="status">记得在 Hark 里更新连接地址</p>
      <button type="button" className="h1-button" onClick={rememberPublicUrl}>已更新</button>
    </div>}

    <p className="status-tasks" role="status" data-running-executions={running}>
      <span className="h1-status-dot" data-tone={running ? 'working' : undefined} aria-hidden="true" />
      {running ? `Agent 正在工作 · ${running} 个任务运行中` : '没有正在运行的任务'}
    </p>
  </>;
}
