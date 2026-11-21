import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { api, type ActivityEvent } from '../api';
import { buttonClass, Card, formatTime } from '../components/Layout';
import { activityRoute } from '../router';
import { isoDay, layout, shiftDay, styleOf } from '../timeline';

const TZ = Intl.DateTimeFormat().resolvedOptions().timeZone;
const HOURS = [0, 3, 6, 9, 12, 15, 18, 21];

export function ActivityPage() {
  const { day } = activityRoute.useSearch();
  const navigate = useNavigate({ from: '/activity' });
  const go = (next: string) => void navigate({ search: { day: next } });
  const activity = useQuery({
    queryKey: ['activity', day, TZ],
    queryFn: () => api.activity(day, TZ),
    refetchInterval: day === isoDay(new Date()) ? 60_000 : false,
  });
  const lanes = useMemo(() => (activity.data ? layout(activity.data) : []), [activity.data]);
  const [selected, setSelected] = useState<ActivityEvent | null>(null);
  const deviceName = (id: number) => activity.data?.devices.find((d) => d.id === id)?.name ?? `device ${id}`;
  const types = useMemo(() => [...new Set(activity.data?.events.map((e) => e.event_type))], [activity.data]);

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <h1 className="mr-2 text-xl font-semibold">Activity</h1>
        <button type="button" className={buttonClass} onClick={() => go(shiftDay(day, -1))} aria-label="Previous day">
          ‹
        </button>
        <input type="date" value={day} onChange={(e) => e.target.value && go(e.target.value)}
               className="rounded-md border border-slate-300 bg-white px-2 py-1 text-sm dark:border-slate-700 dark:bg-slate-900" />
        <button type="button" className={buttonClass} onClick={() => go(shiftDay(day, 1))} aria-label="Next day">
          ›
        </button>
        <button type="button" className={buttonClass} onClick={() => go(isoDay(new Date()))}>
          Today
        </button>
        <span className="ml-auto text-xs text-slate-500">{TZ}</span>
      </div>

      <p className="mb-4 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/40 dark:text-amber-200">
        Raw events as your agents collected them. Tracked project time appears here once the engine is ready.
      </p>

      <Card title="Timeline" actions={<Legend types={types} />}>
        {activity.isPending && <p className="text-sm text-slate-500">Loading…</p>}
        {activity.isError && <p className="text-sm text-red-600">Could not load activity.</p>}
        {activity.data && lanes.length === 0 && (
          <p className="text-sm text-slate-500">No activity on this day.</p>
        )}
        {lanes.length > 0 && (
          <div className="space-y-3">
            <div className="relative ml-32 h-4 text-[10px] text-slate-500">
              {HOURS.map((h) => (
                <span key={h} className="absolute -translate-x-1/2" style={{ left: `${(h / 24) * 100}%` }}>
                  {String(h).padStart(2, '0')}:00
                </span>
              ))}
            </div>
            {lanes.map((lane) => (
              <div key={lane.deviceId} className="flex items-center gap-2">
                <div className="w-30 truncate text-sm" title={lane.name}>{lane.name}</div>
                <div className="relative h-8 flex-1 overflow-hidden rounded-md bg-slate-100 dark:bg-slate-800">
                  {HOURS.map((h) => (
                    <div key={h} className="absolute inset-y-0 w-px bg-slate-200 dark:bg-slate-700"
                         style={{ left: `${(h / 24) * 100}%` }} />
                  ))}
                  {lane.blocks.map((b) => (
                    <button key={b.event.event_id} type="button" onClick={() => setSelected(b.event)}
                            title={`${styleOf(b.event.event_type).label} ${formatTime(b.event.start)}–${formatTime(b.event.end)}`}
                            className={`absolute rounded-sm ${styleOf(b.event.event_type).className} ${
                              b.point ? 'inset-y-0' : 'inset-y-1'
                            } ${selected?.event_id === b.event.event_id ? 'ring-2 ring-slate-900 dark:ring-white' : ''}`}
                            style={{ left: `${b.left}%`, width: `${b.width}%` }} />
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
        {activity.data?.truncated && (
          <p className="mt-3 text-xs text-amber-700">Showing the first 5,000 events of this day.</p>
        )}
      </Card>

      {selected && (
        <Card title="Selected event" actions={<button type="button" className={buttonClass} onClick={() => setSelected(null)}>Close</button>}>
          <EventDetail event={selected} device={deviceName(selected.device_id)} />
        </Card>
      )}

      <Card title={`Events${activity.data ? ` (${activity.data.events.length})` : ''}`}>
        <div className="max-h-[28rem] overflow-auto">
          <table className="w-full text-left text-sm">
            <thead className="sticky top-0 bg-white text-xs text-slate-500 uppercase dark:bg-slate-900">
              <tr>
                <th className="py-2 pr-3 font-medium">Start</th>
                <th className="py-2 pr-3 font-medium">End</th>
                <th className="py-2 pr-3 font-medium">Device</th>
                <th className="py-2 pr-3 font-medium">Type</th>
                <th className="py-2 font-medium">Details</th>
              </tr>
            </thead>
            <tbody>
              {activity.data?.events.map((e) => (
                <tr key={e.event_id} onClick={() => setSelected(e)}
                    className="cursor-pointer border-t border-slate-100 hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-800/50">
                  <td className="py-1.5 pr-3 tabular-nums">{formatTime(e.start)}</td>
                  <td className="py-1.5 pr-3 tabular-nums">{formatTime(e.end)}</td>
                  <td className="py-1.5 pr-3">{deviceName(e.device_id)}</td>
                  <td className="py-1.5 pr-3">
                    <span className={`mr-2 inline-block size-2 rounded-full ${styleOf(e.event_type).className}`} />
                    {e.event_type}
                  </td>
                  <td className="max-w-md truncate py-1.5 text-slate-500">{summary(e)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </>
  );
}

function Legend({ types }: { types: string[] }) {
  return (
    <div className="flex flex-wrap gap-3 text-xs text-slate-500">
      {types.map((t) => (
        <span key={t} className="flex items-center gap-1">
          <span className={`inline-block size-2.5 rounded-sm ${styleOf(t).className}`} />
          {styleOf(t).label}
        </span>
      ))}
    </div>
  );
}

function summary(event: ActivityEvent): string {
  const parts = Object.entries(event.metadata).map(([k, v]) => `${k}: ${typeof v === 'string' ? v : JSON.stringify(v)}`);
  if (event.repository_remote) parts.unshift(event.repository_remote);
  return parts.join(' · ') || '—';
}

function EventDetail({ event, device }: { event: ActivityEvent; device: string }) {
  const rows: [string, string][] = [
    ['Type', event.event_type],
    ['Device', device],
    ['Start', new Date(event.start).toLocaleString()],
    ['End', event.end ? new Date(event.end).toLocaleString() : '— (point event)'],
    ['Source', event.source],
    ['Event id', event.event_id],
  ];
  if (event.project_hint) rows.push(['Project hint', event.project_hint]);
  if (event.repository_remote) rows.push(['Repository', event.repository_remote]);
  return (
    <div className="grid gap-4 md:grid-cols-2">
      <dl className="grid grid-cols-[8rem_1fr] gap-y-1 text-sm">
        {rows.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="text-slate-500">{k}</dt>
            <dd className="break-all">{v}</dd>
          </div>
        ))}
      </dl>
      <pre className="overflow-auto rounded-md bg-slate-100 p-3 text-xs dark:bg-slate-800">
        {JSON.stringify(event.metadata, null, 2)}
      </pre>
    </div>
  );
}
