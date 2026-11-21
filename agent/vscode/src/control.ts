// Manages the local agent from VS Code, the same way the desktop widget does
// (agent/panel): read `<data>/status.json`, and run `secretary-agent` for
// start, stop, login and logout. Never touches tokens: the agent keeps them
// in the OS keychain.
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { createInterface } from 'node:readline';

import type { AgentStatus } from './generated/contract';

/** A status older than this means the daemon is not running (`STALE_AFTER_SECS` in shared). */
export const STALE_AFTER_MS = 20_000;

export interface AgentDirs {
  config: string;
  data: string;
}

/** The agent's directories (mirrors `AgentPaths` in agent/daemon/src/paths.rs). */
export function agentDirs(
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
  home: string = os.homedir(),
): AgentDirs {
  const p = platform === 'win32' ? path.win32 : path.posix;
  if (env.SECRETARY_HOME) {
    return { config: p.join(env.SECRETARY_HOME, 'config'), data: p.join(env.SECRETARY_HOME, 'data') };
  }
  if (platform === 'win32') {
    return {
      config: p.join(env.APPDATA || p.join(home, 'AppData', 'Roaming'), 'secretary', 'config'),
      data: p.join(env.LOCALAPPDATA || p.join(home, 'AppData', 'Local'), 'secretary', 'data'),
    };
  }
  if (platform === 'darwin') {
    const dir = p.join(home, 'Library', 'Application Support', 'secretary');
    return { config: dir, data: dir };
  }
  const xdg = (name: string, fallback: string) => {
    const v = env[name];
    return v && p.isAbsolute(v) ? v : p.join(home, fallback);
  };
  return {
    config: p.join(xdg('XDG_CONFIG_HOME', '.config'), 'secretary'),
    data: p.join(xdg('XDG_DATA_HOME', p.join('.local', 'share')), 'secretary'),
  };
}

export function isLive(status: AgentStatus | null, now: number = Date.now()): boolean {
  if (!status?.running || !status.written_at) return false;
  return now - Date.parse(status.written_at) <= STALE_AFTER_MS;
}

export async function readStatus(dirs: AgentDirs): Promise<AgentStatus | null> {
  try {
    return JSON.parse(await readFile(path.join(dirs.data, 'status.json'), 'utf8')) as AgentStatus;
  } catch {
    return null;
  }
}

/** `server_url` from `agent.toml`, if set (a plain line scan; the file is tiny). */
export function configuredServer(dirs: AgentDirs): string | null {
  try {
    const text = readFileSync(path.join(dirs.config, 'agent.toml'), 'utf8');
    const m = /^\s*server_url\s*=\s*"([^"]*)"/m.exec(text);
    return m?.[1] || null;
  } catch {
    return null;
  }
}

/** The agent program: the setting, else `secretary-agent` on PATH. */
export function findAgent(setting: string, env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform): string | null {
  if (setting) return existsSync(setting) ? setting : null;
  const name = platform === 'win32' ? 'secretary-agent.exe' : 'secretary-agent';
  const sep = platform === 'win32' ? ';' : ':';
  for (const dir of (env.PATH ?? env.Path ?? '').split(sep)) {
    if (!dir) continue;
    const candidate = path.join(dir, name);
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

/** Runs the agent without a console window, detached from VS Code. */
function run(exe: string, args: string[], keepAlive: boolean): ChildProcess {
  const child = spawn(exe, args, {
    detached: keepAlive,
    stdio: 'ignore',
    windowsHide: true,
  });
  child.on('error', () => {});
  if (keepAlive) child.unref();
  return child;
}

/** `secretary-agent run`: a second daemon exits on its own (single-instance lock). */
export const startAgent = (exe: string) => void run(exe, ['run'], true);
/** Asks a running daemon to stop (it checks every 2 s). */
export const stopAgent = (exe: string) => void run(exe, ['stop'], false);

export function exitCode(child: ChildProcess): Promise<number> {
  return new Promise((resolve) => {
    child.on('exit', (code) => resolve(code ?? 1));
    child.on('error', () => resolve(1));
  });
}

export const logout = (exe: string) => exitCode(run(exe, ['logout'], false));

export type LoginEvent =
  | { kind: 'code'; url: string; userCode: string; expiresIn: number }
  | { kind: 'signed_in'; email: string | null }
  | { kind: 'error'; message: string };

/** One line of `secretary-agent login --json` (never contains tokens). */
export function parseLoginLine(line: string): LoginEvent | null {
  let v: Record<string, unknown>;
  try {
    v = JSON.parse(line) as Record<string, unknown>;
  } catch {
    return null;
  }
  const s = (k: string) => (typeof v[k] === 'string' ? (v[k] as string) : null);
  switch (v.event) {
    case 'code': {
      const url = s('verification_uri_complete') ?? s('verification_uri');
      const userCode = s('user_code');
      if (!url || !userCode) return null;
      return { kind: 'code', url, userCode, expiresIn: typeof v.expires_in === 'number' ? v.expires_in : 600 };
    }
    case 'signed_in':
      return { kind: 'signed_in', email: s('email') };
    case 'error':
      return { kind: 'error', message: s('message') ?? 'sign-in failed' };
    default:
      return null;
  }
}

/** A running `secretary-agent login --json`. The agent opens the browser itself. */
export class Login {
  private readonly child: ChildProcess;
  private finished = false;

  constructor(exe: string, server: string, onEvent: (e: LoginEvent) => void) {
    this.child = spawn(exe, ['login', '--json', '--server', server], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
    this.child.on('error', () => this.finish(onEvent, { kind: 'error', message: 'could not start the agent' }));
    createInterface({ input: this.child.stdout! }).on('line', (line) => {
      const event = parseLoginLine(line);
      if (event?.kind === 'code') onEvent(event);
      else if (event) this.finish(onEvent, event);
    });
    this.child.on('exit', () => this.finish(onEvent, { kind: 'error', message: 'sign-in ended unexpectedly' }));
  }

  private finish(onEvent: (e: LoginEvent) => void, event: LoginEvent): void {
    if (this.finished) return;
    this.finished = true;
    onEvent(event);
  }

  cancel(): void {
    this.finished = true;
    this.child.kill();
  }
}
