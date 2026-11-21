// Dashboard API client (overview/api). Same origin: the session is an
// HTTP-only cookie; mutations send the CSRF token from /api/me.
import type { EventType } from './generated/contract';

export interface Me {
  email: string;
  display_name: string | null;
  time_zone: string;
  csrf_token: string;
}

export interface Device {
  id: number;
  name: string;
  os: string;
  agent_version: string | null;
  queue_depth: number | null;
  clock_offset_ms: number;
  last_seen_at: string | null;
  revoked_at: string | null;
  created_at: string;
}

export interface ActivityEvent {
  event_id: string;
  device_id: number;
  event_type: EventType;
  source: string;
  start: string;
  end: string | null;
  project_hint: string | null;
  repository_remote: string | null;
  metadata: Record<string, unknown>;
}

export interface Activity {
  day: string;
  tz: string;
  range: { start: string; end: string };
  devices: Pick<Device, 'id' | 'name' | 'os' | 'revoked_at'>[];
  events: ActivityEvent[];
  truncated: boolean;
}

export type ExclusionKind = 'path' | 'app' | 'domain' | 'time_window';

export interface Exclusion {
  id: number;
  kind: ExclusionKind;
  pattern: string | null;
  start: string | null;
  end: string | null;
  days: number[];
  created_at: string;
}

export type RuleSignal = 'path' | 'remote' | 'domain' | 'app' | 'title';

export interface Rule {
  id: number;
  project_id: number;
  signal: RuleSignal;
  pattern: string;
  priority: number;
  shared: boolean;
  created_at: string;
}

export interface Project {
  id: number;
  workspace_id: number;
  name: string;
  color: string | null;
  client_name: string | null;
  billable: boolean;
  hourly_rate: string | null;
  currency: string | null;
  archived_at: string | null;
  created_at: string;
  rules: Rule[];
}

export interface ProjectFields {
  name?: string;
  color?: string | null;
  client_name?: string | null;
  billable?: boolean;
  hourly_rate?: number | null;
  currency?: string | null;
  archived?: boolean;
}

export interface RulePreview {
  signal: RuleSignal;
  pattern: string;
  days: number;
  matched_events: number;
  distinct_values: number;
  examples: { value: string; events: number; last_seen: string | null }[];
  conflicts: { project_id: number; name: string | null; events: number }[];
  truncated: boolean;
}

export interface RepositorySuggestion {
  remote: string;
  root: string | null;
  name: string;
  events: number;
  last_seen: string | null;
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(code);
  }
}

let csrfToken: string | null = null;

export function signIn(): never {
  const next = window.location.pathname + window.location.search;
  window.location.assign(`/auth/login?next=${encodeURIComponent(next)}`);
  throw new ApiError(401, 'signed_out');
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (method !== 'GET') headers['X-CSRF-Token'] = csrfToken ?? '';
  const response = await fetch(path, {
    method,
    headers,
    credentials: 'same-origin',
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (response.status === 401) signIn();
  const data = await response.json().catch(() => ({}));
  if (!response.ok) throw new ApiError(response.status, (data as { error?: string }).error ?? 'error');
  return data as T;
}

export const api = {
  async me(): Promise<Me> {
    const me = await request<Me>('GET', '/api/me');
    csrfToken = me.csrf_token;
    return me;
  },
  devices: () => request<{ devices: Device[] }>('GET', '/api/devices').then((r) => r.devices),
  renameDevice: (id: number, name: string) => request('PATCH', `/api/devices/${id}`, { name }),
  revokeDevice: (id: number) => request('POST', `/api/devices/${id}/revoke`),
  activity: (day: string, tz: string) =>
    request<Activity>('GET', `/api/activity?day=${encodeURIComponent(day)}&tz=${encodeURIComponent(tz)}`),
  exclusions: () => request<{ exclusions: Exclusion[] }>('GET', '/api/exclusions').then((r) => r.exclusions),
  addExclusion: (body: Partial<Exclusion>) => request<Exclusion>('POST', '/api/exclusions', body),
  deleteExclusion: (id: number) => request('DELETE', `/api/exclusions/${id}`),
  projects: () => request<{ projects: Project[] }>('GET', '/api/projects').then((r) => r.projects),
  addProject: (body: ProjectFields) => request<Project>('POST', '/api/projects', body),
  editProject: (id: number, body: ProjectFields) => request('PATCH', `/api/projects/${id}`, body),
  deleteProject: (id: number) => request('DELETE', `/api/projects/${id}`),
  addRule: (projectId: number, body: { signal: RuleSignal; pattern: string; priority?: number }) =>
    request<Rule>('POST', `/api/projects/${projectId}/rules`, body),
  deleteRule: (id: number) => request('DELETE', `/api/rules/${id}`),
  previewRule: (body: { signal: RuleSignal; pattern: string; project_id?: number }) =>
    request<RulePreview>('POST', '/api/rules/preview', body),
  suggestions: () =>
    request<{ repositories: RepositorySuggestion[] }>('GET', '/api/rules/suggestions').then((r) => r.repositories),
  async logout(): Promise<void> {
    const { logout_url } = await request<{ logout_url: string }>('POST', '/auth/logout');
    window.location.assign(logout_url);
  },
};
