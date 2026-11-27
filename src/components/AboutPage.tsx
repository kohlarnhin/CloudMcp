import type { AppUpdateController } from '../hooks/useAppUpdate';
import type { Snapshot } from '../types';
import { AppUpdateRow } from './AppUpdateRow';

export function AboutPage({ data, appUpdate }: { data: Snapshot; appUpdate: AppUpdateController }) {
  return <>
    <header className="dash-page-heading">
      <div>
        <h1>关于</h1>
        <p className="dash-page-description">版本、更新与本机数据位置。</p>
      </div>
    </header>
    <section className="dash-section" aria-labelledby="about-version">
      <h2 className="h1-group-title dash-section-title" id="about-version">版本</h2>
      <div className="h1-group"><AppUpdateRow controller={appUpdate} /></div>
    </section>
    <section className="dash-section" aria-labelledby="about-data">
      <h2 className="h1-group-title dash-section-title" id="about-data">本机数据</h2>
      <div className="h1-group">
        <div className="h1-list-row">
          <div className="min-w-0">
            <p className="text-sm font-medium">数据目录</p>
            <p className="settings-help">保存授权、令牌、执行记录与输出，权限为 0700。</p>
          </div>
          <code className="dash-truncate max-w-[50%] select-text text-xs text-muted-foreground" title={data.data_dir}>{data.data_dir || '系统应用数据目录'}</code>
        </div>
      </div>
    </section>
  </>;
}
