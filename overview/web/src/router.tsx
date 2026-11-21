import { createRootRoute, createRoute, createRouter, redirect } from '@tanstack/react-router';

import { Layout } from './components/Layout';
import { ActivityPage } from './pages/Activity';
import { DevicesPage } from './pages/Devices';
import { ExclusionsPage } from './pages/Exclusions';
import { isoDay } from './timeline';

const rootRoute = createRootRoute({ component: Layout });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  beforeLoad: () => {
    throw redirect({ to: '/activity', search: { day: isoDay(new Date()) } });
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

const devicesRoute = createRoute({ getParentRoute: () => rootRoute, path: '/devices', component: DevicesPage });
const exclusionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/exclusions',
  component: ExclusionsPage,
});

export const router = createRouter({
  routeTree: rootRoute.addChildren([indexRoute, activityRoute, devicesRoute, exclusionsRoute]),
  defaultPreload: 'intent',
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
