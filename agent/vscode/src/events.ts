// Builds shared-envelope events (see src/generated/contract.ts). Only names,
// paths, counts and times: never file contents.
import type { Event, Payload } from './generated/contract';
import { uuidv7 } from './ids';
import type { FileRef, Repo } from './repo';

export const COLLECTOR = 'vscode';

export function source(version: string): string {
  return `${COLLECTOR}@${version}`;
}

function iso(ms: number): string {
  return new Date(ms).toISOString();
}

export function makeEvent(
  src: string,
  payload: Payload,
  start: number,
  end: number | null,
  repo: Repo | null,
): Event {
  return {
    event_id: uuidv7(start),
    schema_version: 1,
    timestamp_start: iso(start),
    timestamp_end: end === null ? null : iso(end),
    source: src,
    project_hint: null,
    // Outside Git the root is the workspace folder: file paths stay relative
    // to it, and folder rules still match.
    repository: repo ? { root_path: repo.root, remote_url: repo.remote } : null,
    working_directory: null,
    ...payload,
  } as Event;
}

export const fileFocus = (src: string, f: FileRef, language: string | null, start: number, end: number) =>
  makeEvent(src, { event_type: 'editor.file_focus', metadata: { file_path: f.relPath, language } }, start, end, f.repo);

export const editBurst = (src: string, f: FileRef, count: number, start: number, end: number) =>
  makeEvent(src, { event_type: 'editor.edit_burst', metadata: { file_path: f.relPath, edit_count: count } },
    start, end, f.repo);

export const save = (src: string, f: FileRef, at: number) =>
  makeEvent(src, { event_type: 'editor.save', metadata: { file_path: f.relPath } }, at, null, f.repo);

export const workspaceOpen = (src: string, folder: string, repo: Repo | null, at: number) =>
  makeEvent(src, { event_type: 'editor.workspace_open', metadata: { workspace_path: folder, remote_url: repo?.remote ?? null } },
    at, null, repo);

export const debugSession = (src: string, name: string | null, repo: Repo | null, start: number, end: number) =>
  makeEvent(src, { event_type: 'editor.debug_session', metadata: { configuration_name: name } }, start, end, repo);
