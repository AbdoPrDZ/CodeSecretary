import { useQuery } from '@tanstack/react-query';
import { Link, Outlet } from '@tanstack/react-router';

import { api } from '../api';
import { isoDay } from '../timeline';

const linkClass =
  'rounded-md px-3 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-200/60 hover:text-slate-900 ' +
  'dark:text-slate-300 dark:hover:bg-slate-800 dark:hover:text-white';
const activeClass = 'bg-slate-200 text-slate-900 dark:bg-slate-800 dark:text-white';

export function Layout() {
  const me = useQuery({ queryKey: ['me'], queryFn: api.me, staleTime: 60_000 });

  if (me.isPending) {
    return <div className="p-8 text-sm text-slate-500">Loading…</div>;
  }
  if (me.isError) {
    return <div className="p-8 text-sm text-red-600">Could not reach the dashboard API.</div>;
  }

  return (
    <div className="min-h-screen">
      <header className="border-b border-slate-200 bg-white/80 backdrop-blur dark:border-slate-800 dark:bg-slate-900/80">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center gap-2 px-4 py-3">
          <span className="mr-4 font-semibold tracking-tight">CodeSecretary</span>
          <nav className="flex flex-wrap gap-1">
            <Link to="/activity" search={{ day: isoDay(new Date()) }} className={linkClass}
                  activeProps={{ className: activeClass }}>
              Activity
            </Link>
            <Link to="/devices" className={linkClass} activeProps={{ className: activeClass }}>
              Devices
            </Link>
            <Link to="/exclusions" className={linkClass} activeProps={{ className: activeClass }}>
              Exclusions
            </Link>
          </nav>
          <div className="ml-auto flex items-center gap-3 text-sm">
            <span className="text-slate-500 dark:text-slate-400">{me.data.email}</span>
            <button type="button" onClick={() => void api.logout()}
                    className="rounded-md border border-slate-300 px-3 py-1.5 hover:bg-slate-100 dark:border-slate-700 dark:hover:bg-slate-800">
              Sign out
            </button>
          </div>
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 py-6">
        <Outlet />
      </main>
    </div>
  );
}

export function Card({ title, children, actions }: { title: string; children: React.ReactNode; actions?: React.ReactNode }) {
  return (
    <section className="mb-6 rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <h2 className="text-base font-semibold">{title}</h2>
        <div className="ml-auto flex items-center gap-2">{actions}</div>
      </div>
      {children}
    </section>
  );
}

export function formatTime(iso: string | null): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function relative(iso: string | null): string {
  if (!iso) return 'never';
  const seconds = Math.round((Date.now() - Date.parse(iso)) / 1000);
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h ago`;
  return new Date(iso).toLocaleString();
}

export const buttonClass =
  'rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100 disabled:opacity-50 ' +
  'dark:border-slate-700 dark:hover:bg-slate-800';
export const primaryButtonClass =
  'rounded-md bg-slate-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-50 ' +
  'dark:bg-white dark:text-slate-900 dark:hover:bg-slate-200';
export const inputClass =
  'rounded-md border border-slate-300 bg-white px-2 py-1.5 text-sm dark:border-slate-700 dark:bg-slate-950';
