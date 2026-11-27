import { useCallback, useEffect, useState } from 'react';
import { ArrowRight, Loader2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import { usePortalStatus } from '../hooks/usePortalStatus';
import { desktop, native, preview } from '../lib/desktop';
import { shortPath } from '../lib/format';
import { clampPercent, portalDetail, quotaTitle, quotaTone } from '../lib/portal';
import type { Dependency, Execution, PortalAccount, Snapshot } from '../types';
import { ActivityTable } from './ActivityTable';
import { ExecutionTable } from './ExecutionTable';

const recentLimit = 8;

function QuotaMeter({ label, account, kind }: { label: string; account: PortalAccount; kind: 'short' | 'long' }) {
  const percent = clampPercent(kind === 'short' ? account.shortRemainingPercent : account.longRemainingPercent);
  return <span className="dash-quota" title={quotaTitle(label, account, kind)}>
    <span className="dash-quota-label">{label}</span>
    <span className="dash-quota-bar" role="meter" aria-label={label + '剩余额度'} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
      <i style={{ width: percent + '%' }} data-tone={quotaTone(percent)} />
    </span>
    <span className="dash-quota-value">{percent}%</span>
  </span>;
}

function PortalEnvironment() {
  const { status, error, loaded } = usePortalStatus();
  const account = status?.current;
  const detail = status ? portalDetail(status) : error ? '未运行或未开启 MCP 端点' : loaded ? '暂无额度信息' : '正在读取额度…';
  return <div className="h1-list-row diagnostic-portal">
    <div className="min-w-0">
      <p className="text-sm font-medium">Codex Portal</p>
      <p className="settings-help wrap-anywhere">{detail}</p>
    </div>
    {account && <div className="dash-quota-group">
      <QuotaMeter label="5h" account={account} kind="short" />
      <QuotaMeter label="周" account={account} kind="long" />
    </div>}
  </div>;
}

function Environment({ agent }: { agent: 'none' | 'codex' | 'pi' }) {
  const codexEnabled = agent === 'codex';
  const dependencyNames = agent === 'none' ? ['cloudflared'] : ['cloudflared', agent];
  const [dependencies, setDependencies] = useState<Dependency[] | null>(null);
  const [checking, setChecking] = useState(false);
  const check = useCallback(async () => {
    setChecking(true);
    try {
      setDependencies(await native<Dependency[]>('dependencies'));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setChecking(false);
    }
  }, []);
  useEffect(() => {
    if (desktop || preview) void check();
  }, [check, agent]);

  return <section className="dash-section" aria-labelledby="diagnostics-environment">
    <header className="dash-section-heading">
      <h2 className="h1-group-title" id="diagnostics-environment">运行环境</h2>
      <button type="button" className="h1-tag" disabled={checking} onClick={() => { void check(); }} data-check-environment>
        <RefreshCw size={12} className={checking ? 'animate-spin' : undefined} aria-hidden="true" />检查环境
      </button>
    </header>
    <div className="h1-group">
      {dependencyNames.map(name => {
        const dependency = dependencies?.find(item => item.name === name);
        const details = dependency ? [dependency.version, shortPath(dependency.path)].filter(Boolean).join(' · ') : null;
        const state = checking ? '检查中…' : dependency ? (dependency.available ? '可用' : '未找到') : '尚未检查';
        return <div key={name} className="h1-list-row" data-dependency={name}>
          <div className="min-w-0">
            <p className="text-sm font-medium">{name}</p>
            {details && <p className="settings-help dash-truncate select-text font-mono" title={details}>{details}</p>}
          </div>
          <span className="dash-state shrink-0" data-tone={!checking && dependency && !dependency.available ? 'error' : 'idle'}>
            {checking ? <Loader2 size={12} className="animate-spin" aria-hidden="true" />
              : <span className="h1-status-dot" data-tone={dependency ? (dependency.available ? 'running' : 'error') : undefined} aria-hidden="true" />}
            {state}
          </span>
        </div>;
      })}
      {codexEnabled && (desktop || preview) && <PortalEnvironment />}
    </div>
  </section>;
}

export function DiagnosticsPage({ data, onOpenExecutions, onOpenExecution }: {
  data: Snapshot;
  onOpenExecutions: () => void;
  onOpenExecution: (run: Execution) => void;
}) {
  return <>
    <header className="dash-page-heading">
      <div>
        <h1>诊断</h1>
        <p className="dash-page-description">排查连接、执行与运行环境时使用。</p>
      </div>
    </header>
    <section className="dash-section" aria-labelledby="diagnostics-executions">
      <header className="dash-section-heading">
        <h2 className="h1-group-title" id="diagnostics-executions">最近执行</h2>
        <button type="button" className="h1-tag" onClick={onOpenExecutions}>
          查看全部 <ArrowRight size={12} aria-hidden="true" />
        </button>
      </header>
      <ExecutionTable executions={data.executions.slice(0, recentLimit)} onOpen={onOpenExecution} />
    </section>
    <section className="dash-section" aria-labelledby="diagnostics-activity">
      <h2 className="h1-group-title dash-section-title" id="diagnostics-activity">最近访问</h2>
      <ActivityTable audit={data.audit.slice(0, recentLimit)} />
    </section>
    <Environment agent={data.settings.local_agent} />
  </>;
}
