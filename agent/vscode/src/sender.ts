// Queues events in memory and hands them to the agent in batches. When the
// agent is unreachable, events are kept for [`RETAIN_MS`] and then dropped
// (AGENT plan §4); nothing is ever written to disk here.
import { AgentError, type Transport } from './agent';
import type { Event } from './generated/contract';

export const RETAIN_MS = 5 * 60_000;
export const MAX_QUEUE = 5_000;
export const BATCH = 500;

export type Link = 'connecting' | 'connected' | 'unreachable';

export interface SenderState {
  link: Link;
  queued: number;
  /** Events dropped because the agent stayed unreachable. */
  dropped: number;
  /** Events handed to the agent. */
  sent: number;
  /** Events the agent refused as invalid (a bug here, not the user's problem). */
  rejected: number;
}

export class Sender {
  private queue: { event: Event; at: number }[] = [];
  private inFlight = false;
  readonly state: SenderState = { link: 'connecting', queued: 0, sent: 0, dropped: 0, rejected: 0 };

  constructor(private readonly transport: Transport, private readonly onChange: () => void = () => {}) {}

  push(event: Event, now: number = Date.now()): void {
    this.queue.push({ event, at: now });
    if (this.queue.length > MAX_QUEUE) this.drop(this.queue.length - MAX_QUEUE);
    this.state.queued = this.queue.length;
  }

  /** Sends what is queued, one batch at a time, until empty or the agent fails. */
  async flush(now: number = Date.now()): Promise<void> {
    if (this.inFlight) return;
    this.inFlight = true;
    try {
      while (this.queue.length > 0) {
        const batch = this.queue.slice(0, BATCH);
        try {
          const result = await this.transport.send(batch.map((q) => q.event));
          this.queue.splice(0, batch.length);
          this.state.sent += result.accepted;
          this.state.rejected += result.rejected.length;
          this.setLink('connected');
        } catch (err) {
          // 4xx other than 401 means the batch itself is unacceptable: retrying won't help.
          if (err instanceof AgentError && err.kind === 'refused') {
            this.queue.splice(0, batch.length);
            this.state.rejected += batch.length;
            continue;
          }
          this.setLink('unreachable');
          break;
        }
      }
    } finally {
      this.expire(now);
      this.state.queued = this.queue.length;
      this.inFlight = false;
      this.onChange();
    }
  }

  private expire(now: number): void {
    let n = 0;
    while (n < this.queue.length && now - this.queue[n]!.at > RETAIN_MS) n++;
    if (n > 0) this.drop(n);
  }

  private drop(n: number): void {
    this.queue.splice(0, n);
    this.state.dropped += n;
  }

  private setLink(link: Link): void {
    if (this.state.link !== link) {
      this.state.link = link;
      this.onChange();
    }
  }
}
