// Calendar periods for the Time page (local days as `YYYY-MM-DD`).
import { isoDay } from './timeline';

export type Period = 'day' | 'week' | 'month';

function parse(day: string): Date {
  const [y, m, d] = day.split('-').map(Number);
  return new Date(y!, m! - 1, d!);
}

/** First and last local day of the period containing `day` (weeks start on Monday). */
export function periodRange(period: Period, day: string): { start: string; end: string } {
  const d = parse(day);
  if (period === 'day') return { start: day, end: day };
  if (period === 'week') {
    const offset = (d.getDay() + 6) % 7;
    const start = new Date(d.getFullYear(), d.getMonth(), d.getDate() - offset);
    const end = new Date(start.getFullYear(), start.getMonth(), start.getDate() + 6);
    return { start: isoDay(start), end: isoDay(end) };
  }
  return {
    start: isoDay(new Date(d.getFullYear(), d.getMonth(), 1)),
    end: isoDay(new Date(d.getFullYear(), d.getMonth() + 1, 0)),
  };
}

/** The same position one period earlier (-1) or later (+1). */
export function shiftPeriod(period: Period, day: string, by: number): string {
  const d = parse(day);
  if (period === 'day') return isoDay(new Date(d.getFullYear(), d.getMonth(), d.getDate() + by));
  if (period === 'week') return isoDay(new Date(d.getFullYear(), d.getMonth(), d.getDate() + 7 * by));
  return isoDay(new Date(d.getFullYear(), d.getMonth() + by, 1));
}

export function periodTitle(period: Period, day: string, today: string): string {
  const { start, end } = periodRange(period, day);
  const fmt = (s: string, opts: Intl.DateTimeFormatOptions) => parse(s).toLocaleDateString([], opts);
  if (period === 'day') {
    if (day === today) return 'Today';
    return fmt(day, { weekday: 'long', month: 'long', day: 'numeric' });
  }
  if (period === 'week') {
    if (start <= today && today <= end) return 'This week';
    return `${fmt(start, { month: 'short', day: 'numeric' })} - ${fmt(end, { month: 'short', day: 'numeric', year: 'numeric' })}`;
  }
  if (start <= today && today <= end) return 'This month';
  return fmt(start, { month: 'long', year: 'numeric' });
}

export function weekday(day: string): string {
  return parse(day).toLocaleDateString([], { weekday: 'short' });
}

export function dayOfMonth(day: string): number {
  return parse(day).getDate();
}

/** A project's color, or a stable fallback from its id. */
const FALLBACK = ['#6366f1', '#0ea5e9', '#10b981', '#f59e0b', '#ef4444', '#a855f7', '#14b8a6', '#f97316'];
export function projectColor(id: number | null, color: string | null | undefined): string {
  if (id === null) return '#a1a1aa';
  if (color) return color;
  return FALLBACK[Math.abs(id) % FALLBACK.length]!;
}
