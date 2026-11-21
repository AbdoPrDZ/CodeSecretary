// Light / dark / system theme: a class on <html>; the choice is a per-browser
// convenience in localStorage (the page works without it).
import { useEffect, useState } from 'react';

export type Theme = 'system' | 'light' | 'dark';

const KEY = 'secretary.theme';
const media = () => window.matchMedia('(prefers-color-scheme: dark)');

function stored(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

export function applyTheme(theme: Theme = stored()): void {
  const dark = theme === 'dark' || (theme === 'system' && media().matches);
  document.documentElement.classList.toggle('dark', dark);
}

export function useTheme(): [Theme, (t: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(stored);
  useEffect(() => {
    applyTheme(theme);
    if (theme !== 'system') return;
    const m = media();
    const onChange = () => applyTheme('system');
    m.addEventListener('change', onChange);
    return () => m.removeEventListener('change', onChange);
  }, [theme]);
  const set = (t: Theme) => {
    try {
      if (t === 'system') localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, t);
    } catch {
      // Storage blocked: the choice lasts for this page only.
    }
    setTheme(t);
  };
  return [theme, set];
}
