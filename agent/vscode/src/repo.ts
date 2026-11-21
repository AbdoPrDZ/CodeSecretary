// Repository context for a file: the enclosing Git work tree (or, outside
// Git, the workspace folder), its remote, and the file's path relative to it.
// Only `.git` metadata is read, never the user's files.
import { existsSync, readFileSync, statSync } from 'node:fs';
import * as path from 'node:path';

export interface Repo {
  /** Absolute root, as the OS spells it (drive letter upper-cased). */
  root: string;
  /** Raw remote URL; the agent masks credentials and normalizes it. */
  remote: string | null;
  /** True when `root` is a Git work tree, false for a plain folder. */
  git: boolean;
}

export interface FileRef {
  repo: Repo;
  /** Relative to `repo.root`, with `/` separators. */
  relPath: string;
}

/** `c:\x` -> `C:\x`, as Windows tools (and the agent's Git tracker) spell it. */
export function canonical(p: string): string {
  return /^[a-z]:/.test(p) ? p[0]!.toUpperCase() + p.slice(1) : p;
}

/** First `[remote "origin"]` url, else the first remote's (as the agent's Git tracker). */
export function remoteFromConfig(config: string): string | null {
  let section: string | null = null;
  let first: string | null = null;
  for (const raw of config.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith('[')) {
      const m = /^\[remote "(.+)"\]$/.exec(line);
      section = m ? m[1]! : null;
      continue;
    }
    if (section === null) continue;
    const eq = line.indexOf('=');
    if (eq < 0 || line.slice(0, eq).trim().toLowerCase() !== 'url') continue;
    const url = line.slice(eq + 1).trim();
    if (section === 'origin') return url;
    first ??= url;
  }
  return first;
}

/** The `.git` directory holding `config` for a work tree (handles worktrees and submodules). */
function gitCommonDir(dotGit: string): string | null {
  try {
    if (statSync(dotGit).isDirectory()) return dotGit;
    const m = /^gitdir:\s*(.+)$/m.exec(readFileSync(dotGit, 'utf8'));
    if (!m) return null;
    const gitDir = path.resolve(path.dirname(dotGit), m[1]!.trim());
    const common = path.join(gitDir, 'commondir');
    return existsSync(common) ? path.resolve(gitDir, readFileSync(common, 'utf8').trim()) : gitDir;
  } catch {
    return null;
  }
}

export class RepoFinder {
  private readonly byDir = new Map<string, Repo | null>();

  /** The Git work tree containing `dir`, if any. Cached per directory. */
  gitRepo(dir: string): Repo | null {
    const key = canonical(path.resolve(dir));
    if (this.byDir.has(key)) return this.byDir.get(key)!;
    let found: Repo | null = null;
    const dotGit = path.join(key, '.git');
    if (existsSync(dotGit)) {
      const common = gitCommonDir(dotGit);
      let remote: string | null = null;
      if (common) {
        try {
          remote = remoteFromConfig(readFileSync(path.join(common, 'config'), 'utf8'));
        } catch {
          remote = null;
        }
      }
      found = { root: key, remote, git: true };
    } else {
      const parent = path.dirname(key);
      found = parent === key ? null : this.gitRepo(parent);
    }
    this.byDir.set(key, found);
    return found;
  }

  /** Context for `file`: its Git work tree, else `folder` (the workspace folder). */
  locate(file: string, folder: string | null): FileRef | null {
    const abs = canonical(path.resolve(file));
    const repo = this.gitRepo(path.dirname(abs)) ??
      (folder ? { root: canonical(path.resolve(folder)), remote: null, git: false } : null);
    if (!repo) return null;
    const rel = path.relative(repo.root, abs);
    if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) return null;
    const relPath = rel.split(path.sep).join('/');
    // Inside `.git` itself (commit message buffers and the like): not work.
    if (relPath === '.git' || relPath.startsWith('.git/')) return null;
    return { repo, relPath };
  }

  /** Forget cached lookups (a folder became a repository, a remote changed). */
  clear(): void {
    this.byDir.clear();
  }
}
