import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { describe, it } from 'node:test';

import { AgentError, localApiFile, type Transport } from '../agent';
import * as control from '../control';
import * as ev from '../events';
import type { Event, LocalEventsResponse } from '../generated/contract';
import { uuidv7 } from '../ids';
import { canonical, remoteFromConfig, RepoFinder, type FileRef } from '../repo';
import { RETAIN_MS, Sender } from '../sender';
import { EDIT_WINDOW_MS, EditAggregator, FocusTracker, MAX_INTERVAL_MS } from '../trackers';

const T0 = Date.parse('2026-11-21T09:00:00Z');
const file = (rel: string): FileRef => ({ repo: { root: '/work/api', remote: null, git: true }, relPath: rel });

describe('ids', () => {
  it('are UUIDv7 carrying the time', () => {
    const id = uuidv7(T0);
    assert.match(id, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    assert.equal(parseInt(id.replace(/-/g, '').slice(0, 12), 16), T0);
    assert.notEqual(uuidv7(T0), id);
  });
});

describe('local.json location', () => {
  it('matches the daemon on each OS', () => {
    assert.equal(localApiFile({ APPDATA: 'C:\\Users\\a\\AppData\\Roaming' }, 'win32', 'C:\\Users\\a'),
      'C:\\Users\\a\\AppData\\Roaming\\secretary\\config\\local.json');
    assert.equal(localApiFile({}, 'darwin', '/Users/a'), '/Users/a/Library/Application Support/secretary/local.json');
    assert.equal(localApiFile({}, 'linux', '/home/a'), '/home/a/.config/secretary/local.json');
    assert.equal(localApiFile({ XDG_CONFIG_HOME: '/x' }, 'linux', '/home/a'), '/x/secretary/local.json');
    assert.equal(localApiFile({ XDG_CONFIG_HOME: 'rel' }, 'linux', '/home/a'), '/home/a/.config/secretary/local.json');
    assert.equal(localApiFile({ SECRETARY_HOME: '/tmp/s' }, 'linux', '/home/a'), '/tmp/s/config/local.json');
  });
});

describe('repositories', () => {
  it('reads the origin remote like the Git tracker', () => {
    const config = '[core]\n\tbare = false\n[remote "upstream"]\n\turl = git@x:u/a.git\n[remote "origin"]\n\turl = https://github.com/acme/api.git\n';
    assert.equal(remoteFromConfig(config), 'https://github.com/acme/api.git');
    assert.equal(remoteFromConfig('[remote "fork"]\n url = a\n'), 'a');
    assert.equal(remoteFromConfig('[core]\n url = nope\n'), null);
  });

  it('finds the work tree, relative paths, and skips .git and outside files', () => {
    const tmp = mkdtempSync(path.join(os.tmpdir(), 'cs-repo-'));
    const repo = path.join(tmp, 'api');
    mkdirSync(path.join(repo, '.git'), { recursive: true });
    mkdirSync(path.join(repo, 'src', 'deep'), { recursive: true });
    writeFileSync(path.join(repo, '.git', 'config'), '[remote "origin"]\n\turl = git@github.com:acme/api.git\n');
    const finder = new RepoFinder();

    const ref = finder.locate(path.join(repo, 'src', 'deep', 'main.rs'), null)!;
    assert.equal(ref.relPath, 'src/deep/main.rs');
    assert.equal(ref.repo.root, canonical(repo));
    assert.equal(ref.repo.remote, 'git@github.com:acme/api.git');
    assert.equal(ref.repo.git, true);
    assert.equal(finder.locate(path.join(repo, '.git', 'COMMIT_EDITMSG'), null), null);

    // A worktree: `.git` is a file pointing at the main repository's git dir.
    const wt = path.join(tmp, 'wt');
    const wtGit = path.join(repo, '.git', 'worktrees', 'wt');
    mkdirSync(wtGit, { recursive: true });
    writeFileSync(path.join(wtGit, 'commondir'), '../..');
    mkdirSync(wt);
    writeFileSync(path.join(wt, '.git'), `gitdir: ${wtGit}\n`);
    assert.equal(finder.locate(path.join(wt, 'a.ts'), null)!.repo.remote, 'git@github.com:acme/api.git');

    // Outside Git: relative to the workspace folder, or nothing.
    const plain = path.join(tmp, 'notes');
    mkdirSync(plain);
    const loose = finder.locate(path.join(plain, 'todo.md'), plain)!;
    assert.deepEqual([loose.relPath, loose.repo.git, loose.repo.remote], ['todo.md', false, null]);
    assert.equal(finder.locate(path.join(plain, 'todo.md'), null), null);
  });
});

describe('focus tracker', () => {
  it('opens, switches, drops flicks, cuts long intervals and closes', () => {
    const t = new FocusTracker();
    const a = { file: file('a.ts'), language: 'typescript' };
    const b = { file: file('b.ts'), language: 'typescript' };
    assert.deepEqual(t.update(a, T0), []);
    assert.deepEqual(t.update(a, T0 + 10_000), []);
    const switched = t.update(b, T0 + 60_000);
    assert.deepEqual(switched.map((i) => [i.file.relPath, i.start, i.end]), [['a.ts', T0, T0 + 60_000]]);
    // A flick to `a` for one second is dropped.
    assert.deepEqual(t.update(a, T0 + 120_000).map((i) => i.file.relPath), ['b.ts']);
    assert.deepEqual(t.update(b, T0 + 121_000), []);
    // Long focus is cut every MAX_INTERVAL_MS.
    const cut = t.tick(T0 + 121_000 + MAX_INTERVAL_MS);
    assert.equal(cut.length, 1);
    assert.equal(cut[0]!.end - cut[0]!.start, MAX_INTERVAL_MS);
    // Window loses focus: closed.
    assert.equal(t.update(null, T0 + 121_000 + MAX_INTERVAL_MS + 30_000).length, 1);
    assert.deepEqual(t.finish(T0 + 999_999), []);
  });
});

describe('edit aggregator', () => {
  it('sums edits per file over the window', () => {
    const agg = new EditAggregator();
    agg.edit(file('a.ts'), T0);
    agg.edit(file('a.ts'), T0 + 5_000);
    agg.edit(file('b.ts'), T0 + 20_000);
    assert.deepEqual(agg.flush(T0 + 10_000), []);
    const first = agg.flush(T0 + EDIT_WINDOW_MS);
    assert.deepEqual(first.map((b) => [b.file.relPath, b.count, b.start, b.end]), [['a.ts', 2, T0, T0 + 5_000]]);
    assert.deepEqual(agg.flush(T0 + 31_000, true).map((b) => b.count), [1]);
  });
});

describe('events', () => {
  it('use the shared envelope with relative paths and no contents', () => {
    const src = ev.source('0.1.0');
    const f = file('src/main.rs');
    const focus = ev.fileFocus(src, f, 'rust', T0, T0 + 5_000);
    assert.equal(focus.event_type, 'editor.file_focus');
    assert.equal(focus.source, 'vscode@0.1.0');
    assert.deepEqual(focus.repository, { root_path: '/work/api', remote_url: null });
    assert.deepEqual(focus.metadata, { file_path: 'src/main.rs', language: 'rust' });
    assert.equal(focus.timestamp_end, '2026-11-21T09:00:05.000Z');
    const saved = ev.save(src, f, T0);
    assert.equal(saved.timestamp_end, null);
    assert.deepEqual(Object.keys(ev.editBurst(src, f, 3, T0, T0).metadata), ['file_path', 'edit_count']);
  });

  it('extension code never reads document text', () => {
    // Guard for the privacy rule: no API that returns file contents is used.
    const dir = path.join(__dirname, '..', '..', 'src');
    for (const name of readdirSync(dir).filter((n) => n.endsWith('.ts'))) {
      const code = readFileSync(path.join(dir, name), 'utf8');
      for (const banned of ['getText(', 'lineAt(', '.text)', '.text;', 'rangeLength', 'env.clipboard', 'readFile(doc']) {
        assert.ok(!code.includes(banned), `${name} uses ${banned}`);
      }
    }
  });
});

class FakeTransport implements Transport {
  sent: Event[][] = [];
  fail: AgentError | null = null;
  async send(events: Event[]): Promise<LocalEventsResponse> {
    if (this.fail) throw this.fail;
    this.sent.push(events);
    return { accepted: events.length, rejected: [] };
  }
  async state() {
    return { version: '0.1.0', paused: false, connection: 'online' as const };
  }
}

describe('sender', () => {
  const event = () => ev.save(ev.source('0.1.0'), file('a.ts'), T0);

  it('keeps events while the agent is down, sends them once it is back', async () => {
    const transport = new FakeTransport();
    const sender = new Sender(transport);
    transport.fail = new AgentError('unreachable', 'down');
    sender.push(event(), T0);
    sender.push(event(), T0 + 1_000);
    await sender.flush(T0 + 2_000);
    assert.equal(sender.state.link, 'unreachable');
    assert.equal(sender.state.queued, 2);
    transport.fail = null;
    await sender.flush(T0 + 3_000);
    assert.equal(sender.state.link, 'connected');
    assert.equal(sender.state.queued, 0);
    assert.equal(transport.sent.flat().length, 2);
  });

  it('drops events older than the retention while unreachable', async () => {
    const transport = new FakeTransport();
    const sender = new Sender(transport);
    transport.fail = new AgentError('unreachable', 'down');
    sender.push(event(), T0);
    sender.push(event(), T0 + RETAIN_MS);
    await sender.flush(T0 + RETAIN_MS + 1);
    assert.equal(sender.state.dropped, 1);
    assert.equal(sender.state.queued, 1);
  });

  it('does not retry a batch the agent refuses', async () => {
    const transport = new FakeTransport();
    const sender = new Sender(transport);
    transport.fail = new AgentError('refused', 'HTTP 400');
    sender.push(event(), T0);
    await sender.flush(T0);
    assert.equal(sender.state.queued, 0);
    assert.equal(sender.state.rejected, 1);
  });
});

describe('agent control', () => {
  it('finds the agent directories like the daemon', () => {
    assert.deepEqual(control.agentDirs({ APPDATA: String.raw`C:\R`, LOCALAPPDATA: String.raw`C:\L` }, 'win32', String.raw`C:\Users\a`),
      { config: String.raw`C:\R\secretary\config`, data: String.raw`C:\L\secretary\data` });
    const mac = '/Users/a/Library/Application Support/secretary';
    assert.deepEqual(control.agentDirs({}, 'darwin', '/Users/a'), { config: mac, data: mac });
    assert.deepEqual(control.agentDirs({ XDG_DATA_HOME: '/d' }, 'linux', '/home/a'),
      { config: '/home/a/.config/secretary', data: '/d/secretary' });
    assert.deepEqual(control.agentDirs({ SECRETARY_HOME: '/s' }, 'linux', '/home/a'), { config: '/s/config', data: '/s/data' });
  });

  it('parses login --json lines', () => {
    assert.deepEqual(control.parseLoginLine('{"event":"code","verification_uri":"https://k/device","verification_uri_complete":"https://k/device?c=AB","user_code":"AB-CD","expires_in":600}'),
      { kind: 'code', url: 'https://k/device?c=AB', userCode: 'AB-CD', expiresIn: 600 });
    assert.deepEqual(control.parseLoginLine('{"event":"signed_in","email":"a@b.c","device_id":3}'), { kind: 'signed_in', email: 'a@b.c' });
    assert.deepEqual(control.parseLoginLine('{"event":"error","message":"denied"}'), { kind: 'error', message: 'denied' });
    assert.equal(control.parseLoginLine('not json'), null);
    assert.equal(control.parseLoginLine('{"event":"code"}'), null);
  });

  it('treats an old or stopped status as not running', () => {
    const now = Date.parse('2026-11-21T10:00:00Z');
    const status = { running: true, written_at: '2026-11-21T09:59:55Z' } as Parameters<typeof control.isLive>[0];
    assert.equal(control.isLive(status, now), true);
    assert.equal(control.isLive({ ...status!, written_at: '2026-11-21T09:59:00Z' }, now), false);
    assert.equal(control.isLive({ ...status!, running: false }, now), false);
    assert.equal(control.isLive(null, now), false);
  });

  it('reads server_url from agent.toml', () => {
    const tmp = mkdtempSync(path.join(os.tmpdir(), 'cs-cfg-'));
    writeFileSync(path.join(tmp, 'agent.toml'), 'log_level = "info"\nserver_url = "https://w.example:8443"\n');
    assert.equal(control.configuredServer({ config: tmp, data: tmp }), 'https://w.example:8443');
    assert.equal(control.configuredServer({ config: path.join(tmp, 'none'), data: tmp }), null);
  });
});
