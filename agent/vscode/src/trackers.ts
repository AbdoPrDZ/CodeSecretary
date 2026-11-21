// Pure state machines turning editor signals into intervals. Times are Unix
// milliseconds passed in by the caller, so tests drive them without a clock.
import type { FileRef } from './repo';

/** Intervals are cut at this length so a crash loses at most this much. */
export const MAX_INTERVAL_MS = 5 * 60_000;
/** Focus shorter than this is a flick through tabs, not work. */
export const MIN_FOCUS_MS = 2_000;
/** Edits to one file are summed over this window (AGENT plan §4). */
export const EDIT_WINDOW_MS = 30_000;

export interface Focused {
  file: FileRef;
  language: string | null;
}

export interface FocusInterval extends Focused {
  start: number;
  end: number;
}

const key = (f: FileRef) => `${f.repo.root}\u0000${f.relPath}`;

/** `editor.file_focus`: which file has focus while the window is focused and in use. */
export class FocusTracker {
  private current: (Focused & { start: number }) | null = null;

  /** `focused` is null when the window is unfocused/inactive or no file editor is active. */
  update(focused: Focused | null, now: number): FocusInterval[] {
    const out: FocusInterval[] = [];
    const cur = this.current;
    if (cur && focused && key(cur.file) === key(focused.file)) {
      out.push(...this.tick(now));
      return out;
    }
    if (cur) this.close(now, out);
    if (focused) this.current = { ...focused, start: now };
    return out;
  }

  /** Cuts an interval that reached [`MAX_INTERVAL_MS`]. */
  tick(now: number): FocusInterval[] {
    const cur = this.current;
    if (!cur || now - cur.start < MAX_INTERVAL_MS) return [];
    this.current = { ...cur, start: now };
    return [{ file: cur.file, language: cur.language, start: cur.start, end: now }];
  }

  finish(now: number): FocusInterval[] {
    const out: FocusInterval[] = [];
    if (this.current) this.close(now, out);
    return out;
  }

  private close(now: number, out: FocusInterval[]): void {
    const cur = this.current!;
    this.current = null;
    if (now - cur.start >= MIN_FOCUS_MS) out.push({ file: cur.file, language: cur.language, start: cur.start, end: now });
  }
}

export interface Burst {
  file: FileRef;
  count: number;
  start: number;
  end: number;
}

/** `editor.edit_burst`: change counts per file over [`EDIT_WINDOW_MS`]. */
export class EditAggregator {
  private readonly open = new Map<string, Burst>();

  edit(file: FileRef, now: number): void {
    const k = key(file);
    const burst = this.open.get(k);
    if (burst) {
      burst.count += 1;
      burst.end = Math.max(burst.end, now);
    } else {
      this.open.set(k, { file, count: 1, start: now, end: now });
    }
  }

  /** Bursts whose window has passed (all of them with `force`). */
  flush(now: number, force = false): Burst[] {
    const out: Burst[] = [];
    for (const [k, burst] of this.open) {
      if (force || now - burst.start >= EDIT_WINDOW_MS) {
        out.push(burst);
        this.open.delete(k);
      }
    }
    return out;
  }
}
