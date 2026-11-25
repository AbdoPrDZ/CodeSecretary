// App shell: sidebar navigation (a drawer on small screens), account menu and
// theme switch around the routed page.
import { useQuery } from '@tanstack/react-query';
import { Link, Outlet, useRouterState } from '@tanstack/react-router';
import { useEffect, useState, type ReactNode } from 'react';

import { api } from '../api';
import { initials } from '../format';
import { useTheme, type Theme } from '../theme';
import { isoDay } from '../timeline';
import { IconCheck, IconClock, IconFolder, IconLaptop, IconLogo, IconLogOut, IconMenu, IconMonitor, IconMoon, IconPulse,
  IconShield, IconSun, IconX } from './icons';
import { Button, cx, Segmented, Skeleton } from './ui';

const NAV: { to: '/time' | '/inbox' | '/activity' | '/projects' | '/devices' | '/exclusions'; label: string; icon: ReactNode; hint: string }[] = [
  { to: '/time', label: 'Time', icon: <IconClock />, hint: 'Time per project' },
  { to: '/inbox', label: 'Inbox', icon: <IconCheck />, hint: 'Unassigned time to review' },
  { to: '/activity', label: 'Activity', icon: <IconPulse />, hint: 'Your day, as collected' },
  { to: '/projects', label: 'Projects', icon: <IconFolder />, hint: 'Projects and rules' },
  { to: '/devices', label: 'Devices', icon: <IconMonitor />, hint: 'Signed-in agents' },
  { to: '/exclusions', label: 'Privacy', icon: <IconShield />, hint: 'Exclusions and pauses' },
];

function navSearch(to: (typeof NAV)[number]['to']) {
  const day = isoDay(new Date());
  if (to === '/activity') return { day };
  if (to === '/time') return { period: 'day' as const, day };
  return undefined;
}

export function Layout() {
  const me = useQuery({ queryKey: ['me'], queryFn: api.me, staleTime: 60_000 });
  const [open, setOpen] = useState(false);
  const path = useRouterState({ select: (s) => s.location.pathname });
  useEffect(() => setOpen(false), [path]);

  if (me.isError) {
    return (
      <div className="flex min-h-screen items-center justify-center p-6">
        <div className="max-w-sm text-center">
          <Logo />
          <p className="mt-6 text-sm text-zinc-500">The dashboard API cannot be reached. Try again in a moment.</p>
          <Button className="mt-4" variant="primary" onClick={() => void me.refetch()}>Retry</Button>
        </div>
      </div>
    );
  }

  return (
    <div className="app-backdrop min-h-screen">
      {/* Mobile top bar */}
      <div className="sticky top-0 z-30 flex items-center gap-3 border-b border-zinc-200/70 bg-white/80 px-4 py-3 backdrop-blur-lg lg:hidden dark:border-zinc-800 dark:bg-zinc-950/80">
        <button type="button" aria-label="Open menu" onClick={() => setOpen(true)}
                className="rounded-lg p-1.5 text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800">
          <IconMenu />
        </button>
        <Logo />
      </div>

      {/* Sidebar */}
      {open && <div className="fixed inset-0 z-40 animate-fade-in bg-zinc-950/40 backdrop-blur-sm lg:hidden" onClick={() => setOpen(false)} />}
      <aside className={cx(
        'fixed inset-y-0 left-0 z-50 flex w-72 flex-col border-r border-zinc-200/70 bg-white/90 backdrop-blur-xl transition-transform duration-200',
        'dark:border-zinc-800/80 dark:bg-zinc-950/90 lg:z-20 lg:w-64 lg:translate-x-0',
        open ? 'translate-x-0' : '-translate-x-full',
      )}>
        <div className="flex items-center justify-between px-5 pt-5 pb-6">
          <Logo />
          <button type="button" aria-label="Close menu" onClick={() => setOpen(false)}
                  className="rounded-lg p-1.5 text-zinc-500 hover:bg-zinc-100 lg:hidden dark:hover:bg-zinc-800">
            <IconX size={16} />
          </button>
        </div>
        <nav className="flex-1 space-y-1 px-3">
          <div className="px-3 pb-2 text-[11px] font-semibold tracking-wider text-zinc-400 uppercase">Workspace</div>
          {NAV.map((item) => (
            <Link key={item.to} to={item.to} search={navSearch(item.to)}
                  className="group flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium text-zinc-600 transition hover:bg-zinc-100 hover:text-zinc-950 dark:text-zinc-400 dark:hover:bg-zinc-800/60 dark:hover:text-white"
                  activeProps={{ className: '!bg-brand-50 !text-brand-700 dark:!bg-brand-500/10 dark:!text-brand-200' }}>
              <span className="text-zinc-400 transition group-hover:text-zinc-600 group-data-[status=active]:text-brand-600 dark:group-data-[status=active]:text-brand-300 dark:group-hover:text-zinc-200">
                {item.icon}
              </span>
              <span className="flex-1">{item.label}</span>
            </Link>
          ))}
          <div className="!mt-6 rounded-2xl bg-gradient-to-br from-brand-500/10 via-fuchsia-500/5 to-transparent p-4 ring-1 ring-brand-500/15">
            <div className="text-xs font-semibold text-brand-700 dark:text-brand-300">Time per project</div>
            <p className="mt-1 text-xs leading-relaxed text-zinc-600 dark:text-zinc-400">
              The engine recalculates a day a few minutes after new activity. Rules you add apply to the last 30 days.
            </p>
          </div>
        </nav>
        <div className="space-y-3 border-t border-zinc-200/70 p-4 dark:border-zinc-800">
          <ThemeSwitch />
          {me.isPending ? (
            <Skeleton className="h-11" />
          ) : (
            <div className="flex items-center gap-3 rounded-xl p-1.5">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-brand-500 to-fuchsia-500 text-xs font-semibold text-white shadow-sm">
                {initials(me.data.display_name || me.data.email)}
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium">{me.data.display_name || me.data.email.split('@')[0]}</div>
                <div className="truncate text-xs text-zinc-500">{me.data.email}</div>
              </div>
              <button type="button" aria-label="Sign out" title="Sign out" onClick={() => void api.logout()}
                      className="rounded-lg p-2 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-900 dark:hover:bg-zinc-800 dark:hover:text-white">
                <IconLogOut size={16} />
              </button>
            </div>
          )}
        </div>
      </aside>

      <main className="lg:pl-64">
        <div className="mx-auto max-w-6xl px-4 py-8 sm:px-6 lg:px-10 lg:py-12">
          {me.isPending ? <PageSkeleton /> : <div key={path} className="animate-pop-in"><Outlet /></div>}
        </div>
      </main>
    </div>
  );
}

function Logo() {
  return (
    <div className="flex items-center gap-2.5">
      <div className="flex size-8 items-center justify-center rounded-xl bg-gradient-to-br from-brand-500 to-fuchsia-500 text-white shadow-md shadow-brand-500/30">
        <IconLogo size={18} />
      </div>
      <span className="text-[15px] font-semibold tracking-tight">
        Code<span className="text-brand-600 dark:text-brand-400">Secretary</span>
      </span>
    </div>
  );
}

function ThemeSwitch() {
  const [theme, setTheme] = useTheme();
  return (
    <div className="flex items-center justify-between">
      <span className="text-xs font-medium text-zinc-500">Theme</span>
      <Segmented<Theme> value={theme} onChange={setTheme} options={[
        { value: 'light', label: <IconSun size={14} />, title: 'Light' },
        { value: 'system', label: <IconLaptop size={14} />, title: 'System' },
        { value: 'dark', label: <IconMoon size={14} />, title: 'Dark' },
      ]} />
    </div>
  );
}

function PageSkeleton() {
  return (
    <div className="space-y-6">
      <Skeleton className="h-9 w-64" />
      <Skeleton className="h-4 w-96 max-w-full" />
      <div className="grid gap-4 sm:grid-cols-3">
        <Skeleton className="h-24" />
        <Skeleton className="h-24" />
        <Skeleton className="h-24" />
      </div>
      <Skeleton className="h-64" />
    </div>
  );
}
