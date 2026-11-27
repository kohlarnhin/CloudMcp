import type { ProxyServer } from '../types.ts';

export function emptyProxyServer(): ProxyServer {
  return { name: '', enabled: true, transport: 'stdio', command: '', args: [], env: {}, cwd: '', url: '', headers: {}, description: '', source: '' };
}

/** One `key<sep>value` per line; blank lines and lines without the separator are ignored. */
export function linesToMap(text: string, separator: string): Record<string, string> {
  const entries: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const index = line.indexOf(separator);
    if (index <= 0) continue;
    const key = line.slice(0, index).trim();
    if (key) entries[key] = line.slice(index + separator.length).trim();
  }
  return entries;
}

export function mapToLines(entries: Record<string, string>, separator: string): string {
  return Object.entries(entries).map(([key, value]) => key + separator + value).join('\n');
}
