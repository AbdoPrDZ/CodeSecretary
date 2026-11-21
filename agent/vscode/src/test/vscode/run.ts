// End-to-end: a test agent + a downloaded VS Code (@vscode/test-electron)
// running index.ts, then checks what reached the agent's buffer.
//
//   cargo build -p secretary-agent
//   corepack pnpm --filter codesecretary test:vscode
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { runTests } from '@vscode/test-electron';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function main(): Promise<void> {
  const ext = path.resolve(__dirname, '..', '..', '..');
  const repoRoot = path.resolve(ext, '..', '..');
  const exe = path.join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'secretary-agent.exe' : 'secretary-agent');
  if (!existsSync(exe)) throw new Error(`build the agent first: ${exe}`);

  const tmp = mkdtempSync(path.join(os.tmpdir(), 'cs-vscode-'));
  const home = path.join(tmp, 'home');
  const ws = path.join(tmp, 'api');
  mkdirSync(path.join(home, 'config'), { recursive: true });
  mkdirSync(path.join(ws, 'src'), { recursive: true });
  // No server: events stay in the buffer. No repository scan outside tmp.
  writeFileSync(path.join(home, 'config', 'agent.toml'),
    `server_url = "http://127.0.0.1:9"\nproject_roots = [${JSON.stringify(path.join(tmp, 'none'))}]\n`);
  writeFileSync(path.join(ws, 'src', 'main.rs'), 'fn main() {}\n');
  spawnSync('git', ['init', '-q'], { cwd: ws });
  spawnSync('git', ['remote', 'add', 'origin', 'https://token123@github.com/acme/api.git'], { cwd: ws });

  const env = { ...process.env, SECRETARY_HOME: home };
  const agent = spawn(exe, ['run', '--run-for', '300'], { env, stdio: 'ignore' });
  const localJson = path.join(home, 'config', 'local.json');
  for (let i = 0; i < 100 && !existsSync(localJson); i++) await wait(100);
  if (!existsSync(localJson)) throw new Error('agent did not start its local API');

  let code = 1;
  try {
    // Clean environment: variables of a parent VS Code would redirect the launch.
    const testEnv = Object.fromEntries(Object.keys(process.env)
      .filter((k) => k.startsWith('VSCODE_') || k === 'ELECTRON_RUN_AS_NODE')
      .map((k) => [k, undefined]));
    code = await runTests({
      extensionDevelopmentPath: ext,
      extensionTestsPath: path.join(__dirname, 'index.js'),
      extensionTestsEnv: { ...testEnv, SECRETARY_HOME: home },
      launchArgs: [ws, '--disable-extensions', '--disable-workspace-trust', '--skip-welcome',
        `--user-data-dir=${path.join(tmp, 'ud')}`],
    });
  } finally {
    writeFileSync(path.join(home, 'data', 'stop.request'), '');
    for (let i = 0; i < 50 && agent.exitCode === null; i++) await wait(100);
    if (agent.exitCode === null) agent.kill();
  }
  if (code !== 0) throw new Error(`VS Code tests failed (${code})`);

  const db = new DatabaseSync(path.join(home, 'data', 'buffer.sqlite3'), { readOnly: true });
  const rows = db.prepare("SELECT payload FROM events WHERE payload LIKE '%\"vscode@%' ORDER BY seq").all();
  const events = rows.map((r) => JSON.parse(String((r as { payload: string }).payload)) as {
    event_type: string; repository: { root_path: string; remote_url: string } | null; metadata: Record<string, unknown>;
  });
  for (const e of events) console.log(e.event_type, JSON.stringify(e.repository), JSON.stringify(e.metadata));

  const of = (t: string) => events.filter((e) => e.event_type === t);
  const check = (ok: boolean, what: string) => {
    if (!ok) throw new Error(`missing: ${what}`);
  };
  check(of('editor.workspace_open').length === 1, 'one workspace_open');
  check(of('editor.save').some((e) => e.metadata.file_path === 'src/main.rs'), 'save of src/main.rs');
  check(of('editor.edit_burst').reduce((n, e) => n + Number(e.metadata.edit_count), 0) === 5, '5 edits counted');
  check(events.every((e) => e.repository?.remote_url === 'github.com/acme/api'), 'normalized remote without token');
  check(!JSON.stringify(events).includes('edit 0'), 'no file contents');
  console.log(`ok: ${events.length} events (file_focus: ${of('editor.file_focus').length}, needs a focused window)`);
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
