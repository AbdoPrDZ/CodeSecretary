// Talks to the local agent (AG-12): finds `local.json`, then calls the
// loopback API with this collector's token. Node's http module sends no
// `Origin` header, which the agent would refuse.
import { readFile } from 'node:fs/promises';
import * as http from 'node:http';
import * as os from 'node:os';
import * as path from 'node:path';

import { agentDirs } from './control';
import type { Event, LocalAgentState, LocalApiFile, LocalEventsResponse } from './generated/contract';

export const EVENTS_PATH = '/local/v1/events';
export const STATUS_PATH = '/local/v1/status';
const TIMEOUT_MS = 5_000;

/** Where the daemon writes `local.json` (its config directory). */
export function localApiFile(
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
  home: string = os.homedir(),
): string {
  const p = platform === 'win32' ? path.win32 : path.posix;
  return p.join(agentDirs(env, platform, home).config, 'local.json');
}

export class AgentError extends Error {
  constructor(readonly kind: 'unreachable' | 'unauthorized' | 'refused' | 'unavailable', detail: string) {
    super(detail);
  }
}

interface Endpoint {
  port: number;
  token: string;
}

export interface Transport {
  send(events: Event[]): Promise<LocalEventsResponse>;
  state(): Promise<LocalAgentState>;
}

export class AgentClient implements Transport {
  private endpoint: Endpoint | null = null;

  constructor(private readonly file: string, private readonly collector: string) {}

  async send(events: Event[]): Promise<LocalEventsResponse> {
    return (await this.call('POST', EVENTS_PATH, JSON.stringify({ events }))) as LocalEventsResponse;
  }

  async state(): Promise<LocalAgentState> {
    return (await this.call('GET', STATUS_PATH)) as LocalAgentState;
  }

  private async call(method: string, route: string, body?: string): Promise<unknown> {
    const endpoint = this.endpoint ?? (await this.load());
    try {
      return await request(endpoint, method, route, body);
    } catch (err) {
      // A restarted daemon has a new port and tokens: read the file again next time.
      if (err instanceof AgentError && err.kind !== 'refused') this.endpoint = null;
      throw err;
    }
  }

  private async load(): Promise<Endpoint> {
    let info: LocalApiFile;
    try {
      info = JSON.parse(await readFile(this.file, 'utf8')) as LocalApiFile;
    } catch {
      throw new AgentError('unreachable', 'agent not running');
    }
    const token = info.tokens?.[this.collector];
    if (!info.port || !token) throw new AgentError('unreachable', 'agent has no local API');
    this.endpoint = { port: info.port, token };
    return this.endpoint;
  }
}

function request(endpoint: Endpoint, method: string, route: string, body?: string): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        host: '127.0.0.1',
        port: endpoint.port,
        path: route,
        method,
        timeout: TIMEOUT_MS,
        headers: {
          authorization: `Bearer ${endpoint.token}`,
          ...(body === undefined ? {} : { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) }),
        },
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (c: Buffer) => chunks.push(c));
        res.on('end', () => {
          const status = res.statusCode ?? 0;
          let parsed: unknown = null;
          try {
            parsed = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          } catch {
            parsed = null;
          }
          if (status === 200) resolve(parsed);
          else if (status === 401) reject(new AgentError('unauthorized', 'token refused'));
          else if (status >= 500) reject(new AgentError('unavailable', `HTTP ${status}`));
          else reject(new AgentError('refused', `HTTP ${status}`));
        });
      },
    );
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', () => reject(new AgentError('unreachable', 'agent not reachable')));
    if (body !== undefined) req.write(body);
    req.end();
  });
}
