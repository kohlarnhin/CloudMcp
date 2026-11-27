import { useState } from 'react';
import { ChevronDown, Cloud, Loader2, LogOut, Wand2 } from 'lucide-react';
import { toast } from 'sonner';
import { localEndpoint } from '../lib/connection';
import { native } from '../lib/desktop';
import type { Settings, Snapshot } from '../types';
import { Input } from './ui/input';

const pathInput = 'w-1/2 max-w-[270px] shrink-0 font-mono text-xs';

function hostOf(url: string) {
  try { return url ? new URL(url).host : ''; } catch { return ''; }
}

/** 固定隧道：登录 Cloudflare 后填写域名即可自动创建隧道与 DNS；保留手动配置作为备选。 */
export function CloudflareTunnel({ data, settings, update, saveDraft, refresh }: {
  data: Snapshot;
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  saveDraft: () => Promise<void>;
  refresh: () => Promise<void>;
}) {
  const [busy, setBusy] = useState<'login' | 'logout' | 'setup' | null>(null);
  const [hostname, setHostname] = useState(() => hostOf(data.settings.public_url));
  const { logged_in: loggedIn, managed_tunnel: managed } = data.cloudflare;
  const configuredHost = hostOf(data.settings.public_url);

  const run = async (kind: 'login' | 'logout' | 'setup', task: () => Promise<void>) => {
    setBusy(kind);
    try {
      await task();
      await refresh();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(null);
    }
  };
  const login = () => run('login', async () => {
    await native('cloudflare_login');
    toast.success('已登录 Cloudflare');
  });
  const logout = () => run('logout', async () => {
    await native('cloudflare_logout');
  });
  const setup = () => run('setup', async () => {
    await saveDraft();
    const result = await native<{ hostname: string }>('cloudflare_setup', { hostname });
    setHostname(result.hostname);
    toast.success('固定隧道已配置', { description: `启动服务并连接隧道后，地址为 https://${result.hostname}/mcp` });
  });

  return <>
    <div className="h1-list-row" data-cloudflare-account>
      <div className="min-w-0">
        <p className="text-sm font-medium">Cloudflare 账号</p>
        <p className="settings-help">
          {busy === 'login' ? '已在浏览器打开授权页，选择域名并授权后会自动完成。'
            : loggedIn ? '已登录。凭据只保存在 CloudMCP 数据目录。'
              : '登录后填写域名，即可自动创建隧道和 DNS 记录。'}
        </p>
      </div>
      {loggedIn
        ? <button type="button" className="h1-tag" disabled={!!busy} onClick={() => { void logout(); }}>
          <LogOut size={12} aria-hidden="true" />退出登录
        </button>
        : <button type="button" className="h1-button" disabled={!!busy} onClick={() => { void login(); }} data-cloudflare-login>
          {busy === 'login' ? <Loader2 size={13} className="animate-spin" aria-hidden="true" /> : <Cloud size={13} aria-hidden="true" />}
          {busy === 'login' ? '等待授权…' : '登录 Cloudflare'}
        </button>}
    </div>
    <div className="h1-list-row settings-hostname" data-cloudflare-hostname>
      <div className="min-w-0">
        <label htmlFor="tunnel-hostname" className="text-sm font-medium">域名</label>
        <p id="tunnel-hostname-help" className="settings-help">
          {managed && configuredHost ? `已配置 ${configuredHost}，可直接连接隧道。` : '需托管在当前账号下，例如 mcp.example.com。'}
        </p>
      </div>
      <div className="settings-hostname-actions">
        <Input id="tunnel-hostname" value={hostname} placeholder="mcp.example.com" disabled={!loggedIn || !!busy}
          onChange={event => setHostname(event.target.value)} aria-describedby="tunnel-hostname-help" className="w-[200px] font-mono text-xs" />
        <button type="button" className="h1-button h1-button-primary" disabled={!loggedIn || !hostname.trim() || !!busy}
          onClick={() => { void setup(); }} data-cloudflare-setup>
          {busy === 'setup' ? <Loader2 size={13} className="animate-spin" aria-hidden="true" /> : <Wand2 size={13} aria-hidden="true" />}
          {managed && hostOf(`https://${hostname.trim()}`) === configuredHost ? '重新配置' : '自动配置'}
        </button>
      </div>
    </div>
    <details className="settings-manual" open={!managed && !!settings.tunnel_name && !loggedIn ? true : undefined}>
      <summary><span>手动配置</span><ChevronDown size={14} aria-hidden="true" /></summary>
      <div className="h1-list-row">
        <div className="min-w-0">
          <label htmlFor="tunnel-name" className="text-sm font-medium">隧道名称或 ID</label>
          <p className="settings-help">使用 ~/.cloudflared 中自行配置的隧道时填写。</p>
        </div>
        <Input id="tunnel-name" value={settings.tunnel_name} title={settings.tunnel_name}
          onChange={event => update('tunnel_name', event.target.value)} className={pathInput} />
      </div>
      <div className="h1-list-row">
        <div className="min-w-0">
          <label htmlFor="public-url" className="text-sm font-medium">公网 HTTPS 地址</label>
          <p id="public-url-help" className="settings-help">{'路由应指向 http://' + localEndpoint(settings.port) + '。'}</p>
        </div>
        <Input id="public-url" type="url" value={settings.public_url} title={settings.public_url}
          onChange={event => update('public_url', event.target.value)} aria-describedby="public-url-help" className={pathInput} />
      </div>
    </details>
  </>;
}
