import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import type { Snapshot } from '../types';

export const desktop = isTauri();
export const preview = import.meta.env.DEV && !desktop && new URLSearchParams(location.search).has('preview');

export const initialSnapshot: Snapshot = {
  settings: {
    port: 8787,
    cloudflared_path: 'cloudflared',
    tunnel_mode: 'quick',
    tunnel_name: '',
    public_url: '',
    result_chars: 6000,
    command_timeout_seconds: 3600,
    codex_path: 'codex',
  },
  scopes: [],
  audit: [],
  executions: [],
  server_running: false,
  tunnel_running: false,
  public_url: null,
  tunnel_error: null,
  data_dir: '',
  platform: '',
};

export async function native<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (import.meta.env.DEV && preview) {
    const { previewNative } = await import('./preview');
    return previewNative(command, args) as T;
  }
  if (!desktop) throw new Error('请在 CloudMCP 桌面应用中操作。本页面仅用于界面预览。');
  return invoke<T>(command, args);
}

export async function copyText(value: string) {
  if (desktop) await writeText(value);
  else await navigator.clipboard.writeText(value);
}

export async function setWindowTheme(theme: 'light' | 'dark' | null) {
  if (desktop) await getCurrentWindow().setTheme(theme);
}
