// Sidebar panel (activity bar > CodeSecretary): agent status and connection
// controls. The webview only renders state and posts button clicks; all
// actions run here, through the commands also listed in the command palette.
import * as vscode from 'vscode';

import * as control from './control';
import type { AgentStatus } from './generated/contract';
import { html } from './panelView';
import type { SenderState } from './sender';

export const VIEW_ID = 'codesecretary.panel';
const POLL_MS = 2_000;

export interface EditorInfo {
  enabled: boolean;
  project: string | null;
  sender: SenderState;
}

type LoginState =
  | { state: 'idle' }
  | { state: 'waiting'; url: string | null; userCode: string | null }
  | { state: 'error'; message: string };

export interface PanelState {
  agentFound: boolean;
  live: boolean;
  status: AgentStatus | null;
  login: LoginState;
  editor: EditorInfo;
  dashboardUrl: string | null;
}

export class Panel implements vscode.WebviewViewProvider, vscode.Disposable {
  private view: vscode.WebviewView | null = null;
  private timer: NodeJS.Timeout | null = null;
  private status: AgentStatus | null = null;
  private login: LoginState = { state: 'idle' };
  private running: control.Login | null = null;
  private readonly dirs = control.agentDirs();

  constructor(private readonly editor: () => EditorInfo) {}

  register(context: vscode.ExtensionContext): void {
    const cmd = (id: string, fn: () => unknown) => vscode.commands.registerCommand(id, fn);
    context.subscriptions.push(
      this,
      vscode.window.registerWebviewViewProvider(VIEW_ID, this),
      cmd('codesecretary.signIn', () => this.signIn()),
      cmd('codesecretary.cancelSignIn', () => this.cancelSignIn()),
      cmd('codesecretary.signOut', () => this.signOut()),
      cmd('codesecretary.startAgent', () => this.withAgent((exe) => control.startAgent(exe))),
      cmd('codesecretary.stopAgent', () => this.withAgent((exe) => control.stopAgent(exe))),
      cmd('codesecretary.toggleTracking', () => this.toggleTracking()),
      cmd('codesecretary.openDashboard', () => this.openDashboard()),
      cmd('codesecretary.showStatus', () => vscode.commands.executeCommand(`${VIEW_ID}.focus`)),
    );
  }

  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view;
    view.webview.options = { enableScripts: true, localResourceRoots: [] };
    view.webview.html = html(view.webview.cspSource);
    view.webview.onDidReceiveMessage((m: { cmd?: string; url?: string }) => this.onMessage(m));
    view.onDidChangeVisibility(() => this.schedule());
    view.onDidDispose(() => {
      this.view = null;
      this.schedule();
    });
    this.schedule();
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.running?.cancel();
  }

  /** Polls `status.json` only while the panel is visible. */
  private schedule(): void {
    const visible = this.view?.visible ?? false;
    if (visible && !this.timer) {
      this.timer = setInterval(() => void this.refresh(), POLL_MS);
      void this.refresh();
    } else if (!visible && this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  async refresh(): Promise<void> {
    this.status = await control.readStatus(this.dirs);
    this.render();
  }

  render(): void {
    if (!this.view) return;
    const state: PanelState = {
      agentFound: this.exe() !== null,
      live: control.isLive(this.status),
      status: this.status,
      login: this.login,
      editor: this.editor(),
      dashboardUrl: setting<string>('dashboardUrl', '') || null,
    };
    void this.view.webview.postMessage({ type: 'state', state });
  }

  private onMessage(m: { cmd?: string; url?: string }): void {
    const commands: Record<string, string> = {
      signIn: 'codesecretary.signIn',
      cancelSignIn: 'codesecretary.cancelSignIn',
      signOut: 'codesecretary.signOut',
      start: 'codesecretary.startAgent',
      stop: 'codesecretary.stopAgent',
      toggleTracking: 'codesecretary.toggleTracking',
      openDashboard: 'codesecretary.openDashboard',
    };
    if (m.cmd === 'ready' || m.cmd === 'refresh') void this.refresh();
    else if (m.cmd === 'openLink' && m.url && /^https?:\/\//.test(m.url)) void vscode.env.openExternal(vscode.Uri.parse(m.url));
    else if (m.cmd === 'settings') void vscode.commands.executeCommand('workbench.action.openSettings', 'codesecretary');
    else if (m.cmd && commands[m.cmd]) void vscode.commands.executeCommand(commands[m.cmd]!);
  }

  // --- actions -----------------------------------------------------------

  private exe(): string | null {
    return control.findAgent(setting<string>('agentPath', ''));
  }

  private withAgent(action: (exe: string) => void): void {
    const exe = this.exe();
    if (!exe) {
      void vscode.window.showWarningMessage(
        'CodeSecretary agent not found. Install it, or set "codesecretary.agentPath" to secretary-agent.',
        'Open Settings',
      ).then((pick) => pick && vscode.commands.executeCommand('workbench.action.openSettings', 'codesecretary.agentPath'));
      return;
    }
    action(exe);
    setTimeout(() => void this.refresh(), 1_500);
    setTimeout(() => void this.refresh(), 6_000);
  }

  private async signIn(): Promise<void> {
    const exe = this.exe();
    if (!exe) return this.withAgent(() => {});
    if (this.running) return;
    const known = this.status?.account?.server_url || control.configuredServer(this.dirs) || '';
    const server = await vscode.window.showInputBox({
      title: 'Sign in to CodeSecretary',
      prompt: 'Server address (the warehouse URL your team gave you)',
      value: known,
      ignoreFocusOut: true,
      validateInput: (v) => (/^https?:\/\/\S+$/.test(v.trim()) ? null : 'Starts with https:// (or http:// for a local test server)'),
    });
    if (!server) return;
    this.login = { state: 'waiting', url: null, userCode: null };
    this.render();
    this.running = new control.Login(exe, server.trim(), (event) => {
      if (event.kind === 'code') {
        this.login = { state: 'waiting', url: event.url, userCode: event.userCode };
      } else {
        this.running = null;
        if (event.kind === 'signed_in') {
          this.login = { state: 'idle' };
          void vscode.window.showInformationMessage(`CodeSecretary: signed in${event.email ? ` as ${event.email}` : ''}.`);
          if (!control.isLive(this.status)) control.startAgent(exe);
        } else {
          this.login = { state: 'error', message: event.message };
        }
      }
      void this.refresh();
    });
  }

  private cancelSignIn(): void {
    this.running?.cancel();
    this.running = null;
    this.login = { state: 'idle' };
    this.render();
  }

  private async signOut(): Promise<void> {
    const exe = this.exe();
    if (!exe) return this.withAgent(() => {});
    const pick = await vscode.window.showWarningMessage(
      'Sign this device out of CodeSecretary? Collected activity stays on this device until you sign in again.',
      { modal: true }, 'Sign out');
    if (pick !== 'Sign out') return;
    await control.logout(exe);
    void this.refresh();
  }

  private async toggleTracking(): Promise<void> {
    const config = vscode.workspace.getConfiguration('codesecretary');
    await config.update('enabled', !config.get<boolean>('enabled', true), vscode.ConfigurationTarget.Global);
    this.render();
  }

  private async openDashboard(): Promise<void> {
    let url = setting<string>('dashboardUrl', '');
    if (!url) {
      url = (await vscode.window.showInputBox({
        title: 'CodeSecretary dashboard',
        prompt: 'Dashboard address, e.g. https://time.example.com',
        ignoreFocusOut: true,
        validateInput: (v) => (/^https?:\/\/\S+$/.test(v.trim()) ? null : 'Starts with https://'),
      }))?.trim() ?? '';
      if (!url) return;
      await vscode.workspace.getConfiguration('codesecretary').update('dashboardUrl', url, vscode.ConfigurationTarget.Global);
    }
    void vscode.env.openExternal(vscode.Uri.parse(url));
  }
}

function setting<T>(key: string, fallback: T): T {
  return vscode.workspace.getConfiguration('codesecretary').get<T>(key, fallback);
}
