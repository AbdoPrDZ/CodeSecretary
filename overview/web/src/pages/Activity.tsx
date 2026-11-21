import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { api, type ActivityEvent } from '../api';
import { IconBolt, IconCalendar, IconChevronLeft, IconChevronRight, IconClock, IconLayers, IconMonitor,
  IconPulse } from '../components/icons';
import { Badge, Button, Card, cx, Drawer, EmptyState, IconButton, Input, Notice, PageHeader, Skeleton,
  Stat } from '../components/ui';
import { clock, formatTime, longDay, plural } from '../format';
import { activityRoute } from '../router';
import { isoDay, layout, shiftDay, styleOf } from '../timeline';

const TZ = Intl.DateTimeFormat().resolvedOptions().timeZone;
const HOURS = [0, 3, 6, 9, 12, 15, 18, 21, 24];

export function ActivityPage() {
  const { day } = activityRoute.useSearch();
  const navigate = useNavigate({ from: '/activity' });
  const go = (next: string) => void navigate({ search: { day: next } });
  const today = isoDay(new Date());
  const activity = useQuery({
    queryKey: ['activity', day, TZ],
    queryFn: () => api.activity(day, TZ),
    refetchInterval: day === today ? 60_000 : false,
  });
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<ActivityEvent | null>(null);

  const data = activity.data;
  const events = useMemo(() => (data?.events ?? []).filter((e) => !hidden.has(e.event_type)), [data, hidden]);
  const lanes = useMemo(() => (data ? layout({ ...data, events }) : []), [data, events]);
  const types = useMemo(() => {
    const counts = new Map<string, number>();
    for (const e of data?.events ?? []) counts.set(e.event_type, (counts.get(e.event_type) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  }, [data]);
  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? events.filter((e) => `${e.event_type} ${summary(e)}`.toLowerCase().includes(q)) : events;
  }, [events, query]);
  const deviceName = (id: number) => data?.devices.find((d) => d.id === id)?.name ?? `device ${id}`;
  const all = data?.events ?? [];
  const nowPct = day === today && data
    ? ((Date.now() - Date.parse(data.range.start)) / (Date.parse(data.range.end) - Date.parse(data.range.start))) * 100
    : null;

  const toggle = (t: string) => setHidden((cur) => {
    const next = new Set(cur);
    if (next.has(t)) next.delete(t); else next.add(t);
    return next;
  });

  return (
    <>
      <PageHeader
        eyebrow={day === today ? 'Today' : 'Activity'}
        title={longDay(day)}
        description="Raw activity as your agents collected it. Project time is calculated by the engine and appears here once it is ready."
        actions={
          <div className="flex items-center gap-1 rounded-xl bg-white p-1 shadow-soft ring-1 ring-zinc-200/70 dark:bg-zinc-900 dark:ring-zinc-800">
            <IconButton label="Previous day" onClick={() => go(shiftDay(day, -1))}><IconChevronLeft size={16} /></IconButton>
            <label className="relative flex items-center">
              <IconCalendar size={15} className="pointer-events-none absolute left-2.5 text-zinc-400" />
              <input type="date" value={day} max={today} onChange={(e) => e.target.value && go(e.target.value)}
                     className="h-8 rounded-lg bg-transparent pr-2 pl-8 text-sm font-medium tabular-nums focus:outline-none" />
            </label>
            <IconButton label="Next day" onClick={() => go(shiftDay(day, 1))} disabled={day >= today}>
              <IconChevronRight size={16} />
            </IconButton>
            <Button size="sm" variant={day === today ? 'subtle' : 'ghost'} onClick={() => go(today)}>Today</Button>
          </div>
        }
      />

      <div className="mb-6 grid grid-cols-2 gap-4 lg:grid-cols-4">
        {activity.isPending ? (
          [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-[104px] rounded-2xl" />)
        ) : (
          <>
            <Stat icon={<IconPulse size={14} />} label="Events" value={all.length.toLocaleString()}
                  hint={data?.truncated ? 'first 5,000 shown' : 'collected this day'} />
            <Stat icon={<IconMonitor size={14} />} label="Devices" value={new Set(all.map((e) => e.device_id)).size}
                  hint="with activity" />
            <Stat icon={<IconClock size={14} />} label="First / last"
                  value={all.length ? `${clock(all[0]!.start)} - ${clock(lastEnd(all))}` : '-'}
                  hint={TZ} />
            <Stat icon={<IconLayers size={14} />} label="Kinds" value={types.length}
                  hint={types[0] ? `mostly ${styleOf(types[0][0]).label.toLowerCase()}` : 'no events'} />
          </>
        )}
      </div>

      <Card className="mb-6" title="Timeline" description="Click a block to inspect the event. Click a kind to hide it.">
        {types.length > 0 && (
          <div className="mb-5 flex flex-wrap gap-1.5">
            {types.map(([t, n]) => (
              <button key={t} type="button" onClick={() => toggle(t)}
                      className={cx('inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium ring-1 ring-inset transition',
                        hidden.has(t)
                          ? 'text-zinc-400 ring-zinc-200 line-through dark:ring-zinc-800'
                          : 'bg-zinc-50 text-zinc-700 ring-zinc-200 hover:bg-zinc-100 dark:bg-zinc-800/60 dark:text-zinc-300 dark:ring-zinc-700')}>
                <span className={cx('size-2 rounded-full', styleOf(t).className, hidden.has(t) && 'opacity-30')} />
                {styleOf(t).label}
                <span className="text-zinc-400 tabular-nums">{n}</span>
              </button>
            ))}
          </div>
        )}
        {activity.isPending && <Skeleton className="h-32" />}
        {activity.isError && <Notice tone="error">Could not load activity.</Notice>}
        {data && all.length === 0 && (
          <EmptyState icon={<IconBolt />} title="Nothing collected on this day">
            Start the CodeSecretary agent on a computer and activity shows up here within a minute.
          </EmptyState>
        )}
        {lanes.length > 0 && (
          <div className="overflow-x-auto">
            <div className="min-w-[640px]">
              <div className="relative ml-36 h-5 text-[10px] font-medium text-zinc-400 tabular-nums">
                {HOURS.map((h) => (
                  <span key={h} className={cx('absolute', h === 0 ? '' : h === 24 ? '-translate-x-full' : '-translate-x-1/2')}
                        style={{ left: `${(h / 24) * 100}%` }}>
                    {String(h % 24).padStart(2, '0')}:00
                  </span>
                ))}
              </div>
              <div className="space-y-2.5">
                {lanes.map((lane) => (
                  <div key={lane.deviceId} className="flex items-center gap-3">
                    <div className="flex w-33 shrink-0 items-center gap-2 text-sm font-medium" title={lane.name}>
                      <IconMonitor size={15} className="shrink-0 text-zinc-400" />
                      <span className="truncate">{lane.name}</span>
                    </div>
                    <div className="relative h-10 flex-1 overflow-hidden rounded-xl bg-zinc-100/80 ring-1 ring-zinc-200/60 ring-inset dark:bg-zinc-800/50 dark:ring-zinc-800">
                      {HOURS.slice(1, -1).map((h) => (
                        <div key={h} className="absolute inset-y-0 w-px bg-zinc-200/80 dark:bg-zinc-700/60"
                             style={{ left: `${(h / 24) * 100}%` }} />
                      ))}
                      {lane.blocks.map((b) => (
                        <button key={b.event.event_id} type="button" onClick={() => setSelected(b.event)}
                                title={`${styleOf(b.event.event_type).label} ${formatTime(b.event.start)}${b.event.end ? ` - ${formatTime(b.event.end)}` : ''}`}
                                className={cx('absolute rounded-[3px] transition hover:brightness-110', styleOf(b.event.event_type).className,
                                  b.point ? 'inset-y-1' : 'inset-y-2.5',
                                  selected?.event_id === b.event.event_id && 'z-10 ring-2 ring-zinc-900 ring-offset-1 dark:ring-white dark:ring-offset-zinc-900')}
                                style={{ left: `${b.left}%`, width: `${b.width}%` }} />
                      ))}
                      {nowPct !== null && nowPct > 0 && nowPct < 100 && (
                        <div className="absolute inset-y-0 z-20 w-0.5 bg-brand-500" style={{ left: `${nowPct}%` }}>
                          <span className="absolute -top-0.5 -left-[3px] size-2 rounded-full bg-brand-500" />
                        </div>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}
        {data?.truncated && <p className="mt-4 text-xs text-amber-600">Showing the first 5,000 events of this day.</p>}
      </Card>

      <Card title="Events" description={data ? `${plural(rows.length, 'event')}${query ? ' match' : ''}` : undefined}
            actions={<Input className="h-8 w-56" placeholder="Filter events..." value={query} onChange={(e) => setQuery(e.target.value)} />}
            bodyClassName="p-0">
        <div className="scroll-thin max-h-[32rem] overflow-auto">
          <table className="w-full text-left text-sm">
            <thead className="sticky top-0 z-10 bg-white/95 text-[11px] font-semibold tracking-wider text-zinc-400 uppercase backdrop-blur dark:bg-zinc-900/95">
              <tr>
                <th className="py-2.5 pr-3 pl-5">Time</th>
                <th className="py-2.5 pr-3">Kind</th>
                <th className="hidden py-2.5 pr-3 md:table-cell">Device</th>
                <th className="py-2.5 pr-5">Details</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-100 dark:divide-zinc-800/80">
              {rows.map((e) => (
                <tr key={e.event_id} onClick={() => setSelected(e)}
                    className={cx('cursor-pointer transition hover:bg-zinc-50 dark:hover:bg-zinc-800/40',
                      selected?.event_id === e.event_id && 'bg-brand-50/60 dark:bg-brand-500/5')}>
                  <td className="py-2 pr-3 pl-5 whitespace-nowrap text-zinc-600 tabular-nums dark:text-zinc-400">
                    {formatTime(e.start)}
                    {e.end && <span className="hidden text-zinc-400 sm:inline"> - {formatTime(e.end)}</span>}
                  </td>
                  <td className="py-2 pr-3 whitespace-nowrap">
                    <span className="inline-flex items-center gap-2 font-medium">
                      <span className={cx('size-2 rounded-full', styleOf(e.event_type).className)} />
                      {styleOf(e.event_type).label}
                    </span>
                  </td>
                  <td className="hidden py-2 pr-3 whitespace-nowrap text-zinc-500 md:table-cell">{deviceName(e.device_id)}</td>
                  <td className="max-w-md truncate py-2 pr-5 text-zinc-500">{summary(e)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {data && rows.length === 0 && all.length > 0 && <p className="p-5 text-sm text-zinc-500">No events match.</p>}
        </div>
      </Card>

      {selected && (
        <Drawer onClose={() => setSelected(null)}
                title={<span className="flex items-center gap-2"><span className={cx('size-2.5 rounded-full', styleOf(selected.event_type).className)} />{styleOf(selected.event_type).label}</span>}>
          <EventDetail event={selected} device={deviceName(selected.device_id)} />
        </Drawer>
      )}
    </>
  );
}

function lastEnd(events: ActivityEvent[]): string {
  return events.reduce((max, e) => {
    const end = e.end ?? e.start;
    return end > max ? end : max;
  }, events[0]!.start);
}

function summary(event: ActivityEvent): string {
  const m = event.metadata as Record<string, unknown>;
  const main = [m.title, m.app, m.file_path, m.workspace_path, m.to_branch && `to ${String(m.to_branch)}`, m.branch]
    .filter((v) => typeof v === 'string' && v) as string[];
  const parts = main.length ? main : Object.entries(m).map(([k, v]) => `${k}: ${typeof v === 'string' ? v : JSON.stringify(v)}`);
  if (event.repository_remote) parts.unshift(event.repository_remote);
  return parts.join(' · ') || '-';
}

function EventDetail({ event, device }: { event: ActivityEvent; device: string }) {
  const rows: [string, string][] = [
    ['Kind', event.event_type],
    ['Device', device],
    ['Start', new Date(event.start).toLocaleString()],
    ['End', event.end ? new Date(event.end).toLocaleString() : 'point event'],
    ['Source', event.source],
  ];
  if (event.project_hint) rows.push(['Project hint', event.project_hint]);
  if (event.repository_remote) rows.push(['Repository', event.repository_remote]);
  return (
    <div className="space-y-5">
      <dl className="divide-y divide-zinc-100 rounded-xl ring-1 ring-zinc-200/70 dark:divide-zinc-800 dark:ring-zinc-800">
        {rows.map(([k, v]) => (
          <div key={k} className="grid grid-cols-[7rem_1fr] gap-3 px-3.5 py-2.5 text-sm">
            <dt className="text-zinc-500">{k}</dt>
            <dd className="font-medium break-all">{v}</dd>
          </div>
        ))}
      </dl>
      <div>
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold tracking-wider text-zinc-400 uppercase">
          Metadata <Badge>{Object.keys(event.metadata).length} fields</Badge>
        </div>
        <pre className="scroll-thin overflow-auto rounded-xl bg-zinc-950 p-4 text-xs leading-relaxed text-zinc-200 dark:bg-black/40">
          {JSON.stringify(event.metadata, null, 2)}
        </pre>
      </div>
      <p className="font-mono text-[11px] break-all text-zinc-400">{event.event_id}</p>
    </div>
  );
}
