import { describe, expect, it } from 'vitest';

import type { ActivityEvent } from './api';
import { layout, shiftDay } from './timeline';

const range = { start: '2026-11-19T23:00:00Z', end: '2026-11-20T23:00:00Z' };

function event(device_id: number, start: string, end: string | null, event_type = 'presence.active'): ActivityEvent {
  return {
    event_id: `${device_id}-${start}`,
    device_id,
    event_type: event_type as ActivityEvent['event_type'],
    source: 'agent@0.1.0',
    start,
    end,
    project_hint: null,
    repository_remote: null,
    metadata: {},
  };
}

describe('layout', () => {
  it('places events on lanes per device, clipped to the day', () => {
    const lanes = layout({
      range,
      devices: [
        { id: 1, name: 'laptop', os: 'windows', revoked_at: null },
        { id: 2, name: 'desktop', os: 'linux', revoked_at: null },
      ],
      events: [
        event(1, '2026-11-19T22:00:00Z', '2026-11-20T01:00:00Z'), // starts before the day
        event(1, '2026-11-20T11:00:00Z', '2026-11-20T11:00:00Z', 'agent.pause'),
      ],
    });
    expect(lanes.map((l) => l.name)).toEqual(['laptop']); // no events → no lane
    const [first, point] = lanes[0]!.blocks;
    expect(first!.left).toBe(0);
    expect(first!.width).toBeCloseTo((2 / 24) * 100);
    expect(point!.point).toBe(true);
    expect(point!.left).toBeCloseTo((12 / 24) * 100);
    expect(point!.width).toBeGreaterThan(0);
  });

  it('keeps events from unknown devices', () => {
    const lanes = layout({ range, devices: [], events: [event(9, '2026-11-20T00:00:00Z', null)] });
    expect(lanes[0]!.name).toBe('device 9');
  });
});

describe('shiftDay', () => {
  it('crosses month and year boundaries', () => {
    expect(shiftDay('2026-11-30', 1)).toBe('2026-12-01');
    expect(shiftDay('2026-01-01', -1)).toBe('2025-12-31');
  });
});
