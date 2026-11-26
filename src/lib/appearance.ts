export type Appearance = 'system' | 'light' | 'dark';
export type ColorScheme = 'light' | 'dark';

export const APPEARANCE_KEY = 'cloudmcp.appearance';

export const appearances = [
  { value: 'system', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
] as const;

export function parseAppearance(value: unknown): Appearance {
  return value === 'light' || value === 'dark' ? value : 'system';
}

export function resolveColorScheme(appearance: Appearance, systemDark: boolean): ColorScheme {
  return appearance === 'system' ? (systemDark ? 'dark' : 'light') : appearance;
}
