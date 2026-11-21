// The panel's webview page: styles from VS Code's theme variables, and a small
// script that renders posted state with DOM text nodes (no HTML from data).
import { randomBytes } from 'node:crypto';

export function html(cspSource: string): string {
  const nonce = randomBytes(16).toString('base64');
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} 'nonce-${nonce}'; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style nonce="${nonce}">${CSS}</style>
</head>
<body>
<div id="root"><p class="muted">Loading...</p></div>
<script nonce="${nonce}">${SCRIPT}</script>
</body>
</html>`;
}

const CSS = `
:root { --gap: 10px; }
*, *::before, *::after { box-sizing: border-box; }
body { padding: 10px 12px 16px; color: var(--vscode-foreground); font-family: var(--vscode-font-family); font-size: var(--vscode-font-size); }
.card { border: 1px solid var(--vscode-widget-border, var(--vscode-panel-border, transparent)); border-radius: 8px; padding: 10px 12px; margin-bottom: var(--gap);
  background: var(--vscode-sideBarSectionHeader-background, transparent); }
.hero { display: flex; align-items: center; gap: 10px; }
.dot { width: 10px; height: 10px; border-radius: 50%; flex: none; box-shadow: 0 0 0 3px color-mix(in srgb, currentColor 22%, transparent); }
.dot.green { color: var(--vscode-testing-iconPassed, #3fb950); background: currentColor; }
.dot.amber { color: var(--vscode-editorWarning-foreground, #d29922); background: currentColor; }
.dot.red { color: var(--vscode-errorForeground, #f85149); background: currentColor; }
.dot.gray { color: var(--vscode-disabledForeground, #8b949e); background: currentColor; }
.pulse { animation: pulse 2s ease-in-out infinite; }
@keyframes pulse { 50% { box-shadow: 0 0 0 6px color-mix(in srgb, currentColor 0%, transparent); } }
.title { font-weight: 600; font-size: 1.08em; }
.sub { color: var(--vscode-descriptionForeground); margin-top: 2px; word-break: break-word; }
h3 { margin: 0 0 6px; font-size: 0.78em; letter-spacing: 0.06em; text-transform: uppercase; color: var(--vscode-descriptionForeground); font-weight: 600; }
dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 0; }
dt { color: var(--vscode-descriptionForeground); }
dd { margin: 0; text-align: right; font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.row { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
button { font: inherit; border: 1px solid var(--vscode-button-border, transparent); border-radius: 4px; padding: 4px 11px; cursor: pointer;
  color: var(--vscode-button-secondaryForeground); background: var(--vscode-button-secondaryBackground); }
button:hover { background: var(--vscode-button-secondaryHoverBackground); }
button.primary { color: var(--vscode-button-foreground); background: var(--vscode-button-background); }
button.primary:hover { background: var(--vscode-button-hoverBackground); }
button.grow { flex: 1; }
button:focus-visible { outline: 1px solid var(--vscode-focusBorder); outline-offset: 2px; }
.code { font-family: var(--vscode-editor-font-family); font-size: 1.6em; letter-spacing: 0.18em; text-align: center; padding: 8px 0 4px; user-select: all; font-weight: 600; }
.muted { color: var(--vscode-descriptionForeground); }
.error { color: var(--vscode-errorForeground); }
.toggle { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.switch { width: 34px; height: 18px; border-radius: 9px; padding: 0; position: relative; flex: none; background: var(--vscode-input-background); border: 1px solid var(--vscode-input-border, var(--vscode-widget-border, #888)); }
.switch::after { content: ''; position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: 50%; background: var(--vscode-descriptionForeground); transition: left 120ms; }
.switch.on { background: var(--vscode-button-background); border-color: var(--vscode-button-background); }
.switch.on::after { left: 18px; background: var(--vscode-button-foreground); }
.spacer { height: 8px; }
.note { font-size: 0.9em; color: var(--vscode-descriptionForeground); margin: 8px 2px 0; line-height: 1.45; }
a { color: var(--vscode-textLink-foreground); cursor: pointer; }
`;

// Plain DOM, text nodes only (no innerHTML with data).
const SCRIPT = `
const vscode = acquireVsCodeApi();
const root = document.getElementById('root');
const send = (cmd, extra) => vscode.postMessage({ cmd, ...extra });

function el(tag, attrs, ...children) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (k === 'onclick') n.addEventListener('click', v);
    else if (k === 'class') n.className = v;
    else n.setAttribute(k, v);
  }
  for (const c of children.flat(Infinity)) if (c !== null && c !== undefined && c !== false) n.append(c instanceof Node ? c : String(c));
  return n;
}
const btn = (label, cmd, cls) => el('button', { class: cls || '', onclick: () => send(cmd) }, label);
const kv = (pairs) => el('dl', {}, pairs.filter(Boolean).map(([k, v]) => [el('dt', {}, k), el('dd', { title: String(v) }, v)]));

function ago(iso) {
  if (!iso) return 'never';
  const s = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000));
  if (s < 10) return 'just now';
  if (s < 60) return s + ' s ago';
  if (s < 3600) return Math.round(s / 60) + ' min ago';
  if (s < 86400) return Math.round(s / 3600) + ' h ago';
  return new Date(iso).toLocaleDateString();
}

function headline(st) {
  if (!st.agentFound && !st.live) return ['red', 'Agent not installed', 'Install the CodeSecretary agent, or set its path in the settings.'];
  if (!st.live) return ['gray', 'Agent stopped', 'Nothing is collected or uploaded until it runs.'];
  const s = st.status;
  if (s.config && s.config.paused) return ['amber', 'Tracking paused', 'Inside a pause window: nothing is collected.'];
  const c = s.connection ? s.connection.state : 'unknown';
  if (c === 'signed_out' || !s.account) return ['amber', 'Signed out', 'Collecting locally. Sign in to upload.'];
  if (c === 'offline') return ['red', 'Server unreachable', (s.connection.last_error || 'retrying') + '. Events wait on this device.'];
  if (c === 'online') return ['green', 'Connected', s.account.email || s.account.server_url];
  return ['amber', 'Connecting...', s.account.server_url];
}

function render(st) {
  const s = st.status;
  const [tone, title, sub] = headline(st);
  const signedIn = st.live && s && s.account && s.connection.state !== 'signed_out';
  const cards = [];

  cards.push(el('div', { class: 'card' },
    el('div', { class: 'hero' }, el('span', { class: 'dot ' + tone + (tone === 'green' ? ' pulse' : '') }),
      el('div', {}, el('div', { class: 'title' }, title), el('div', { class: 'sub' }, sub))),
    el('div', { class: 'row' },
      !st.agentFound && !st.live ? btn('Set agent path', 'settings', 'primary grow') : null,
      st.agentFound && !st.live ? btn('Start agent', 'start', 'primary grow') : null,
      st.live && !signedIn && st.login.state !== 'waiting' ? btn('Sign in', 'signIn', 'primary grow') : null,
      st.dashboardUrl || signedIn ? btn('Open dashboard', 'openDashboard', signedIn ? 'primary grow' : 'grow') : null)));

  if (st.login.state === 'waiting') {
    cards.push(el('div', { class: 'card' }, el('h3', {}, 'Sign in'),
      st.login.userCode
        ? [el('div', { class: 'muted' }, 'Approve this device in the browser window that opened. Check that it shows this code:'),
           el('div', { class: 'code' }, st.login.userCode),
           el('div', { class: 'row' }, el('button', { class: 'primary grow', onclick: () => send('openLink', { url: st.login.url }) }, 'Open sign-in page'),
             btn('Cancel', 'cancelSignIn'))]
        : [el('div', { class: 'muted' }, 'Contacting the server...'), el('div', { class: 'row' }, btn('Cancel', 'cancelSignIn'))]));
  } else if (st.login.state === 'error') {
    cards.push(el('div', { class: 'card' }, el('h3', {}, 'Sign in'), el('div', { class: 'error' }, st.login.message),
      el('div', { class: 'row' }, btn('Try again', 'signIn', 'primary'))));
  }

  if (st.live && s) {
    cards.push(el('div', { class: 'card' }, el('h3', {}, 'Account'),
      kv([
        ['Signed in', s.account && s.account.email ? s.account.email : (signedIn ? 'yes' : 'no')],
        s.account ? ['Server', s.account.server_url] : null,
        s.account && s.account.device_id ? ['Device', '#' + s.account.device_id] : null,
        ['Last contact', ago(s.connection.last_ok_at)],
      ]),
      el('div', { class: 'row' }, signedIn ? btn('Sign out', 'signOut') : null, signedIn ? null : btn('Sign in', 'signIn'))));

    cards.push(el('div', { class: 'card' }, el('h3', {}, 'Agent'),
      kv([
        ['Version', s.version || '-'],
        ['Running since', ago(s.started_at)],
        ['You are', s.presence && s.presence.state ? s.presence.state : 'unknown'],
        ['Waiting to upload', s.upload.queued],
        ['Last upload', ago(s.upload.last_upload_at)],
        ['Uploaded since start', s.upload.uploaded_since_start],
        ['Git repositories', s.git.repositories],
        ['Rules', s.config.path_exclusions + s.config.app_exclusions + s.config.domain_exclusions + ' exclusions, ' + s.config.pause_windows + ' pauses'],
      ]),
      el('div', { class: 'row' }, btn('Stop agent', 'stop'), btn('Refresh', 'refresh'))));
  }

  const e = st.editor;
  const link = e.sender.link === 'connected' ? 'connected' : e.sender.link === 'unreachable' ? 'not reachable' : 'connecting';
  cards.push(el('div', { class: 'card' }, el('h3', {}, 'This editor'),
    el('div', { class: 'toggle' }, el('div', {}, el('div', {}, 'Track VS Code activity'),
      el('div', { class: 'sub' }, e.enabled ? (e.project ? 'Project: ' + e.project : 'No project file open') : 'Off: nothing is sent')),
      el('button', { class: 'switch' + (e.enabled ? ' on' : ''), role: 'switch', 'aria-checked': String(e.enabled), 'aria-label': 'Track VS Code activity', onclick: () => send('toggleTracking') })),
    el('div', { class: 'spacer' }),
    kv([
      ['Link to agent', link],
      ['Sent', e.sender.sent],
      ['Waiting', e.sender.queued],
      e.sender.dropped ? ['Dropped (agent offline)', e.sender.dropped] : null,
    ])));

  root.replaceChildren(...cards,
    el('p', { class: 'note' }, 'Sent: which file has focus, edit counts, saves, debug sessions. Never file contents, keystrokes or the clipboard. ',
      el('a', { onclick: () => send('settings') }, 'Settings')));
}

window.addEventListener('message', (ev) => { if (ev.data && ev.data.type === 'state') render(ev.data.state); });
send('ready');
`;
