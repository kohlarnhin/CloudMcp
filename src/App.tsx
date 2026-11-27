import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Activity, FolderKey, Info, ListChecks, SlidersHorizontal, Stethoscope, type LucideIcon } from 'lucide-react';
import { toast } from 'sonner';
import { AboutPage } from './components/AboutPage';
import { AddDirectoryDialog } from './components/AddDirectoryDialog';
import { DiagnosticsPage } from './components/Diagnostics';
import { DirectoriesPage } from './components/DirectoriesPage';
import { ExecutionPage } from './components/ExecutionPage';
import { ExecutionTable } from './components/ExecutionTable';
import { Logo } from './components/Logo';
import { SettingsPage } from './components/SettingsPage';
import { SidebarStatus } from './components/SidebarStatus';
import { StatusPage } from './components/StatusPage';
import { Alert, AlertDescription } from './components/ui/alert';
import { Toaster } from './components/ui/sonner';
import { useAppUpdate } from './hooks/useAppUpdate';
import { useAppearance } from './hooks/useAppearance';
import { useSnapshot } from './hooks/useSnapshot';
import { desktop } from './lib/desktop';
import { runningCount } from './lib/executions';
import type { Execution } from './types';

type Page = 'status' | 'directories' | 'executions' | 'diagnostics' | 'settings' | 'about';
type View =
  | { page: Page }
  | { page: 'execution'; workspace: string; runId: string; back: 'executions' | 'diagnostics' };

interface NavItem {
  id: Page;
  label: string;
  icon: LucideIcon;
}

const navGroups: Array<{ label: string; items: NavItem[] }> = [
  {
    label: '连接',
    items: [
      { id: 'status', label: '服务状态', icon: Activity },
      { id: 'directories', label: '授权目录', icon: FolderKey },
    ],
  },
  {
    label: '执行',
    items: [
      { id: 'executions', label: '执行记录', icon: ListChecks },
      { id: 'diagnostics', label: '诊断', icon: Stethoscope },
    ],
  },
  {
    label: '应用',
    items: [
      { id: 'settings', label: '设置', icon: SlidersHorizontal },
      { id: 'about', label: '关于', icon: Info },
    ],
  },
];

export function App() {
  const appearance = useAppearance();
  const [view, setView] = useState<View>({ page: 'status' });
  const appUpdate = useAppUpdate(() => setView({ page: 'about' }));
  const { data, busy, loadError, refresh, action } = useSnapshot();
  const [directory, setDirectory] = useState<string | null>(null);
  const current: Page = view.page === 'execution' ? view.back : view.page;
  const running = runningCount(data.executions);
  const updateReady = appUpdate.state.status === 'available';

  const addDirectory = async () => {
    try {
      if (!desktop) throw new Error('请在桌面应用中添加目录');
      const path = await open({ directory: true, multiple: false, title: '选择向 MCP 客户端开放完整权限的目录' });
      if (typeof path === 'string') setDirectory(path);
    } catch (error) {
      toast.error(String(error));
    }
  };
  const openExecution = (run: Execution, back: 'executions' | 'diagnostics') => {
    setView({ page: 'execution', workspace: run.workspace, runId: run.run_id, back });
  };

  return <><Toaster theme={appearance.colorScheme} /><div className="app-window cursor-default select-none">
    <aside className="app-sidebar" aria-label="主导航侧边栏">
      <header className="sidebar-titlebar" data-tauri-drag-region>
        <div className="sidebar-chrome" data-tauri-drag-region />
        <div className="sidebar-brand" data-tauri-drag-region>
          <Logo className="size-8 shrink-0" />
          <div className="min-w-0 flex-1" data-tauri-drag-region>
            <div className="flex items-center gap-1.5" data-tauri-drag-region>
              <h1 className="sidebar-brand-title" data-tauri-drag-region>CloudMCP</h1>
              <button type="button" className="sidebar-version" onClick={() => setView({ page: 'about' })}
                title={updateReady ? '发现新版本，点击查看' : '查看版本与更新'}>
                v{__APP_VERSION__}
                {updateReady && <span className="h1-status-dot" data-tone="error" aria-label="有新版本" />}
              </button>
            </div>
            <p className="sidebar-brand-subtitle" data-tauri-drag-region>远端 Agent 本机网关</p>
          </div>
        </div>
      </header>

      <nav className="sidebar-nav" aria-label="主导航">
        {navGroups.map(group => <div key={group.label}>
          <div className="sidebar-group-heading">{group.label}</div>
          <ul className="flex flex-col gap-0.5">
            {group.items.map(item => {
              const selected = current === item.id;
              const badge = item.id === 'executions' && running > 0 ? running : null;
              return <li key={item.id}>
                <button type="button" className="sidebar-nav-item" data-selected={selected || undefined}
                  aria-current={selected ? 'page' : undefined} data-nav={item.id} onClick={() => setView({ page: item.id })}>
                  <span className="sidebar-nav-icon" aria-hidden="true"><item.icon size={15} strokeWidth={1.9} /></span>
                  <span className="dash-truncate">{item.label}</span>
                  {badge && <span className="sidebar-nav-badge" aria-label={`${badge} 个运行中`}>{badge}</span>}
                  {item.id === 'about' && updateReady && <span className="sidebar-nav-update">新版本</span>}
                </button>
              </li>;
            })}
          </ul>
        </div>)}
      </nav>

      <div className="sidebar-footer">
        <SidebarStatus data={data} onOpen={() => setView({ page: 'status' })} />
      </div>
    </aside>

    <main key={view.page === 'execution' ? view.runId : view.page} className="app-main" data-page={view.page}>
      <div className="app-main-drag" data-tauri-drag-region />
      <div className="dash-main">
        {loadError && <Alert variant="destructive" data-load-error>
          <AlertDescription className="select-text wrap-anywhere">{loadError}</AlertDescription>
        </Alert>}
        {view.page === 'status' && <StatusPage data={data} busy={busy} action={action} />}
        {view.page === 'directories' && <DirectoriesPage data={data} action={action} onAdd={() => { void addDirectory(); }} />}
        {view.page === 'executions' && <>
          <header className="dash-page-heading">
            <div>
              <h1>执行记录</h1>
              <p className="dash-page-description">本机最近 50 条命令{data.settings.codex_enabled ? '与 Codex 会话' : ''}，远端断开后仍持续记录。</p>
            </div>
          </header>
          <ExecutionTable executions={data.executions} onOpen={run => openExecution(run, 'executions')} />
        </>}
        {view.page === 'diagnostics' && <DiagnosticsPage data={data}
          onOpenExecutions={() => setView({ page: 'executions' })} onOpenExecution={run => openExecution(run, 'diagnostics')} />}
        {view.page === 'settings' && <SettingsPage data={data} refresh={refresh} appearance={appearance} />}
        {view.page === 'about' && <AboutPage data={data} appUpdate={appUpdate} />}
        {view.page === 'execution' && <ExecutionPage key={view.runId} workspace={view.workspace} runId={view.runId}
          snapshotRun={data.executions.find(run => run.run_id === view.runId)} refresh={refresh}
          backLabel={view.back === 'diagnostics' ? '返回诊断' : '返回执行记录'}
          onBack={() => setView({ page: view.back })} />}
      </div>
    </main>
    {directory && <AddDirectoryDialog key={directory} path={directory} action={action} onClose={() => setDirectory(null)} />}
  </div></>;
}

export default App;
