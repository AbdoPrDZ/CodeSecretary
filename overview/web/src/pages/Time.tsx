// Time per project: the engine's work sessions, added up by the API. This page
// only displays durations it was given; it never derives time from events.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { api, type DaySessions, type Me, type TimeSummary, type WorkSession } from '../api';
import { AddTimeDialog, CorrectionsCard, SessionDrawer } from '../components/corrections';
import { IconChevronLeft, IconChevronRight, IconClock, IconFolder, IconInfo, IconLayers,
  IconSparkles } from '../components/icons';
import { Button, Card, cx, EmptyState, IconButton, Notice, PageHeader, Segmented, Skeleton, Stat } from '../components/ui';
import { clock, duration, hours, plural } from '../format';
import { dayOfMonth, type Period, periodRange, periodTitle, projectColor, shiftPeriod, weekday } from '../period';
import { timeRoute } from '../router';
import { isoDay } from '../timeline';

const TZ = Intl.DateTimeFormat().resolvedOptions().timeZone;
const HOURS = [0, 3, 6, 9, 12, 15, 18, 21, 24];

export function TimePage() {
  const { period, day } = timeRoute.useSearch();
  const navigate = useNavigate({ from: '/time' });
  const go = (next: { period?: Period; day?: string }) => void navigate({ search: (s) => ({ ...s, ...next }) });
  const today = isoDay(new Date());
  const range = periodRange(period, day);
  const isCurrent = range.start <= today && today <= range.end;

  const me = useQuery({ queryKey: ['me'], queryFn: api.me, staleTime: 60_000 });
  const summary = useQuery({
    queryKey: ['time', range.start, range.end],
    queryFn: () => api.time(range.start, range.end),
    // While the engine is recalculating, poll; otherwise refresh the current period once a minute.
    refetchInterval: (q) => ((q.state.data?.pending_days ?? 0) > 0 ? 5_000 : isCurrent ? 60_000 : false),
  });
  const sessions = useQuery({
    queryKey: ['sessions', day],
    queryFn: () => api.sessions(day),
    enabled: period === 'day',
    refetchInterval: (q) => ((q.state.data?.pending_days ?? 0) > 0 ? 5_000 : isCurrent ? 60_000 : false),
  });
  const projects = useQuery({ queryKey: ['projects'], queryFn: api.projects, staleTime: 60_000 });
  const [selected, setSelected] = useState<WorkSession | null>(null);
  const [adding, setAdding] = useState(false);

  const data = summary.data;
  const top = data?.projects[0];
  const pending = data?.pending_days ?? 0;

  return (
    <>
      <PageHeader
        eyebrow="Time"
        title={periodTitle(period, day, today)}
        description="Time per project, calculated by the engine from your activity and your project rules."
        actions={
          <>
            <Segmented<Period> value={period} onChange={(p) => go({ period: p })}
                               options={[{ value: 'day', label: 'Day' }, { value: 'week', label: 'Week' }, { value: 'month', label: 'Month' }]} />
            <div className="flex items-center gap-1 rounded-xl bg-white p-1 shadow-soft ring-1 ring-zinc-200/70 dark:bg-zinc-900 dark:ring-zinc-800">
              <IconButton label="Previous" onClick={() => go({ day: shiftPeriod(period, day, -1) })}><IconChevronLeft size={16} /></IconButton>
              <IconButton label="Next" onClick={() => go({ day: shiftPeriod(period, day, 1) })} disabled={range.end >= today}>
                <IconChevronRight size={16} />
              </IconButton>
              <Button size="sm" variant={isCurrent ? 'subtle' : 'ghost'} onClick={() => go({ day: today })}>Today</Button>
            </div>
          </>
        }
      />

      <div className="mb-6 space-y-3">
        {me.data && <TimeZoneNotice me={me.data} />}
        {pending > 0 && (
          <Notice>Recalculating {plural(pending, 'day')}. Totals update by themselves in a moment.</Notice>
        )}
        {(data?.failed_days ?? 0) > 0 && (
          <Notice tone="warn">{plural(data!.failed_days, 'day')} could not be calculated. They are retried when new activity arrives.</Notice>
        )}
      </div>

      <div className="mb-6 grid grid-cols-2 gap-4 lg:grid-cols-4">
        {summary.isPending ? (
          [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-[104px] rounded-2xl" />)
        ) : (
          <>
            <Stat icon={<IconClock size={14} />} label="Tracked" value={duration(data?.total_seconds ?? 0)}
                  hint={period === 'day' ? 'active time' : `${range.start} - ${range.end}`} />
            <Stat icon={<IconFolder size={14} />} label="On projects" value={duration(data?.assigned_seconds ?? 0)}
                  hint={plural(data?.projects.length ?? 0, 'project')} />
            <Stat icon={<IconSparkles size={14} />} label="Top project" value={top ? duration(top.seconds) : '-'}
                  hint={top?.name ?? 'none yet'} />
            <Stat icon={<IconLayers size={14} />} label="Unassigned" value={duration(data?.unassigned_seconds ?? 0)}
                  hint="no rule matched" />
          </>
        )}
      </div>

      {summary.isError && <Notice tone="error">Could not load time.</Notice>}

      {data && data.total_seconds === 0 && !summary.isPending && (
        <Card className="mb-6">
          <EmptyState icon={<IconClock />} title={pending > 0 ? 'Calculating...' : 'No time calculated for this period'}
                      action={(projects.data?.length ?? 0) === 0
                        ? <Link to="/projects"><Button variant="primary">Create a project</Button></Link>
                        : undefined}>
            {(projects.data?.length ?? 0) === 0
              ? 'Create a project and give it a rule (a folder, repository, app or window title). The engine then assigns your activity to it.'
              : 'Time appears here a few minutes after the agent uploads activity.'}
          </EmptyState>
        </Card>
      )}

      {data && data.total_seconds > 0 && (
        <div className="grid gap-6 xl:grid-cols-5">
          <div className="xl:col-span-2">
            <ByProject data={data} />
          </div>
          <div className="space-y-6 xl:col-span-3">
            {period === 'day'
              ? <DayTimeline data={sessions.data} loading={sessions.isPending} onSelect={setSelected} />
              : <ByDay data={data} onPick={(d) => go({ period: 'day', day: d })} />}
          </div>
        </div>
      )}

      {period === 'day' && data && (
        <div className="mt-6">
          <CorrectionsCard day={day} onAdd={() => setAdding(true)} />
        </div>
      )}
      {selected && <SessionDrawer session={selected} day={day} onClose={() => setSelected(null)} />}
      {adding && <AddTimeDialog day={day} onClose={() => setAdding(false)} />}
    </>
  );
}

function TimeZoneNotice({ me }: { me: Me }) {
  const client = useQueryClient();
  const save = useMutation({
    mutationFn: () => api.setTimeZone(TZ),
    onSuccess: () => void client.invalidateQueries(),
  });
  if (me.time_zone === TZ) return null;
  return (
    <Notice tone="warn">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span>Your days are cut at midnight in <b>{me.time_zone}</b>, but this browser is in <b>{TZ}</b>.</span>
        <Button size="sm" variant="primary" disabled={save.isPending} onClick={() => save.mutate()}>
          Use {TZ}
        </Button>
      </div>
    </Notice>
  );
}

function ByProject({ data }: { data: TimeSummary }) {
  const rows = [
    ...data.projects.map((p) => ({ key: String(p.project_id), id: p.project_id as number | null, name: p.name ?? 'Deleted project',
      color: p.color, seconds: p.seconds, sessions: p.sessions, archived: p.archived })),
    ...(data.unassigned_seconds > 0
      ? [{ key: 'none', id: null, name: 'Unassigned', color: null, seconds: data.unassigned_seconds, sessions: 0, archived: false }]
      : []),
  ];
  const max = Math.max(...rows.map((r) => r.seconds), 1);
  return (
    <Card title="By project" description="Active time per project">
      <ul className="space-y-4">
        {rows.map((r) => (
          <li key={r.key}>
            <div className="mb-1.5 flex items-baseline gap-2 text-sm">
              <span className="size-2.5 shrink-0 rounded-full" style={{ background: projectColor(r.id, r.color) }} />
              <span className={cx('min-w-0 flex-1 truncate font-medium', r.id === null && 'text-zinc-500')}>
                {r.name}{r.archived && <span className="ml-1.5 text-xs font-normal text-zinc-400">archived</span>}
              </span>
              <span className="font-semibold tabular-nums">{duration(r.seconds)}</span>
              <span className="w-10 text-right text-xs text-zinc-400 tabular-nums">
                {Math.round((r.seconds / Math.max(data.total_seconds, 1)) * 100)}%
              </span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
              <div className="h-full rounded-full" style={{ width: `${(r.seconds / max) * 100}%`, background: projectColor(r.id, r.color) }} />
            </div>
          </li>
        ))}
      </ul>
      {data.unassigned_seconds > 0 && (
        <p className="mt-5 flex items-start gap-2 text-xs text-zinc-500">
          <IconInfo size={14} className="mt-px shrink-0" />
          <span>Unassigned time matched no rule. <Link to="/projects" className="font-medium text-brand-600 hover:underline dark:text-brand-400">Add a rule</Link> and recent days are recalculated.</span>
        </p>
      )}
    </Card>
  );
}

function ByDay({ data, onPick }: { data: TimeSummary; onPick: (day: string) => void }) {
  const colors = useMemo(() => new Map(data.projects.map((p) => [p.project_id, projectColor(p.project_id, p.color)])), [data]);
  const names = useMemo(() => new Map(data.projects.map((p) => [p.project_id, p.name ?? 'Deleted project'])), [data]);
  const max = Math.max(...data.days.map((d) => d.total_seconds), 3600);
  const many = data.days.length > 7;
  return (
    <Card title="By day" description="Click a day to see its sessions">
      <div className="flex h-56 items-end gap-1.5">
        {data.days.map((d) => (
          <button key={d.day} type="button" onClick={() => onPick(d.day)}
                  title={`${d.day}: ${duration(d.total_seconds)}`}
                  className="group flex h-full min-w-0 flex-1 flex-col items-center justify-end gap-1.5">
            {!many && d.total_seconds > 0 && <span className="text-[11px] text-zinc-500 tabular-nums">{hours(d.total_seconds)}</span>}
            <div className="flex w-full max-w-12 flex-col-reverse overflow-hidden rounded-md bg-zinc-100 transition group-hover:brightness-95 dark:bg-zinc-800"
                 style={{ height: `${Math.max((d.total_seconds / max) * 100, 2)}%` }}>
              {d.projects.map((p) => (
                <div key={p.project_id} style={{ height: `${(p.seconds / Math.max(d.total_seconds, 1)) * 100}%`, background: colors.get(p.project_id) }}
                     title={`${names.get(p.project_id) ?? ''}: ${duration(p.seconds)}`} />
              ))}
              {d.unassigned_seconds > 0 && (
                <div style={{ height: `${(d.unassigned_seconds / Math.max(d.total_seconds, 1)) * 100}%` }}
                     className="bg-zinc-300 dark:bg-zinc-600" title={`Unassigned: ${duration(d.unassigned_seconds)}`} />
              )}
            </div>
            <span className="text-[11px] text-zinc-400 tabular-nums">{many ? dayOfMonth(d.day) : weekday(d.day)}</span>
          </button>
        ))}
      </div>
    </Card>
  );
}

function DayTimeline({ data, loading, onSelect }: {
  data: DaySessions | undefined; loading: boolean; onSelect: (s: WorkSession) => void;
}) {
  if (loading || !data) return <Skeleton className="h-64 rounded-2xl" />;
  const start = Date.parse(data.range.start);
  const span = Date.parse(data.range.end) - start;
  const pct = (iso: string) => Math.min(Math.max((Date.parse(iso) - start) / span, 0), 1) * 100;
  return (
    <Card title="Sessions" description={`${plural(data.sessions.length, 'session')} · days in ${data.time_zone}`} bodyClassName="p-0">
      <div className="px-5 pt-5 pb-4">
        <div className="relative h-5 text-[10px] font-medium text-zinc-400 tabular-nums">
          {HOURS.map((h) => (
            <span key={h} className={cx('absolute', h === 0 ? '' : h === 24 ? '-translate-x-full' : '-translate-x-1/2')}
                  style={{ left: `${(h / 24) * 100}%` }}>
              {String(h % 24).padStart(2, '0')}:00
            </span>
          ))}
        </div>
        <div className="relative h-10 overflow-hidden rounded-xl bg-zinc-100/80 ring-1 ring-zinc-200/60 ring-inset dark:bg-zinc-800/50 dark:ring-zinc-800">
          {data.sessions.map((s) => (
            <button key={s.id} type="button" onClick={() => onSelect(s)}
                 className="absolute inset-y-1.5 rounded-[4px] transition hover:brightness-110"
                 title={`${s.project_name ?? 'Unassigned'} ${clock(s.start)} - ${clock(s.end)} (${duration(s.seconds)})`}
                 style={{ left: `${pct(s.start)}%`, width: `${Math.max(pct(s.end) - pct(s.start), 0.2)}%`,
                   background: projectColor(s.project_id, s.project_color) }} />
          ))}
        </div>
        <p className="mt-2 text-xs text-zinc-400">Click a session to see why it was counted and to correct it.</p>
      </div>
      <div className="scroll-thin max-h-[28rem] overflow-auto border-t border-zinc-100 dark:border-zinc-800/80">
        <table className="w-full text-left text-sm">
          <thead className="sticky top-0 bg-white/95 text-[11px] font-semibold tracking-wider text-zinc-400 uppercase backdrop-blur dark:bg-zinc-900/95">
            <tr>
              <th className="py-2.5 pr-3 pl-5">Time</th>
              <th className="py-2.5 pr-3">Project</th>
              <th className="py-2.5 pr-3 text-right">Duration</th>
              <th className="hidden py-2.5 pr-5 text-right sm:table-cell">Evidence</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-100 dark:divide-zinc-800/80">
            {data.sessions.map((s) => (
              <tr key={s.id} onClick={() => onSelect(s)} className="cursor-pointer transition hover:bg-zinc-50 dark:hover:bg-zinc-800/40">
                <td className="py-2 pr-3 pl-5 whitespace-nowrap text-zinc-600 tabular-nums dark:text-zinc-400">
                  {clock(s.start)} - {clock(s.end)}
                </td>
                <td className="py-2 pr-3">
                  <span className="inline-flex items-center gap-2 font-medium">
                    <span className="size-2 rounded-full" style={{ background: projectColor(s.project_id, s.project_color) }} />
                    <span className={cx(s.project_id === null && 'text-zinc-500')}>{s.project_name ?? 'Unassigned'}</span>
                    {s.kind === 'waiting' && <span className="text-xs text-zinc-400">waiting</span>}
                    {s.manual && <span className="text-xs text-zinc-400">manual</span>}
                  </span>
                </td>
                <td className="py-2 pr-3 text-right font-medium tabular-nums">{duration(s.seconds)}</td>
                <td className="hidden py-2 pr-5 text-right text-zinc-500 tabular-nums sm:table-cell">{s.manual ? '-' : plural(s.evidence, 'event')}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {data.sessions.length === 0 && <p className="p-5 text-sm text-zinc-500">No sessions on this day.</p>}
      </div>
    </Card>
  );
}
