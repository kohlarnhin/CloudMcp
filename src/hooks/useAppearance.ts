import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import { APPEARANCE_KEY, parseAppearance, resolveColorScheme, type Appearance } from '../lib/appearance';
import { setWindowTheme } from '../lib/desktop';

function readAppearance(): Appearance {
  try { return parseAppearance(localStorage.getItem(APPEARANCE_KEY)); }
  catch { return 'system'; }
}

function saveAppearance(appearance: Appearance) {
  try { localStorage.setItem(APPEARANCE_KEY, appearance); return true; }
  catch { return false; }
}

export function useAppearance() {
  const [appearance, setAppearance] = useState(readAppearance);
  const [systemDark, setSystemDark] = useState(() => window.matchMedia('(prefers-color-scheme: dark)').matches);
  const colorScheme = resolveColorScheme(appearance, systemDark);

  useEffect(() => {
    const scheme = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => setSystemDark(scheme.matches);
    scheme.addEventListener('change', update);
    update();
    return () => scheme.removeEventListener('change', update);
  }, []);

  useLayoutEffect(() => {
    const root = document.documentElement;
    root.classList.toggle('dark', colorScheme === 'dark');
    root.classList.toggle('light', colorScheme === 'light');
  }, [colorScheme]);

  useEffect(() => {
    void setWindowTheme(appearance === 'system' ? null : appearance)
      .catch(error => console.warn('CloudMCP appearance:', error));
  }, [appearance]);

  const changeAppearance = useCallback((next: Appearance) => {
    setAppearance(next);
    saveAppearance(next);
  }, []);

  return { appearance, colorScheme, changeAppearance };
}

export type AppearanceController = ReturnType<typeof useAppearance>;
