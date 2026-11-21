// Display formatting only (never sums durations: time is the engine's).

export function formatTime(iso: string | null, seconds = true): string {
  if (!iso) return '-';
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', ...(seconds ? { second: '2-digit' } : {}) });
}

/** `HH:MM` on a 24 h clock (compact, for ranges). */
export function clock(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
}

export function relative(iso: string | null): string {
  if (!iso) return 'never';
  const seconds = Math.round((Date.now() - Date.parse(iso)) / 1000);
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h ago`;
  if (seconds < 7 * 86_400) return `${Math.floor(seconds / 86_400)} d ago`;
  return new Date(iso).toLocaleDateString();
}

export function longDay(day: string): string {
  const [y, m, d] = day.split('-').map(Number);
  return new Date(y!, m! - 1, d!).toLocaleDateString([], { weekday: 'long', month: 'long', day: 'numeric', year: 'numeric' });
}

export function initials(nameOrEmail: string): string {
  const base = nameOrEmail.split('@')[0] ?? nameOrEmail;
  const parts = base.split(/[\s._-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? '?') + (parts[1]?.[0] ?? '')).toUpperCase();
}

export function plural(n: number, word: string): string {
  return `${n.toLocaleString()} ${word}${n === 1 ? '' : 's'}`;
}
