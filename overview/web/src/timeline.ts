// Pure layout for the day timeline: where each raw event sits on a 24 h
// lane. Display only; this never adds durations up (only the engine does).
import type { Activity, ActivityEvent } from './api';

export interface Block {
  event: ActivityEvent;
  /** Percent of the day from the left edge. */
  left: number;
  /** Percent width (at least a sliver, so point events stay visible). */
  width: number;
  point: boolean;
}

export interface Lane {
  deviceId: number;
  name: string;
  blocks: Block[];
}

const MIN_WIDTH = 0.15;

export function layout(activity: Pick<Activity, 'range' | 'devices' | 'events'>): Lane[] {
  const start = Date.parse(activity.range.start);
  const span = Date.parse(activity.range.end) - start;
  const clamp = (t: number) => Math.min(Math.max((t - start) / span, 0), 1) * 100;
  const lanes = new Map<number, Lane>();
  for (const device of activity.devices) {
    lanes.set(device.id, { deviceId: device.id, name: device.name, blocks: [] });
  }
  for (const event of activity.events) {
    let lane = lanes.get(event.device_id);
    if (!lane) {
      lane = { deviceId: event.device_id, name: `device ${event.device_id}`, blocks: [] };
      lanes.set(event.device_id, lane);
    }
    const from = Date.parse(event.start);
    const to = event.end ? Date.parse(event.end) : from;
    const left = clamp(from);
    const point = to <= from;
    lane.blocks.push({ event, left, width: point ? MIN_WIDTH : Math.max(clamp(to) - left, MIN_WIDTH), point });
  }
  // Devices with nothing that day are left out.
  return [...lanes.values()].filter((l) => l.blocks.length > 0);
}

/** `YYYY-MM-DD` of `date` in the browser's zone. */
export function isoDay(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

export function shiftDay(day: string, days: number): string {
  const [y, m, d] = day.split('-').map(Number);
  return isoDay(new Date(y!, m! - 1, d! + days));
}

export const EVENT_STYLE: Record<string, { label: string; className: string }> = {
  'presence.active': { label: 'Active', className: 'bg-emerald-500' },
  'presence.idle': { label: 'Idle', className: 'bg-slate-300 dark:bg-slate-600' },
  'window.focus': { label: 'Window', className: 'bg-sky-500' },
  'editor.file_focus': { label: 'Editor', className: 'bg-violet-500' },
  'editor.edit_burst': { label: 'Edits', className: 'bg-violet-600' },
  'agent.pause': { label: 'Paused', className: 'bg-amber-500' },
  'agent.resume': { label: 'Resumed', className: 'bg-amber-300' },
};

export function styleOf(type: string) {
  return EVENT_STYLE[type] ?? { label: type, className: 'bg-indigo-400' };
}
