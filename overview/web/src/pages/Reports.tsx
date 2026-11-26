// Reports (OV-11): the engine's time per project over any range up to a year,
// filtered by project, with CSV export. Every total here comes from the API
// (`/api/reports`), and the CSV is built from the same rows, so both match.
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { api, type ReportFilter, type ReportGroup, reportCsvUrl, type TimeSummary } from '../api';
import { IconCalendar, IconCheck, IconClock, IconFolder, IconLayers } from '../components/icons';
import { Button, Card, cx, EmptyState, Field, Input, Notice, PageHeader, Segmented, Skeleton, Stat } from '../components/ui';
import { duration, longDay, plural } from '../format';
import { periodRange, projectColor } from '../period';
import { reportsRoute } from '../router';
import { isoDay, shiftDay } from '../timeline';

const MAX_DAYS = 366;

function presets(today: string) {
  const lastWeek = periodRange('week', shiftDay(today, -7));
  const month = periodRange('month', today);
  const lastMonth = periodRange('month', shiftDay(month.start, -1));
  return [
    { key: 'week', label: 'This week', ...periodRange('week', today) },
    { key: 'last-week', label: 'Last week', ...lastWeek },
    { key: 'month', label: 'This month', ...month },
    { key: 'last-month', label: 'Last month', ...lastMonth },
    { key: 'year', label: 'This year', start: `${today.slice(0, 4)}-01-01`, end: `${today.slice(0, 4)}-12-31` },
  ];
}

function days(start: string, end: string): number {
  return Math.round((Date.parse(end) - Date.parse(start)) / 86_400_000) + 1;
}

export function ReportsPage() {
  const { start, end } = reportsRoute.useSearch();
  const navigate = useNavigate({ from: '/reports' });
  const today = isoDay(new Date());
  const ranges = presets(today);
  const valid = start <= end && days(start, end) <= MAX_DAYS;

  const projects = useQuery({ queryKey: ['projects'], queryFn: api.projects, staleTime: 60_000 });
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  const [unassigned, setUnassigned] = useState(true);
  const [group, setGroup] = useState<ReportGroup>('project');
  const filter: ReportFilter = useMemo(() => ({
    projects: hidden.size === 0 ? null : (projects.data ?? []).map((p) => p.id).filter((id) => !hidden.has(id)),
    unassigned,
  }), [hidden, unassigned, projects.data]);

  const report = useQuery({
    queryKey: ['report', start, end, filter],
    queryFn: () => api.report(start, end, filter),
    enabled: valid,
    refetchInterval: (q) => ((q.state.data?.pending_days ?? 0) > 0 ? 5_000 : false),
  });
  const data = report.data;
  const toggle = (id: number) => setHidden((h) => {
    const next = new Set(h);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  return (
    <>
      <PageHeader eyebrow="Reports" title="Time report"
                  description="Totals per project for any range up to a year. The CSV holds the same rows and totals as this page."
                  actions={
                    // The response is an attachment, so the page stays where it is.
                    <Button variant="primary" disabled={!valid}
                            onClick={() => window.location.assign(reportCsvUrl(start, end, filter, group))}>
                      Download CSV
                    </Button>
                  } />

      <Card className="mb-6" bodyClassName="space-y-5">
        <div className="flex flex-wrap items-end gap-4">
          <div className="flex flex-wrap gap-1.5">
            {ranges.map((r) => (
              <Button key={r.key} size="sm" variant={r.start === start && r.end === end ? 'subtle' : 'ghost'}
                      onClick={() => void navigate({ search: { start: r.start, end: r.end } })}>
                {r.label}
              </Button>
            ))}
          </div>
          <div className="flex flex-wrap items-end gap-3">
            <Field label="From" className="w-40">
              {(id) => <Input id={id} type="date" value={start} max={end}
                              onChange={(e) => e.target.value && void navigate({ search: (s) => ({ ...s, start: e.target.value }) })} />}
            </Field>
            <Field label="To" className="w-40">
              {(id) => <Input id={id} type="date" value={end} min={start}
                              onChange={(e) => e.target.value && void navigate({ search: (s) => ({ ...s, end: e.target.value }) })} />}
            </Field>
          </div>
          <div className="ml-auto">
            <Field label="CSV rows">
              {() => <Segmented<ReportGroup> value={group} onChange={setGroup}
                                             options={[{ value: 'project', label: 'Per project' }, { value: 'day', label: 'Per day' },
                                               { value: 'session', label: 'Per session' }]} />}
            </Field>
          </div>
        </div>

        <div>
          <p className="mb-2 text-xs font-medium text-zinc-600 dark:text-zinc-400">Projects</p>
          <div className="flex flex-wrap gap-2">
            {(projects.data ?? []).map((p) => (
              <Chip key={p.id} on={!hidden.has(p.id)} color={projectColor(p.id, p.color)} onClick={() => toggle(p.id)}>
                {p.name}{p.archived_at && <span className="ml-1 text-zinc-400">archived</span>}
              </Chip>
            ))}
            <Chip on={unassigned} color={projectColor(null, null)} onClick={() => setUnassigned((u) => !u)}>Unassigned</Chip>
            {(hidden.size > 0 || !unassigned) && (
              <Button size="sm" variant="ghost" onClick={() => { setHidden(new Set()); setUnassigned(true); }}>Show all</Button>
            )}
          </div>
        </div>
      </Card>

      {!valid && <Notice tone="warn">Pick a range of at most {MAX_DAYS} days, with the start before the end.</Notice>}
      {report.isError && <Notice tone="error">Could not load the report.</Notice>}
      {(data?.pending_days ?? 0) > 0 && (
        <div className="mb-4"><Notice>Recalculating {plural(data!.pending_days, 'day')}. Totals update by themselves.</Notice></div>
      )}

      {valid && (
        <div className="mb-6 grid grid-cols-2 gap-4 lg:grid-cols-4">
          {report.isPending ? [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-[104px] rounded-2xl" />) : (
            <>
              <Stat icon={<IconClock size={14} />} label="Total" value={duration(data?.total_seconds ?? 0)} hint={`${start} - ${end}`} />
              <Stat icon={<IconFolder size={14} />} label="On projects" value={duration(data?.assigned_seconds ?? 0)}
                    hint={plural(data?.projects.length ?? 0, 'project')} />
              <Stat icon={<IconLayers size={14} />} label="Unassigned" value={duration(data?.unassigned_seconds ?? 0)}
                    hint={unassigned ? 'no rule matched' : 'left out'} />
              <Stat icon={<IconCalendar size={14} />} label="Days with time"
                    value={data?.days.filter((d) => d.total_seconds > 0).length ?? 0} hint={`of ${days(start, end)}`} />
            </>
          )}
        </div>
      )}

      {data && data.total_seconds === 0 && (
        <Card><EmptyState icon={<IconCheck />} title="No time in this range">Pick another range or show more projects.</EmptyState></Card>
      )}
      {data && data.total_seconds > 0 && (
        <div className="grid gap-6 xl:grid-cols-5">
          <div className="xl:col-span-3"><ProjectTable data={data} /></div>
          <div className="xl:col-span-2"><DayTable data={data} /></div>
        </div>
      )}
    </>
  );
}

function Chip({ on, color, onClick, children }: { on: boolean; color: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" aria-pressed={on} onClick={onClick}
            className={cx('inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium ring-1 transition',
              on ? 'bg-white text-zinc-800 ring-zinc-300 dark:bg-zinc-800 dark:text-zinc-100 dark:ring-zinc-700'
                : 'bg-transparent text-zinc-400 ring-zinc-200 line-through dark:ring-zinc-800')}>
      <span className="size-2 rounded-full" style={{ background: on ? color : 'transparent', boxShadow: `inset 0 0 0 1.5px ${color}` }} />
      {children}
    </button>
  );
}

function ProjectTable({ data }: { data: TimeSummary }) {
  const rows = [
    ...data.projects.map((p) => ({ key: String(p.project_id), id: p.project_id as number | null, name: p.name ?? 'Deleted project',
      color: p.color, seconds: p.seconds, sessions: p.sessions, days: p.days })),
    ...(data.unassigned_seconds > 0
      ? [{ key: 'none', id: null, name: 'Unassigned', color: null, seconds: data.unassigned_seconds,
          sessions: data.unassigned_sessions ?? 0, days: data.unassigned_days ?? 0 }]
      : []),
  ];
  return (
    <Card title="By project" bodyClassName="p-0">
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-zinc-100 text-left text-xs text-zinc-500 dark:border-zinc-800">
              <th className="px-5 py-2.5 font-medium">Project</th>
              <th className="px-3 py-2.5 text-right font-medium">Sessions</th>
              <th className="px-3 py-2.5 text-right font-medium">Days</th>
              <th className="px-3 py-2.5 text-right font-medium">Time</th>
              <th className="px-5 py-2.5 text-right font-medium">Share</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.key} className="border-b border-zinc-50 last:border-0 dark:border-zinc-800/60">
                <td className="px-5 py-2.5">
                  <span className="flex items-center gap-2">
                    <span className="size-2.5 shrink-0 rounded-full" style={{ background: projectColor(r.id, r.color) }} />
                    <span className={cx('truncate font-medium', r.id === null && 'text-zinc-500')}>{r.name}</span>
                  </span>
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums text-zinc-500">{r.sessions}</td>
                <td className="px-3 py-2.5 text-right tabular-nums text-zinc-500">{r.days}</td>
                <td className="px-3 py-2.5 text-right font-semibold tabular-nums">{duration(r.seconds)}</td>
                <td className="px-5 py-2.5 text-right tabular-nums text-zinc-400">
                  {Math.round((r.seconds / Math.max(data.total_seconds, 1)) * 100)}%
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="border-t border-zinc-200 font-semibold dark:border-zinc-700">
              <td className="px-5 py-2.5">Total</td>
              <td /><td />
              <td className="px-3 py-2.5 text-right tabular-nums">{duration(data.total_seconds)}</td>
              <td />
            </tr>
          </tfoot>
        </table>
      </div>
    </Card>
  );
}

function DayTable({ data }: { data: TimeSummary }) {
  const rows = data.days.filter((d) => d.total_seconds > 0);
  const max = Math.max(...rows.map((d) => d.total_seconds), 1);
  const names = new Map(data.projects.map((p) => [p.project_id, p]));
  return (
    <Card title="By day" description={plural(rows.length, 'day')}>
      <ul className="max-h-[32rem] space-y-3 overflow-y-auto pr-1">
        {rows.map((d) => (
          <li key={d.day}>
            <div className="mb-1 flex items-baseline justify-between text-sm">
              <span className="text-zinc-600 dark:text-zinc-300">{longDay(d.day)}</span>
              <span className="font-semibold tabular-nums">{duration(d.total_seconds)}</span>
            </div>
            <div className="flex h-2 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800" style={{ width: `${(d.total_seconds / max) * 100}%` }}>
              {d.projects.map((p) => (
                <div key={p.project_id} title={names.get(p.project_id)?.name ?? ''} className="h-full"
                     style={{ flexGrow: p.seconds, background: projectColor(p.project_id, names.get(p.project_id)?.color) }} />
              ))}
              {d.unassigned_seconds > 0 && <div className="h-full" style={{ flexGrow: d.unassigned_seconds, background: projectColor(null, null) }} />}
            </div>
          </li>
        ))}
      </ul>
    </Card>
  );
}
