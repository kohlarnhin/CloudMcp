import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { ArrowLeft } from 'lucide-react';
import { toast } from 'sonner';
import { AddDirectoryDialog } from './components/AddDirectoryDialog';
import { DirectoriesPage } from './components/DirectoriesPage';
import { ExecutionPage } from './components/ExecutionPage';
import { ExecutionTable } from './components/ExecutionTable';
import { SettingsPage } from './components/SettingsPage';
import { StatusPage } from './components/StatusPage';
import { Alert, AlertDescription } from './components/ui/alert';
import { SegmentedControl } from './components/ui/segmented-control';
import { Toaster } from './components/ui/sonner';
import { useAppUpdate } from './hooks/useAppUpdate';
import { useAppearance } from './hooks/useAppearance';
import { useSnapshot } from './hooks/useSnapshot';
import { desktop } from './lib/desktop';
import type { Execution } from './types';

const tabs = [
  { value: 'status', label: '状态' },
  { value: 'directories', label: '目录' },
  { value: 'settings', label: '设置' },
] as const;

type Tab = (typeof tabs)[number]['value'];
type ExecutionParent = 'settings' | 'executions';
type View =
  | { page: Tab | 'executions' }
  | { page: 'execution'; workspace: string; runId: string; back: ExecutionParent };

export function App() {
  const appearance = useAppearance();
  const appUpdate = useAppUpdate();
  const { data, busy, loadError, refresh, action } = useSnapshot();
  const [view, setView] = useState<View>({ page: 'status' });
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [directory, setDirectory] = useState<string | null>(null);
  const tab = view.page === 'execution' || view.page === 'executions' ? 'settings' : view.page;

  const addDirectory = async () => {
    try {
      if (!desktop) throw new Error('请在桌面应用中添加目录');
      const path = await open({ directory: true, multiple: false, title: '选择向 MCP 客户端开放完整权限的目录' });
      if (typeof path === 'string') setDirectory(path);
    } catch (error) {
      toast.error(String(error));
    }
  };
  const openExecution = (run: Execution, back: ExecutionParent) => {
    setView({ page: 'execution', workspace: run.workspace, runId: run.run_id, back });
  };
  const backToSettings = () => {
    setView({ page: 'settings' });
    window.requestAnimationFrame(() => document.getElementById('diagnostics-summary')?.focus());
  };

  return <div className="h1-window flex cursor-default select-none flex-col">
    <Toaster theme={appearance.colorScheme} />
    <header className="app-titlebar" data-tauri-drag-region>
      <SegmentedControl className="app-tabs" label="主导航" options={tabs} value={tab} onChange={page => setView({ page })} />
    </header>
    <main key={view.page} className="h1-main min-h-0 min-w-0 flex-1" data-page={view.page}>
      <div className="dash-main">
        {loadError && <Alert variant="destructive" data-load-error>
          <AlertDescription className="select-text wrap-anywhere">{loadError}</AlertDescription>
        </Alert>}
        {view.page === 'status' && <StatusPage data={data} busy={busy} action={action} />}
        {view.page === 'directories' && <DirectoriesPage data={data} action={action} onAdd={() => { void addDirectory(); }} />}
        {view.page === 'settings' && <SettingsPage data={data} refresh={refresh} appearance={appearance} appUpdate={appUpdate}
          diagnosticsOpen={diagnosticsOpen} onDiagnosticsToggle={setDiagnosticsOpen}
          onOpenExecutions={() => setView({ page: 'executions' })} onOpenExecution={run => openExecution(run, 'settings')} />}
        {view.page === 'executions' && <>
          <button type="button" className="h1-button self-start" onClick={backToSettings}>
            <ArrowLeft size={13} aria-hidden="true" />返回设置
          </button>
          <header className="dash-page-heading">
            <div>
              <h1>执行记录</h1>
              <p className="dash-page-description">最多显示本机快照中的最近 50 条执行。</p>
            </div>
          </header>
          <ExecutionTable executions={data.executions} onOpen={run => openExecution(run, 'executions')} />
        </>}
        {view.page === 'execution' && <ExecutionPage key={view.runId} workspace={view.workspace} runId={view.runId}
          snapshotRun={data.executions.find(run => run.run_id === view.runId)} refresh={refresh}
          backLabel={view.back === 'settings' ? '返回设置' : '返回执行记录'}
          onBack={view.back === 'settings' ? backToSettings : () => setView({ page: 'executions' })} />}
      </div>
    </main>
    {directory && <AddDirectoryDialog key={directory} path={directory} action={action} onClose={() => setDirectory(null)} />}
  </div>;
}

export default App;
