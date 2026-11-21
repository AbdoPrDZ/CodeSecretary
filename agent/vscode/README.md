# CodeSecretary for VS Code

Tells the local CodeSecretary agent what you work on in the editor. It sends events to the daemon's local API (AG-12), which filters, masks and uploads them like its own. Without a running agent it does nothing useful: events wait in memory for 5 minutes, then are dropped, and the status bar says so.

## What it sends

| Event | When | Fields |
|---|---|---|
| `editor.workspace_open` | at startup and when a folder is added | folder path, Git remote |
| `editor.file_focus` | interval while a file editor has focus and the window is in use (VS Code's own inactivity signal); cut every 5 min, under 2 s dropped | relative path, language id |
| `editor.edit_burst` | edits to the active file, summed per 30 s | relative path, **number** of changes |
| `editor.save` | on save | relative path |
| `editor.debug_session` | from start to end of a debug session, cut every 5 min | launch configuration name |

Every event carries the repository root and the raw remote URL; the agent removes credentials from the remote and normalizes it (`github.com/acme/api`). Paths are relative to the Git work tree, or to the workspace folder outside Git.

**Never read or sent:** file contents, the text of edits, selections, the clipboard, terminal output. Only `file:` documents count (not untitled, output, settings or remote files), and nothing inside `.git/`.

The status bar shows the current project, "Tracking paused" during a pause window, or "agent offline"; clicking it opens the panel.

## Sidebar panel

The CodeSecretary icon in the activity bar opens a panel that shows:

- **The connection:** connected, signed out, server unreachable, paused, agent stopped, or agent not installed.
- **The account:** e-mail, server, device and last contact.
- **The agent:** version, active or idle, events waiting to upload, last upload, watched Git repositories, and the exclusions and pause windows in use.
- **This editor:** a tracking on/off switch (setting `codesecretary.enabled`), the current project, and the events sent, waiting or dropped.

Its buttons run the same `secretary-agent` commands as the desktop widget:

- **Sign in** asks for the server address and runs `login --json`. The browser opens, and the panel shows the device code to check.
- **Sign out** runs `logout`, after asking you to confirm.
- **Start agent** and **Stop agent** run `run` and `stop`.
- **Open dashboard** opens `codesecretary.dashboardUrl`, and asks for it once if it's empty.

The panel reads `<data>/status.json`, which holds states and counts only, every 2 s while it is visible. It never sees tokens: the agent keeps them in the OS keychain. The agent program comes from `codesecretary.agentPath`, or is looked up on `PATH`. All actions are also commands (**CodeSecretary: Sign In**, and so on).

## Develop

```sh
corepack pnpm install                       # from the repository root
corepack pnpm --filter codesecretary test   # compile + unit tests (node:test)
corepack pnpm --filter codesecretary package  # -> agent/vscode/codesecretary.vsix
code --install-extension agent/vscode/codesecretary.vsix
```

`src/generated/contract.ts` is generated from `shared/` (SH-9); do not edit it.

### Run inside VS Code

`out/test/vscode/index.js` opens, edits and saves `src/main.rs` in the opened folder. Run it in a separate VS Code instance against a test agent:

```sh
export SECRETARY_HOME=/tmp/cs-home        # the extension finds local.json here too
secretary-agent run &
Code --user-data-dir /tmp/cs-ud --disable-extensions \
  --extensionDevelopmentPath=$PWD/agent/vscode \
  --extensionTestsPath=$PWD/agent/vscode/out/test/vscode/index.js /path/to/a/git/repo
```

When starting it from a terminal inside VS Code, unset the `VSCODE_*` and `ELECTRON_RUN_AS_NODE` variables first. Otherwise the command is routed to the running editor.

Remote workspaces (SSH, WSL, containers) are not reported yet: the extension runs on the local side (`extensionKind: ui`) and skips non-`file:` documents.
