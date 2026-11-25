import { describe, expect, it } from 'vitest';

import { periodRange, periodTitle, projectColor, shiftPeriod } from './period';

describe('periods', () => {
  it('weeks start on Monday and months span their days', () => {
    expect(periodRange('week', '2026-11-25')).toEqual({ start: '2026-11-23', end: '2026-11-29' });
    expect(periodRange('week', '2026-11-29')).toEqual({ start: '2026-11-23', end: '2026-11-29' });
    expect(periodRange('month', '2026-02-10')).toEqual({ start: '2026-02-01', end: '2026-02-28' });
    expect(periodRange('day', '2026-11-25')).toEqual({ start: '2026-11-25', end: '2026-11-25' });
  });

  it('shifts by whole periods', () => {
    expect(shiftPeriod('day', '2026-11-30', 1)).toBe('2026-12-01');
    expect(shiftPeriod('week', '2026-11-25', -1)).toBe('2026-11-18');
    expect(shiftPeriod('month', '2026-01-31', 1)).toBe('2026-02-01');
  });

  it('names the current period', () => {
    expect(periodTitle('day', '2026-11-25', '2026-11-25')).toBe('Today');
    expect(periodTitle('week', '2026-11-23', '2026-11-25')).toBe('This week');
    expect(periodTitle('month', '2026-11-01', '2026-11-25')).toBe('This month');
  });

  it('colors projects stably', () => {
    expect(projectColor(null, '#000000')).toBe('#a1a1aa');
    expect(projectColor(3, '#123456')).toBe('#123456');
    expect(projectColor(3, null)).toBe(projectColor(3, null));
  });
});

describe('time inputs', () => {
  it('round-trips HH:MM on a day and rolls an early end to the next day', async () => {
    const { atTime, hhmm } = await import('./period');
    const start = atTime('2026-11-25', '09:30');
    expect(hhmm(start)).toBe('09:30');
    const end = atTime('2026-11-25', '00:30', atTime('2026-11-25', '23:30'));
    expect(Date.parse(end) - Date.parse(atTime('2026-11-25', '23:30'))).toBe(60 * 60 * 1000);
  });
});
