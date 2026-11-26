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

/** Engine queue state for the requested days ("recalculating"). */
export interface JobState {
  pending_days: number;
  failed_days: number;
  calculated_at: string | null;
}

export interface ProjectTime {
  project_id: number;
  name: string | null;
  color: string | null;
  archived: boolean;
  seconds: number;
  sessions: number;
  days: number;
}

/** Totals the API adds up from the engine's sessions (display only here). */
export interface TimeSummary extends JobState {
  start: string;
  end: string;
  time_zone: string;
  total_seconds: number;
  assigned_seconds: number;
  unassigned_seconds: number;
  unassigned_sessions?: number;
  unassigned_days?: number;
  projects: ProjectTime[];
  days: {
    day: string;
    total_seconds: number;
    unassigned_seconds: number;
    projects: { project_id: number; seconds: number }[];
  }[];
}

export interface WorkSession {
  id: number;
  project_id: number | null;
  project_name: string | null;
  project_color: string | null;
  kind: 'active' | 'waiting';
  start: string;
  end: string;
  seconds: number;
  low_confidence: boolean;
  estimated: boolean;
  manual: boolean;
  evidence: number;
}

export interface DaySessions extends JobState {
  day: string;
  time_zone: string;
  range: { start: string; end: string };
  sessions: WorkSession[];
}

export type AssignMethod = 'user' | 'rule' | 'shared_rule' | 'structural' | 'temporal' | 'ai';

export interface EvidenceEvent {
  event_id: string;
  device_id: number;
  device_name: string | null;
  event_type: EventType;
  start: string;
  end: string | null;
  repository_root: string | null;
  repository_remote: string | null;
  working_directory: string | null;
  metadata: Record<string, unknown>;
  assigned_project_id: number | null;
  method: AssignMethod | null;
  confidence: number | null;
  rule_id: number | null;
  overridden: boolean;
}

export interface SessionDetail {
  session: WorkSession & { local_day: string; algorithm_version: number };
  events: EvidenceEvent[];
  evidence_total: number;
  truncated: boolean;
  methods: Partial<Record<AssignMethod | 'unassigned', number>>;
  rules: { id: number; project_id: number; signal: RuleSignal; pattern: string }[];
}

export type AdjustmentKind = 'add' | 'remove' | 'reassign';

export interface Adjustment {
  id: number;
  local_day: string;
  kind: AdjustmentKind;
  starts_at: string;
  ends_at: string;
  project_id: number | null;
  project_name: string | null;
  from_project_id: number | null;
  from_project_name: string | null;
  note: string | null;
  created_at: string;
}

export interface NewAdjustment {
  kind: AdjustmentKind;
  start: string;
  end: string;
  project_id?: number | null;
  from_project_id?: number | null;
  note?: string;
}

export interface InboxGroup {
  signal: 'path' | 'app' | 'other';
  value: string;
  seconds: number;
  titles: string[];
  sessions: { id: number; day: string; start: string; end: string; seconds: number }[];
}

export interface Inbox extends JobState {
  groups: InboxGroup[];
  total_seconds: number;
  sessions: number;
  truncated: boolean;
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

export type PrivacyScope = 'day' | 'device' | 'all';

/** Download of all the user's raw events (NDJSON). */
export const PRIVACY_EXPORT_URL = '/api/privacy/export.ndjson';

/** Which projects a report covers: `projects` null = all; `unassigned` adds time no rule matched. */
export interface ReportFilter {
  projects: number[] | null;
  unassigned: boolean;
}

export type ReportGroup = 'project' | 'day' | 'session';

function reportQuery(start: string, end: string, filter: ReportFilter, group?: ReportGroup): string {
  const q = new URLSearchParams({ start, end, unassigned: String(filter.unassigned) });
  if (group) q.set('group', group);
  // An empty selection still has to filter: -1 matches no project.
  if (filter.projects) for (const id of filter.projects.length ? filter.projects : [-1]) q.append('project', String(id));
  return q.toString();
}

/** A download link for the report as CSV (same totals as `api.report`). */
export function reportCsvUrl(start: string, end: string, filter: ReportFilter, group: ReportGroup): string {
  return `/api/reports/export.csv?${reportQuery(start, end, filter, group)}`;
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
  setTimeZone: (time_zone: string) => request<{ time_zone: string }>('PUT', '/api/me/time_zone', { time_zone }),
  privacyDelete: (body: { scope: PrivacyScope; day?: string; device_id?: number; confirm: true }) =>
    request<{ deleted_events: number; queued_days: number }>('POST', '/api/privacy/delete', body),
  report: (start: string, end: string, filter: ReportFilter) =>
    request<TimeSummary>('GET', `/api/reports?${reportQuery(start, end, filter)}`),
  time: (start: string, end: string) =>
    request<TimeSummary>('GET', `/api/time?start=${encodeURIComponent(start)}&end=${encodeURIComponent(end)}`),
  sessions: (day: string) => request<DaySessions>('GET', `/api/sessions?day=${encodeURIComponent(day)}`),
  session: (id: number) => request<SessionDetail>('GET', `/api/sessions/${id}`),
  assign: (body: { event_ids?: string[]; session_ids?: number[]; project_id?: number | null; clear?: boolean }) =>
    request<{ events: number }>('POST', '/api/assign', body),
  adjustments: (day: string) =>
    request<{ adjustments: Adjustment[] }>('GET', `/api/adjustments?day=${encodeURIComponent(day)}`).then((r) => r.adjustments),
  addAdjustment: (body: NewAdjustment) => request<{ ids: number[] }>('POST', '/api/adjustments', body),
  undoAdjustment: (id: number) => request('DELETE', `/api/adjustments/${id}`),
  inbox: (start: string, end: string) =>
    request<Inbox>('GET', `/api/inbox?start=${encodeURIComponent(start)}&end=${encodeURIComponent(end)}`),
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
