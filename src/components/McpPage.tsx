import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { Download, Loader2, Pencil, Plug, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { desktop, native, preview } from '../lib/desktop';
import { emptyProxyServer, linesToMap, mapToLines } from '../lib/proxy';
import type { ImportSource, ProxyServer } from '../types';
import Button from './ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './ui/dialog';
import { Empty, EmptyDescription, EmptyTitle } from './ui/empty';
import { Input } from './ui/input';
import { Label } from './ui/label';
import { SegmentedControl } from './ui/segmented-control';
import { Switch } from './ui/switch';

const transports = [
  { value: 'stdio', label: 'stdio 命令' },
  { value: 'http', label: 'HTTP 地址' },
] as const;
const textarea = 'h1-field block min-h-[72px] w-full select-text px-3 py-2 font-mono text-xs outline-none placeholder:text-muted-foreground';

function Field({ id, label, help, children }: { id: string; label: string; help?: string; children: ReactNode }) {
  return <div className="space-y-1.5">
    <Label htmlFor={id}>{label}</Label>
    {children}
    {help && <p className="settings-help">{help}</p>}
  </div>;
}

function EditorDialog({ initial, onClose, onSaved }: {
  initial: ProxyServer | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [server, setServer] = useState<ProxyServer>(initial ?? emptyProxyServer());
  const [args, setArgs] = useState((initial?.args ?? []).join('\n'));
  const [env, setEnv] = useState(mapToLines(initial?.env ?? {}, '='));
  const [headers, setHeaders] = useState(mapToLines(initial?.headers ?? {}, ': '));
  const [saving, setSaving] = useState(false);
  const set = <K extends keyof ProxyServer>(key: K, value: ProxyServer[K]) => setServer(current => ({ ...current, [key]: value }));

  const save = async () => {
    setSaving(true);
    try {
      const next: ProxyServer = {
        ...server,
        args: args.split('\n').map(line => line.trim()).filter(Boolean),
        env: linesToMap(env, '='),
        headers: linesToMap(headers, ':'),
      };
      await native('proxy_save', { server: next, original: initial?.name ?? null });
      toast.success(initial ? '已保存' : '已添加');
      onSaved();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSaving(false);
    }
  };

  return <Dialog open onOpenChange={open => { if (!open && !saving) onClose(); }}>
    <DialogContent className="max-h-[calc(100vh-4rem)] max-w-xl overflow-y-auto" hideClose={saving} data-proxy-editor>
      <DialogHeader>
        <DialogTitle>{initial ? `编辑 ${initial.name}` : '新增 MCP 服务器'}</DialogTitle>
        <DialogDescription>值里可以用 {'${变量}'} 引用登录 shell 的环境变量，以 ! 开头表示取命令输出。</DialogDescription>
      </DialogHeader>
      <form className="space-y-4" onSubmit={event => { event.preventDefault(); if (!saving) void save(); }}>
        <Field id="proxy-name" label="名称" help="字母、数字、下划线或连字符，远端用它指定服务器。">
          <Input id="proxy-name" required autoFocus value={server.name} maxLength={48} disabled={saving}
            onChange={event => set('name', event.target.value)} className="font-mono text-xs" />
        </Field>
        <div className="space-y-1.5">
          <p id="proxy-transport" className="text-sm font-medium">连接方式</p>
          <SegmentedControl labelledBy="proxy-transport" value={server.transport} options={transports} disabled={saving}
            className="dash-segments w-fit" onChange={value => set('transport', value)} />
        </div>
        {server.transport === 'stdio' ? <>
          <Field id="proxy-command" label="命令" help="例如 npx、uvx 或可执行文件的完整路径。">
            <Input id="proxy-command" required value={server.command} disabled={saving} placeholder="npx"
              onChange={event => set('command', event.target.value)} className="font-mono text-xs" />
          </Field>
          <Field id="proxy-args" label="参数" help="每行一个。">
            <textarea id="proxy-args" className={textarea} value={args} disabled={saving}
              placeholder={'-y\n@modelcontextprotocol/server-filesystem\n~/Projects'} onChange={event => setArgs(event.target.value)} />
          </Field>
          <Field id="proxy-env" label="环境变量" help="每行 KEY=VALUE。">
            <textarea id="proxy-env" className={textarea} value={env} disabled={saving}
              placeholder="GITHUB_TOKEN=${GITHUB_TOKEN}" onChange={event => setEnv(event.target.value)} />
          </Field>
          <Field id="proxy-cwd" label="工作目录" help="留空时为主目录。">
            <Input id="proxy-cwd" value={server.cwd} disabled={saving} placeholder="~"
              onChange={event => set('cwd', event.target.value)} className="font-mono text-xs" />
          </Field>
        </> : <>
          <Field id="proxy-url" label="地址" help="Streamable HTTP 端点，通常以 /mcp 结尾；不支持旧的 SSE 端点。">
            <Input id="proxy-url" required type="url" value={server.url} disabled={saving} placeholder="http://127.0.0.1:3000/mcp"
              onChange={event => set('url', event.target.value)} className="font-mono text-xs" />
          </Field>
          <Field id="proxy-headers" label="请求头" help="每行 Name: Value。">
            <textarea id="proxy-headers" className={textarea} value={headers} disabled={saving}
              placeholder={'Authorization: Bearer ${TOKEN}'} onChange={event => setHeaders(event.target.value)} />
          </Field>
        </>}
        <Field id="proxy-description" label="说明" help="一句话说明用途，远端选择服务器时会看到。">
          <Input id="proxy-description" value={server.description} maxLength={300} disabled={saving}
            onChange={event => set('description', event.target.value)} />
        </Field>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={saving} onClick={onClose}>取消</Button>
          <Button type="submit" disabled={saving}>{saving && <Loader2 className="animate-spin" />}保存</Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>;
}

function ImportDialog({ existing, onClose, onImported }: {
  existing: ProxyServer[];
  onClose: () => void;
  onImported: () => void;
}) {
  const [sources, setSources] = useState<ImportSource[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [importing, setImporting] = useState(false);
  const key = (source: string, name: string) => source + '\u0000' + name;

  useEffect(() => {
    native<ImportSource[]>('proxy_import_sources').then(found => {
      setSources(found);
      const names = new Set(existing.map(s => s.name));
      setSelected(new Set(found.flatMap(s => s.servers.filter(server => !names.has(server.name)).map(server => key(s.source, server.name)))));
    }).catch(error => { toast.error(String(error)); setSources([]); });
  }, [existing]);

  const toggle = (id: string) => setSelected(current => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  const run = async () => {
    const servers = (sources ?? []).flatMap(s => s.servers.filter(server => selected.has(key(s.source, server.name))));
    setImporting(true);
    try {
      const added = await native<string[]>('proxy_import', { servers });
      toast.success(`已导入 ${added.length} 个服务器`);
      onImported();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setImporting(false);
    }
  };

  return <Dialog open onOpenChange={open => { if (!open && !importing) onClose(); }}>
    <DialogContent className="max-h-[calc(100vh-4rem)] max-w-xl overflow-y-auto" data-proxy-import>
      <DialogHeader>
        <DialogTitle>从其他客户端导入</DialogTitle>
        <DialogDescription>读取本机 Pi、Claude Desktop、Claude Code、Cursor 与 Codex 的 MCP 配置。同名服务器导入后会加数字后缀。</DialogDescription>
      </DialogHeader>
      {!sources ? <p className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />正在读取配置…</p>
        : sources.length === 0 ? <p className="py-6 text-sm text-muted-foreground">没有在本机找到其他客户端的 MCP 配置。</p>
          : <div className="space-y-4">
            {sources.map(source => <section key={source.source} className="space-y-1.5">
              <h3 className="h1-group-title px-1">{source.source} <span className="font-normal">· {source.path}</span></h3>
              <div className="h1-group">
                {source.error && <p className="h1-list-row text-xs text-destructive">{source.error}</p>}
                {source.servers.length === 0 && !source.error && <p className="h1-list-row text-xs text-muted-foreground">没有可导入的服务器</p>}
                {source.servers.map(server => {
                  const id = key(source.source, server.name);
                  return <label key={id} className="h1-list-row cursor-pointer">
                    <span className="flex min-w-0 items-center gap-3">
                      <input type="checkbox" className="size-4 shrink-0" checked={selected.has(id)} onChange={() => toggle(id)} />
                      <span className="min-w-0">
                        <span className="block text-sm font-medium">{server.name}</span>
                        <span className="settings-help dash-truncate block font-mono">{server.transport === 'stdio' ? [server.command, ...server.args].join(' ') : server.url}</span>
                      </span>
                    </span>
                    <span className="dash-kind">{server.transport === 'stdio' ? 'stdio' : 'HTTP'}</span>
                  </label>;
                })}
                {source.skipped.length > 0 && <p className="h1-list-row text-xs text-muted-foreground">已跳过：{source.skipped.join('、')}</p>}
              </div>
            </section>)}
          </div>}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" disabled={importing} onClick={onClose}>取消</Button>
        <Button type="button" disabled={importing || selected.size === 0} onClick={() => { void run(); }} data-proxy-import-run>
          {importing && <Loader2 className="animate-spin" />}导入所选（{selected.size}）
        </Button>
      </div>
    </DialogContent>
  </Dialog>;
}

export function McpPage() {
  const [servers, setServers] = useState<ProxyServer[] | null>(null);
  const [editing, setEditing] = useState<ProxyServer | null | 'new'>(null);
  const [importing, setImporting] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);
  const [tools, setTools] = useState<Record<string, string[]>>({});

  const refresh = useCallback(async () => {
    if (!desktop && !preview) { setServers([]); return; }
    try {
      setServers(await native<ProxyServer[]>('proxy_list'));
    } catch (error) {
      toast.error(String(error));
      setServers([]);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const toggle = async (server: ProxyServer, enabled: boolean) => {
    try {
      await native('proxy_toggle', { name: server.name, enabled });
      await refresh();
    } catch (error) {
      toast.error(String(error));
    }
  };
  const test = async (server: ProxyServer) => {
    setTesting(server.name);
    try {
      const result = await native<{ tools: string[] }>('proxy_test', { name: server.name });
      setTools(current => ({ ...current, [server.name]: result.tools }));
      toast.success(`${server.name} 连接成功，${result.tools.length} 个工具`);
      await refresh();
    } catch (error) {
      toast.error(`${server.name}：${String(error)}`);
    } finally {
      setTesting(null);
    }
  };
  const remove = async (server: ProxyServer) => {
    if (!window.confirm(`删除 MCP 服务器「${server.name}」？`)) return;
    try {
      await native('proxy_remove', { name: server.name });
      await refresh();
    } catch (error) {
      toast.error(String(error));
    }
  };
  const actions = <div className="flex items-center gap-2">
    <button type="button" className="h1-button" onClick={() => setImporting(true)} data-proxy-import-open>
      <Download size={13} aria-hidden="true" />导入
    </button>
    <button type="button" className="h1-button h1-button-primary" onClick={() => setEditing('new')} data-proxy-add>
      <Plus size={13} aria-hidden="true" />新增
    </button>
  </div>;

  return <>
    <header className="dash-page-heading">
      <div>
        <h1>MCP 转发</h1>
        <p className="dash-page-description">把本机的 MCP 服务器转给远端 Agent，通过 proxy_list、proxy_tools、proxy_call 调用；远端新建会话后生效。</p>
      </div>
      {actions}
    </header>

    {servers === null ? <p className="flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />读取中…</p>
      : servers.length === 0 ? <Empty data-proxy-empty>
        <Plug className="size-7 text-muted-foreground" aria-hidden="true" />
        <EmptyTitle>还没有转发的 MCP 服务器</EmptyTitle>
        <EmptyDescription>可以手动新增，或从 Pi、Claude、Cursor、Codex 的配置导入。</EmptyDescription>
      </Empty>
        : <ul className="h1-group" aria-label="MCP 服务器">
          {servers.map(server => {
            const target = server.transport === 'stdio' ? [server.command, ...server.args].join(' ') : server.url;
            const toolNames = tools[server.name];
            return <li key={server.name} className="proxy-row" data-proxy-server={server.name}>
              <div className="proxy-row-main">
                <div className="flex min-w-0 items-center gap-2">
                  <span className="h1-status-dot" data-tone={server.connected ? 'running' : undefined}
                    title={server.connected ? '已连接' : '未连接'} aria-hidden="true" />
                  <span className="dash-truncate text-sm font-semibold">{server.name}</span>
                  <span className="dash-kind">{server.transport === 'stdio' ? 'stdio' : 'HTTP'}</span>
                  {server.source && <span className="dash-kind">{server.source}</span>}
                </div>
                <code className="proxy-row-target dash-truncate" title={target}>{target}</code>
                {server.description && <p className="settings-help dash-truncate">{server.description}</p>}
                {toolNames && <p className="settings-help wrap-anywhere">工具：{toolNames.length ? toolNames.join('、') : '无'}</p>}
              </div>
              <div className="proxy-row-actions">
                <button type="button" className="h1-tag" disabled={testing === server.name} onClick={() => { void test(server); }} data-proxy-test>
                  {testing === server.name ? <Loader2 size={12} className="animate-spin" aria-hidden="true" /> : <RefreshCw size={12} aria-hidden="true" />}测试
                </button>
                <Button type="button" variant="ghost" size="icon-sm" aria-label={`编辑 ${server.name}`} onClick={() => setEditing(server)}><Pencil /></Button>
                <Button type="button" variant="ghost" size="icon-sm" aria-label={`删除 ${server.name}`} onClick={() => { void remove(server); }}><Trash2 /></Button>
                <Switch checked={server.enabled} aria-label={`启用 ${server.name}`} onCheckedChange={value => { void toggle(server, value); }} />
              </div>
            </li>;
          })}
        </ul>}

    <p className="directory-legend">转发的服务器以你的完整权限运行，不在命令沙箱内；stdio 服务器在首次调用时启动，闲置 10 分钟后关闭。</p>

    {editing && <EditorDialog initial={editing === 'new' ? null : editing} onClose={() => setEditing(null)}
      onSaved={() => { setEditing(null); void refresh(); }} />}
    {importing && servers && <ImportDialog existing={servers} onClose={() => setImporting(false)}
      onImported={() => { setImporting(false); void refresh(); }} />}
  </>;
}
