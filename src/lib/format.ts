export function shortPath(path: string) {
  return path.replace(/^\/Users\/[^/]+/, '~').replace(/^\/home\/[^/]+/, '~');
}

const dateTimeFormat = new Intl.DateTimeFormat('zh-CN', {
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
});

export function dateTime(value: string) {
  return dateTimeFormat.format(new Date(value));
}

const clockFormat = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
});

export function clockTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '--:--:--' : clockFormat.format(date);
}

export function elapsed(start: string, end: string | null = null, now = Date.now()) {
  const from = new Date(start).getTime();
  const to = end ? new Date(end).getTime() : now;
  if (Number.isNaN(from) || Number.isNaN(to)) return '';
  const seconds = Math.max(0, Math.round((to - from) / 1000));
  if (seconds < 60) return seconds + ' 秒';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes + ' 分钟';
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} 小时 ${rest} 分钟` : `${hours} 小时`;
}
