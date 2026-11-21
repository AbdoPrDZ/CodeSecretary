// CodeSecretary for VS Code (AG-13): reports which file has focus, edit
// counts, saves, debug sessions and opened workspaces to the local agent.
//
// Never read or sent: file contents, the text of edits, selections,
// clipboard, terminal output. Only `file:` documents are reported; paths are
// relative to their repository (or workspace folder) root.
import * as vscode from 'vscode';

import { AgentClient, localApiFile } from './agent';
import * as ev from './events';
import type { LocalAgentState } from './generated/contract';
import { Panel } from './panel';
import { canonical, RepoFinder, type FileRef, type Repo } from './repo';
import { Sender } from './sender';
import { EditAggregator, FocusTracker, MAX_INTERVAL_MS, type Focused } from './trackers';

const TICK_MS = 5_000;
const STATE_EVERY_MS = 30_000;
const REPO_CACHE_MS = 5 * 60_000;

let tracker: Tracker | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const version = String(context.extension.packageJSON.version ?? '0.0.0');
  const t = new Tracker(ev.source(version), new AgentClient(localApiFile(), ev.COLLECTOR));
  tracker = t;
  context.subscriptions.push(t);
  t.panel = new Panel(() => ({ enabled: t.enabled, project: t.project(), sender: t.sender.state }));
  t.panel.register(context);
  t.start(context);
}

/** VS Code waits (briefly) for this: open intervals are closed and sent. */
export function deactivate(): Promise<void> | undefined {
  return tracker?.stop();
}

class Tracker implements vscode.Disposable {
  private readonly repos = new RepoFinder();
  private readonly focus = new FocusTracker();
  private readonly edits = new EditAggregator();
  private readonly debug = new Map<string, { name: string | null; repo: Repo | null; start: number }>();
  readonly sender: Sender;
  panel: Panel | null = null;
  private readonly status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  private agent: LocalAgentState | null = null;
  private timers: NodeJS.Timeout[] = [];
  enabled = true;

  constructor(private readonly src: string, private readonly client: AgentClient) {
    this.sender = new Sender(client, () => this.render());
  }

  start(context: vscode.ExtensionContext): void {
    this.enabled = vscode.workspace.getConfiguration('codesecretary').get<boolean>('enabled', true);
    this.status.command = 'codesecretary.showStatus'; // focuses the sidebar panel
    this.status.show();
    const subs = context.subscriptions;
    subs.push(
      this.status,
      vscode.window.onDidChangeActiveTextEditor(() => this.refocus()),
      vscode.window.onDidChangeWindowState(() => this.refocus()),
      vscode.workspace.onDidChangeTextDocument((e) => this.onEdit(e)),
      vscode.workspace.onDidSaveTextDocument((doc) => this.onSave(doc)),
      vscode.debug.onDidStartDebugSession((s) => this.onDebugStart(s)),
      vscode.debug.onDidTerminateDebugSession((s) => this.onDebugEnd(s)),
      vscode.workspace.onDidChangeWorkspaceFolders((e) => {
        this.repos.clear();
        this.workspaceOpened(e.added);
      }),
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration('codesecretary.enabled')) this.setEnabled();
      }),
    );
    this.workspaceOpened(vscode.workspace.workspaceFolders ?? []);
    this.refocus();
    this.timers.push(
      setInterval(() => this.tick(), TICK_MS),
      setInterval(() => void this.refreshState(), STATE_EVERY_MS),
      setInterval(() => this.repos.clear(), REPO_CACHE_MS),
    );
    void this.refreshState();
    this.render();
  }

  private stopped: Promise<void> | null = null;

  stop(): Promise<void> {
    if (!this.stopped) {
      for (const t of this.timers) clearInterval(t);
      this.timers = [];
      this.closeAll(Date.now());
      this.stopped = this.sender.flush();
    }
    return this.stopped;
  }

  dispose(): void {
    void this.stop();
  }

  // --- signals ---------------------------------------------------------

  private fileRef(doc: vscode.TextDocument): FileRef | null {
    if (doc.uri.scheme !== 'file') return null;
    const folder = vscode.workspace.getWorkspaceFolder(doc.uri);
    return this.repos.locate(doc.uri.fsPath, folder?.uri.scheme === 'file' ? folder.uri.fsPath : null);
  }

  private current(): Focused | null {
    const state = vscode.window.state;
    // `active`: focused and recently used (VS Code's own inactivity signal).
    if (!state.focused || !state.active) return null;
    const doc = vscode.window.activeTextEditor?.document;
    const file = doc ? this.fileRef(doc) : null;
    return doc && file ? { file, language: doc.languageId || null } : null;
  }

  private refocus(): void {
    if (!this.enabled) return;
    const now = Date.now();
    for (const i of this.focus.update(this.current(), now)) {
      this.emit(ev.fileFocus(this.src, i.file, i.language, i.start, i.end));
    }
    this.render();
  }

  private onEdit(e: vscode.TextDocumentChangeEvent): void {
    // Counts only; `contentChanges` text is never looked at. Undo/redo count too.
    if (!this.enabled || e.contentChanges.length === 0) return;
    if (vscode.window.activeTextEditor?.document !== e.document) return; // typing, not background rewrites
    const file = this.fileRef(e.document);
    if (file) this.edits.edit(file, Date.now());
  }

  private onSave(doc: vscode.TextDocument): void {
    if (!this.enabled) return;
    const file = this.fileRef(doc);
    if (file) this.emit(ev.save(this.src, file, Date.now()));
  }

  private onDebugStart(session: vscode.DebugSession): void {
    if (!this.enabled) return;
    const folder = session.workspaceFolder;
    const repo = folder?.uri.scheme === 'file' ? this.folderRepo(folder.uri.fsPath) : null;
    this.debug.set(session.id, { name: session.configuration.name || session.name || null, repo, start: Date.now() });
  }

  private onDebugEnd(session: vscode.DebugSession): void {
    const open = this.debug.get(session.id);
    if (!open) return;
    this.debug.delete(session.id);
    this.emit(ev.debugSession(this.src, open.name, open.repo, open.start, Date.now()));
  }

  private folderRepo(folder: string): Repo {
    return this.repos.gitRepo(folder) ?? { root: canonical(folder), remote: null, git: false };
  }

  private workspaceOpened(folders: readonly vscode.WorkspaceFolder[]): void {
    if (!this.enabled) return;
    const now = Date.now();
    for (const folder of folders) {
      if (folder.uri.scheme !== 'file') continue;
      const repo = this.folderRepo(folder.uri.fsPath);
      this.emit(ev.workspaceOpen(this.src, canonical(folder.uri.fsPath), repo, now));
    }
  }

  // --- periodic work ---------------------------------------------------

  private tick(): void {
    const now = Date.now();
    if (this.enabled) {
      // Window "active" goes false after inactivity without an event: poll it.
      for (const i of this.focus.update(this.current(), now)) {
        this.emit(ev.fileFocus(this.src, i.file, i.language, i.start, i.end));
      }
      for (const b of this.edits.flush(now)) this.emit(ev.editBurst(this.src, b.file, b.count, b.start, b.end));
      // Debug sessions longer than the cut are reported in pieces.
      for (const [id, open] of this.debug) {
        if (now - open.start >= MAX_INTERVAL_MS) {
          this.emit(ev.debugSession(this.src, open.name, open.repo, open.start, now));
          this.debug.set(id, { ...open, start: now });
        }
      }
    }
    void this.sender.flush(now);
  }

  private closeAll(now: number): void {
    for (const i of this.focus.finish(now)) this.emit(ev.fileFocus(this.src, i.file, i.language, i.start, i.end));
    for (const b of this.edits.flush(now, true)) this.emit(ev.editBurst(this.src, b.file, b.count, b.start, b.end));
    for (const open of this.debug.values()) this.emit(ev.debugSession(this.src, open.name, open.repo, open.start, now));
    this.debug.clear();
  }

  private setEnabled(): void {
    const enabled = vscode.workspace.getConfiguration('codesecretary').get<boolean>('enabled', true);
    if (enabled === this.enabled) return;
    if (!enabled) this.closeAll(Date.now());
    this.enabled = enabled;
    if (enabled) this.refocus();
    this.render();
  }

  private emit(event: ReturnType<typeof ev.makeEvent>): void {
    this.sender.push(event);
  }

  private async refreshState(): Promise<void> {
    try {
      this.agent = await this.client.state();
    } catch {
      this.agent = null;
    }
    this.render();
  }

  // --- status bar ------------------------------------------------------

  project(): string | null {
    const doc = vscode.window.activeTextEditor?.document;
    const file = doc ? this.fileRef(doc) : null;
    if (!file) return null;
    const remote = file.repo.remote?.replace(/\.git$/, '').split(/[/:]/).filter(Boolean).pop();
    return remote ?? file.repo.root.split(/[\\/]/).filter(Boolean).pop() ?? null;
  }

  private render(): void {
    this.panel?.render();
    const s = this.sender.state;
    const item = this.status;
    item.backgroundColor = undefined;
    if (!this.enabled) {
      item.text = '$(circle-slash) CodeSecretary off';
      item.tooltip = 'Editor activity is not sent (setting codesecretary.enabled).';
    } else if (s.link === 'unreachable' || (this.agent === null && s.link !== 'connected')) {
      item.text = '$(warning) CodeSecretary: agent offline';
      item.tooltip = `The CodeSecretary agent is not running or not reachable. Activity is kept for 5 minutes, then dropped.${
        s.dropped ? ` Dropped so far: ${s.dropped}.` : ''}`;
      if (s.dropped) item.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
    } else if (this.agent?.paused) {
      item.text = '$(debug-pause) Tracking paused';
      item.tooltip = 'Inside a pause window: nothing is collected.';
    } else {
      const project = this.project();
      item.text = `$(pulse) ${project ?? 'CodeSecretary'}`;
      item.tooltip = this.agent?.connection === 'signed_out'
        ? 'Tracking, but the agent is signed out: sign in from the CodeSecretary widget to upload.'
        : 'Tracking editor activity (never file contents).';
    }
  }

}
