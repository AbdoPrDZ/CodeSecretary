import { createRootRoute, createRoute, createRouter, redirect } from '@tanstack/react-router';

import { Layout } from './components/Layout';
import { ActivityPage } from './pages/Activity';
import { DevicesPage } from './pages/Devices';
import { ExclusionsPage } from './pages/Exclusions';
import { InboxPage } from './pages/Inbox';
import { ProjectsPage } from './pages/Projects';
import { ReportsPage } from './pages/Reports';
import { TimePage } from './pages/Time';
import { type Period, periodRange } from './period';
import { isoDay } from './timeline';

const rootRoute = createRootRoute({ component: Layout });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  beforeLoad: () => {
    throw redirect({ to: '/time', search: { period: 'day', day: isoDay(new Date()) } });
  },
});

export const activityRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/activity',
  validateSearch: (search: Record<string, unknown>): { day: string } => ({
    day: typeof search.day === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(search.day) ? search.day : isoDay(new Date()),
  }),
  component: ActivityPage,
});

const DAY = /^\d{4}-\d{2}-\d{2}$/;
const PERIODS: Period[] = ['day', 'week', 'month'];

export const timeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/time',
  validateSearch: (search: Record<string, unknown>): { period: Period; day: string } => ({
    period: PERIODS.includes(search.period as Period) ? (search.period as Period) : 'day',
    day: typeof search.day === 'string' && DAY.test(search.day) ? search.day : isoDay(new Date()),
  }),
  component: TimePage,
});

export const reportsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/reports',
  validateSearch: (search: Record<string, unknown>): { start: string; end: string } => {
    const month = periodRange('month', isoDay(new Date()));
    return {
      start: typeof search.start === 'string' && DAY.test(search.start) ? search.start : month.start,
      end: typeof search.end === 'string' && DAY.test(search.end) ? search.end : month.end,
    };
  },
  component: ReportsPage,
});

const inboxRoute = createRoute({ getParentRoute: () => rootRoute, path: '/inbox', component: InboxPage });

const devicesRoute = createRoute({ getParentRoute: () => rootRoute, path: '/devices', component: DevicesPage });
const exclusionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/exclusions',
  component: ExclusionsPage,
});

const projectsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/projects', component: ProjectsPage });

export const router = createRouter({
  routeTree: rootRoute.addChildren([indexRoute, timeRoute, inboxRoute, reportsRoute, activityRoute, projectsRoute, devicesRoute, exclusionsRoute]),
  defaultPreload: 'intent',
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
