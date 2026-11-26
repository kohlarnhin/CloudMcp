import type { PortalAccount, PortalStatus } from '../types';

export const PORTAL_POLL_MS = 30_000;

export type QuotaTone = 'ok' | 'low' | 'empty';

/** 剩余额度的颜色档位：用完 / 不足 20% / 正常。 */
export function quotaTone(percent: number): QuotaTone {
  if (percent <= 0) return 'empty';
  return percent < 20 ? 'low' : 'ok';
}

export function clampPercent(percent: number) {
  return Number.isFinite(percent) ? Math.min(100, Math.max(0, Math.round(percent))) : 0;
}

/** "2 小时 5 分钟后" / "3 分钟后" / "即将"；无效时间返回空串。 */
export function resetIn(value: string | null, now = Date.now()) {
  if (!value) return '';
  const at = new Date(value).getTime();
  if (Number.isNaN(at)) return '';
  const minutes = Math.ceil((at - now) / 60_000);
  if (minutes <= 0) return '即将';
  if (minutes < 60) return minutes + ' 分钟后';
  const hours = Math.floor(minutes / 60);
  if (hours < 48) {
    const rest = minutes % 60;
    return rest ? `${hours} 小时 ${rest} 分钟后` : `${hours} 小时后`;
  }
  return Math.round(hours / 24) + ' 天后';
}

/** 额度行下方的一句说明：当前账号、接替账号或全部用完的恢复时间。 */
export function portalDetail(status: PortalStatus, now = Date.now()) {
  if (status.allExhausted) {
    const recover = resetIn(status.earliestRecoveryAt, now);
    return recover ? `全部账号额度已用完 · ${recover}恢复` : '全部账号额度已用完';
  }
  if (!status.current) return '未选择当前账号';
  const parts = [`当前 ${status.current.name}`];
  if (!status.current.usable) parts.push('额度已用完');
  if (status.next) parts.push(`接替 ${status.next.name}`);
  return parts.join(' · ');
}

export function portalTone(status: PortalStatus): 'running' | 'error' | undefined {
  if (status.allExhausted || (status.current && !status.current.usable)) return 'error';
  return status.current ? 'running' : undefined;
}

export function quotaTitle(label: string, account: PortalAccount, kind: 'short' | 'long', now = Date.now()) {
  const percent = clampPercent(kind === 'short' ? account.shortRemainingPercent : account.longRemainingPercent);
  const reset = resetIn(kind === 'short' ? account.shortResetsAt : account.longResetsAt, now);
  return `${label}剩余 ${percent}%` + (reset ? ` · ${reset}重置` : '');
}
