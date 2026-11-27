import { cn } from '../lib/utils';

/** CloudMCP 应用图标（与 app-icon.svg 一致），用于侧栏品牌区。 */
export function Logo({ className }: { className?: string }) {
  return <svg viewBox="20 20 472 472" className={cn('block', className)} aria-hidden="true">
    <rect x="20" y="20" width="472" height="472" rx="116" fill="#242423" />
    <path d="M339 171a118 118 0 1 0 0 170" fill="none" stroke="#faf9f7" strokeWidth="42" strokeLinecap="round" />
    <path d="M260 256h106" stroke="#faf9f7" strokeWidth="32" strokeLinecap="round" />
    <circle cx="260" cy="256" r="24" fill="#faf9f7" />
    <circle cx="371" cy="256" r="25" fill="#8dab9c" />
  </svg>;
}
