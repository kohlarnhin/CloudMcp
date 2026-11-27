import { useState, type FormEvent, type ReactNode } from 'react';
import { Check, Loader2, LockKeyhole, ShieldAlert } from 'lucide-react';
import { toast } from 'sonner';
import type { AppUpdateController } from '../hooks/useAppUpdate';
import type { AppearanceController } from '../hooks/useAppearance';
import { appearances } from '../lib/appearance';
import { localEndpoint } from '../lib/connection';
import { native } from '../lib/desktop';
import type { Execution, Settings, Snapshot } from '../types';
import { AppUpdateRow } from './AppUpdateRow';
import { CopyButton } from './CopyButton';
import { Diagnostics } from './Diagnostics';
import { RotateTokenDialog } from './RotateTokenDialog';
import { Alert, AlertDescription } from './ui/alert';
import { Input } from './ui/input';
import { SegmentedControl } from './ui/segmented-control';

const tunnelModes = [
  { value: 'quick', label: '临时隧道' },
  { value: 'named', label: '固定隧道' },
] as const;
const pathInput = 'w-1/2 max-w-[270px] shrink-0 font-mono text-xs';

function SettingsSection({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return <section className="dash-section" aria-labelledby={id} data-settings-section={id}>
    <h2 className="h1-group-title dash-section-title" id={id}>{title}</h2>
    <div className="h1-group">{children}</div>
  </section>;
}

function SettingsField({ id, label, description, children }: {
  id: string;
  label: string;
  description?: string;
  children: ReactNode;
}) {
  return <div className="h1-list-row">
    <div className="min-w-0">
      <label htmlFor={id} className="text-sm font-medium">{label}</label>
      {description && <p id={id + '-help'} className="settings-help">{description}</p>}
    </div>
    {children}
  </div>;
}

export function SettingsPage({ data, refresh, appearance, appUpdate, diagnosticsOpen, onDiagnosticsToggle, onOpenExecutions, onOpenExecution }: {
  data: Snapshot;
  refresh: () => Promise<void>;
  appearance: AppearanceController;
  appUpdate: AppUpdateController;
  diagnosticsOpen: boolean;
  onDiagnosticsToggle: (open: boolean) => void;
  onOpenExecutions: () => void;
  onOpenExecution: (run: Execution) => void;
}) {
  const [draft, setDraft] = useState<Partial<Settings>>({});
  const [saving, setSaving] = useState(false);
  const [rotating, setRotating] = useState(false);
  const settings = { ...data.settings, ...draft };
  const locked = data.server_running || saving;

  const update = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    setDraft(current => ({ ...current, [key]: value }));
  };
  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (locked) return;
    setSaving(true);
    try {
      await native('save_settings', { settings });
      await refresh();
      setDraft({});
      toast.success('设置已保存');
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSaving(false);
    }
  };

  return <>
    <header className="dash-page-heading">
      <h1>设置</h1>
      <button type="submit" form="settings-form" className="h1-button h1-button-primary" disabled={locked} data-save-settings>
        {saving ? <Loader2 size={13} className="animate-spin" aria-hidden="true" /> : <Check size={13} aria-hidden="true" />}保存设置
      </button>
    </header>

    {data.server_running && <Alert variant="warning" className="border-warning/30 bg-warning-muted text-warning-foreground">
      <ShieldAlert aria-hidden="true" />
      <AlertDescription>服务运行中，停止服务后才能修改设置。</AlertDescription>
    </Alert>}

    <form id="settings-form" onSubmit={save} className="settings-form">
      <SettingsSection id="settings-connection" title="连接">
        <fieldset disabled={locked} className="min-w-0">
          <SettingsField id="port" label="MCP 端口" description={'http://' + localEndpoint(settings.port) + '/mcp'}>
            <Input id="port" type="number" min={1024} max={65535} required value={settings.port}
              onChange={event => update('port', Number(event.target.value))} aria-describedby="port-help" className="w-28 shrink-0 tabular-nums" />
          </SettingsField>
          <div className="h1-list-row">
            <div className="min-w-0">
              <p id="settings-tunnel-mode" className="text-sm font-medium">隧道方式</p>
              <p className="settings-help">
                {settings.tunnel_mode === 'quick' ? '自动生成临时 HTTPS 地址。' : '使用已在本机配置凭据和路由的命名隧道。'}
              </p>
            </div>
            <SegmentedControl labelledBy="settings-tunnel-mode" value={settings.tunnel_mode} options={tunnelModes} disabled={locked}
              className="dash-segments" onChange={value => update('tunnel_mode', value)} />
          </div>
          {settings.tunnel_mode === 'named' && <>
            <SettingsField id="tunnel-name" label="隧道名称或 ID">
              <Input id="tunnel-name" required value={settings.tunnel_name} title={settings.tunnel_name}
                onChange={event => update('tunnel_name', event.target.value)} className={pathInput} />
            </SettingsField>
            <SettingsField id="public-url" label="公网 HTTPS 地址" description={'路由应指向 http://' + localEndpoint(settings.port) + '。'}>
              <Input id="public-url" type="url" required value={settings.public_url} title={settings.public_url}
                onChange={event => update('public_url', event.target.value)} aria-describedby="public-url-help" className={pathInput} />
            </SettingsField>
          </>}
          <SettingsField id="cloudflared-path" label="cloudflared 可执行文件" description="填写 cloudflared 自动选择内置版，也可填写本机可执行文件的路径。">
            <Input id="cloudflared-path" required value={settings.cloudflared_path} title={settings.cloudflared_path}
              onChange={event => update('cloudflared_path', event.target.value)} aria-describedby="cloudflared-path-help" className={pathInput} />
          </SettingsField>
        </fieldset>
        <div className="h1-list-row settings-token">
          <div className="min-w-0">
            <p className="text-sm font-medium">访问令牌</p>
            <p className="settings-help">轮换后需要同步更新 Hark 的连接配置。</p>
          </div>
          <div className="settings-token-actions">
            <span className="settings-token-mask" role="img" aria-label="访问令牌已隐藏">
              <LockKeyhole size={13} aria-hidden="true" /><span aria-hidden="true">••••••••</span>
            </span>
            <CopyButton resolve={() => native<string>('get_token')} label="复制令牌" />
            <button type="button" className="h1-tag" onClick={() => setRotating(true)} data-rotate-token-open>轮换</button>
          </div>
        </div>
      </SettingsSection>

      <SettingsSection id="settings-execution" title="执行">
        <fieldset disabled={locked} className="min-w-0">
          <SettingsField id="command-timeout" label="默认执行时限（秒）" description="默认 3,600 秒，最多 86,400 秒。">
            <Input id="command-timeout" type="number" min={1} max={86400} required value={settings.command_timeout_seconds}
              onChange={event => update('command_timeout_seconds', Number(event.target.value))} aria-describedby="command-timeout-help" className="w-28 shrink-0 tabular-nums" />
          </SettingsField>
          <SettingsField id="result-limit" label="命令输出字符上限" description="单个文本字段按此上限截断。每条原始输出流最多保存 16 MiB。">
            <Input id="result-limit" type="number" min={1000} max={16000} required value={settings.result_chars}
              onChange={event => update('result_chars', Number(event.target.value))} aria-describedby="result-limit-help" className="w-28 shrink-0 tabular-nums" />
          </SettingsField>
          <SettingsField id="codex-path" label="Codex 可执行文件" description="使用本机 Codex 的账号和配置。">
            <Input id="codex-path" required value={settings.codex_path} title={settings.codex_path}
              onChange={event => update('codex_path', event.target.value)} aria-describedby="codex-path-help" className={pathInput} />
          </SettingsField>
        </fieldset>
      </SettingsSection>
    </form>

    <SettingsSection id="settings-appearance" title="外观">
      <div className="h1-list-row">
        <span id="settings-theme" className="text-sm font-medium">主题</span>
        <SegmentedControl labelledBy="settings-theme" value={appearance.appearance} options={appearances}
          className="dash-segments" onChange={appearance.changeAppearance} />
      </div>
    </SettingsSection>

    <SettingsSection id="settings-about" title="关于">
      <AppUpdateRow controller={appUpdate} />
      <div className="h1-list-row">
        <p className="shrink-0 text-sm font-medium">本机数据</p>
        <code className="dash-truncate select-text text-xs text-muted-foreground" title={data.data_dir}>{data.data_dir || '系统应用数据目录'}</code>
      </div>
    </SettingsSection>

    <Diagnostics data={data} open={diagnosticsOpen} onToggle={onDiagnosticsToggle}
      onOpenExecutions={onOpenExecutions} onOpenExecution={onOpenExecution} />
    {rotating && <RotateTokenDialog onClose={() => setRotating(false)} />}
  </>;
}
