// Session detail and corrections (OV-8, OV-9). Corrections are stored as
// engine inputs; after each one the day is recalculated and the page shows
// the new sessions once the engine is done. Durations shown come from the API.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type ReactNode } from 'react';

import { api, type Adjustment, type EvidenceEvent, type NewAdjustment, type Project, type WorkSession } from '../api';
import { clock, duration, plural } from '../format';
import { atTime, hhmm, projectColor } from '../period';
import { styleOf } from '../timeline';
import { IconPlus, IconUndo } from './icons';
import { Badge, Button, Card, cx, Dialog, Drawer, ErrorText, Field, Input, Select, Skeleton } from './ui';

const METHOD_LABEL: Record<string, string> = {
  user: 'assigned by you',
  rule: 'matched your rule',
  shared_rule: 'matched a team rule',
  temporal: 'between events of this project',
  unassigned: 'no rule matched',
};

function useInvalidate() {
  const client = useQueryClient();
  return () => {
    for (const key of ['time', 'sessions', 'session', 'adjustments', 'inbox']) {
      void client.invalidateQueries({ queryKey: [key] });
    }
  };
}

export function useProjects(): Project[] {
  const projects = useQuery({ queryKey: ['projects'], queryFn: api.projects, staleTime: 60_000 });
  return (projects.data ?? []).filter((p) => !p.archived_at);
}

export function ProjectSelect({ value, onChange, projects, none, id }: {
  value: string; onChange: (v: string) => void; projects: Project[]; none?: string; id?: string;
}) {
  return (
    <Select id={id} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">{none ?? 'Choose a project'}</option>
      {projects.map((p) => <option key={p.id} value={String(p.id)}>{p.name}</option>)}
    </Select>
  );
}

function evidenceText(e: EvidenceEvent): string {
  const m = e.metadata as Record<string, unknown>;
  const parts = [m.title, m.app, m.file_path, m.workspace_path, m.branch, e.repository_root]
    .filter((v) => typeof v === 'string' && v) as string[];
  return parts.join(' · ') || e.event_type;
}

/** One session: why it exists, its evidence, and every correction. */
export function SessionDrawer({ session, day, onClose }: { session: WorkSession; day: string; onClose: () => void }) {
  const detail = useQuery({ queryKey: ['session', session.id], queryFn: () => api.session(session.id) });
  const projects = useProjects();
  const invalidate = useInvalidate();
  const [target, setTarget] = useState('');
  const [assignTo, setAssignTo] = useState('');
  const [from, setFrom] = useState(hhmm(session.start));
  const [to, setTo] = useState(hhmm(session.end));
  const [partTarget, setPartTarget] = useState('remove');
  const done = () => {
    invalidate();
    onClose();
  };
  const adjust = useMutation({ mutationFn: (body: NewAdjustment) => api.addAdjustment(body), onSuccess: done });
  const assign = useMutation({
    mutationFn: (project: number | null) => api.assign({ session_ids: [session.id], project_id: project }),
    onSuccess: done,
  });

  const fromProject = session.project_id;
  const moveWhole = () => adjust.mutate({
    kind: 'reassign', start: session.start, end: session.end, project_id: Number(target), from_project_id: fromProject,
  });
  const changePart = () => {
    const start = atTime(day, from);
    const end = atTime(day, to, start);
    adjust.mutate(partTarget === 'remove'
      ? { kind: 'remove', start, end, from_project_id: fromProject }
      : { kind: 'reassign', start, end, project_id: Number(partTarget), from_project_id: fromProject });
  };
  const removeWhole = () => adjust.mutate({ kind: 'remove', start: session.start, end: session.end, from_project_id: fromProject });
  const busy = adjust.isPending || assign.isPending;

  return (
    <Drawer onClose={onClose} title={
      <span className="flex items-center gap-2">
        <span className="size-2.5 rounded-full" style={{ background: projectColor(session.project_id, session.project_color) }} />
        {session.project_name ?? 'Unassigned'}
        <span className="font-normal text-zinc-500">{clock(session.start)} - {clock(session.end)} · {duration(session.seconds)}</span>
      </span>
    }>
      <div className="space-y-6">
        <Section title="Why this session">
          {detail.isPending && <Skeleton className="h-16" />}
          {detail.data && (
            <ul className="space-y-1.5 text-sm text-zinc-600 dark:text-zinc-400">
              {detail.data.session.manual && <li>Added by you (manual time).</li>}
              {Object.entries(detail.data.methods).map(([method, n]) => (
                <li key={method}><b className="text-zinc-900 dark:text-zinc-100">{plural(n ?? 0, 'event')}</b> {METHOD_LABEL[method] ?? method}</li>
              ))}
              {detail.data.rules.map((r) => (
                <li key={r.id} className="text-xs">Rule: <code className="rounded bg-zinc-100 px-1 py-0.5 dark:bg-zinc-800">{r.signal}</code> {r.pattern}</li>
              ))}
              <li className="text-xs text-zinc-400">Time is clipped to when you were active and gaps up to 10 min within one project are joined.</li>
            </ul>
          )}
        </Section>

        <Section title="Move the whole session">
          <div className="flex gap-2">
            <ProjectSelect value={target} onChange={setTarget} projects={projects.filter((p) => p.id !== session.project_id)} />
            <Button variant="primary" disabled={!target || busy} onClick={moveWhole}>Move</Button>
          </div>
          <p className="mt-1.5 text-xs text-zinc-500">Applies to this time range and survives recalculation.</p>
        </Section>

        <Section title="Change part of it">
          <div className="grid grid-cols-2 gap-2">
            <Field label="From">{(id) => <Input id={id} type="time" value={from} onChange={(e) => setFrom(e.target.value)} />}</Field>
            <Field label="To">{(id) => <Input id={id} type="time" value={to} onChange={(e) => setTo(e.target.value)} />}</Field>
          </div>
          <div className="mt-2 flex gap-2">
            <Select value={partTarget} onChange={(e) => setPartTarget(e.target.value)}>
              <option value="remove">Remove this part</option>
              {projects.filter((p) => p.id !== session.project_id).map((p) => (
                <option key={p.id} value={String(p.id)}>Move to {p.name}</option>
              ))}
            </Select>
            <Button disabled={busy || !from || !to} onClick={changePart}>Apply</Button>
          </div>
        </Section>

        <Section title="Its events">
          <div className="flex flex-wrap gap-2">
            <ProjectSelect value={assignTo} onChange={setAssignTo} projects={projects} none="Assign the events to..." />
            <Button disabled={!assignTo || busy} onClick={() => assign.mutate(Number(assignTo))}>Assign</Button>
            <Button variant="ghost" disabled={busy} onClick={() => assign.mutate(null)}>Not project work</Button>
          </div>
          <p className="mt-1.5 text-xs text-zinc-500">Follows the events, wherever recalculation places them.</p>
          {detail.data && (
            <ul className="mt-3 divide-y divide-zinc-100 rounded-xl ring-1 ring-zinc-200/70 dark:divide-zinc-800 dark:ring-zinc-800">
              {detail.data.events.map((e) => (
                <li key={e.event_id} className="flex items-start gap-2.5 px-3 py-2 text-xs">
                  <span className={cx('mt-1 size-2 shrink-0 rounded-full', styleOf(e.event_type).className)} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-zinc-800 dark:text-zinc-200" title={evidenceText(e)}>{evidenceText(e)}</div>
                    <div className="text-zinc-400 tabular-nums">
                      {styleOf(e.event_type).label} · {clock(e.start)}{e.end && ` - ${clock(e.end)}`}{e.device_name && ` · ${e.device_name}`}
                    </div>
                  </div>
                  {e.method && <Badge tone={e.method === 'user' ? 'brand' : 'neutral'}>{e.method === 'shared_rule' ? 'team rule' : e.method}</Badge>}
                </li>
              ))}
            </ul>
          )}
          {detail.data?.truncated && <p className="mt-2 text-xs text-zinc-500">First {detail.data.events.length} of {detail.data.evidence_total} events.</p>}
        </Section>

        <div className="flex items-center justify-between border-t border-zinc-100 pt-4 dark:border-zinc-800">
          <Button variant="danger" disabled={busy} onClick={removeWhole}>Remove session</Button>
          <ErrorText error={adjust.error ?? assign.error} />
        </div>
      </div>
    </Drawer>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-2 text-xs font-semibold tracking-wider text-zinc-400 uppercase">{title}</h3>
      {children}
    </section>
  );
}

export function AddTimeDialog({ day, onClose }: { day: string; onClose: () => void }) {
  const projects = useProjects();
  const invalidate = useInvalidate();
  const [project, setProject] = useState('');
  const [from, setFrom] = useState('09:00');
  const [to, setTo] = useState('10:00');
  const [note, setNote] = useState('');
  const add = useMutation({
    mutationFn: () => {
      const start = atTime(day, from);
      return api.addAdjustment({ kind: 'add', start, end: atTime(day, to, start), project_id: Number(project), note });
    },
    onSuccess: () => {
      invalidate();
      onClose();
    },
  });
  return (
    <Dialog title="Add time" description="Manual time replaces whatever was calculated in that range." onClose={onClose}
            footer={<>
              <Button variant="ghost" onClick={onClose}>Cancel</Button>
              <Button variant="primary" disabled={!project || add.isPending} onClick={() => add.mutate()}>Add</Button>
            </>}>
      <div className="space-y-4">
        <Field label="Project">{(id) => <ProjectSelect id={id} value={project} onChange={setProject} projects={projects} />}</Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="From">{(id) => <Input id={id} type="time" value={from} onChange={(e) => setFrom(e.target.value)} />}</Field>
          <Field label="To">{(id) => <Input id={id} type="time" value={to} onChange={(e) => setTo(e.target.value)} />}</Field>
        </div>
        <Field label="Note">{(id) => <Input id={id} value={note} maxLength={500} placeholder="Meeting, call, whiteboard..." onChange={(e) => setNote(e.target.value)} />}</Field>
        <ErrorText error={add.error} />
      </div>
    </Dialog>
  );
}

const KIND_LABEL: Record<Adjustment['kind'], string> = { add: 'Added', remove: 'Removed', reassign: 'Moved' };

/** The day's corrections, newest last, each with undo. */
export function CorrectionsCard({ day, onAdd }: { day: string; onAdd: () => void }) {
  const list = useQuery({ queryKey: ['adjustments', day], queryFn: () => api.adjustments(day) });
  const invalidate = useInvalidate();
  const undo = useMutation({ mutationFn: (id: number) => api.undoAdjustment(id), onSuccess: invalidate });
  const items = list.data ?? [];
  return (
    <Card title="Corrections" description="Your changes to this day; they survive recalculation"
          actions={<Button size="sm" icon={<IconPlus size={14} />} onClick={onAdd}>Add time</Button>} bodyClassName="p-0">
      {items.length === 0
        ? <p className="px-5 py-4 text-sm text-zinc-500">No corrections. Click a session to fix it.</p>
        : (
          <ul className="divide-y divide-zinc-100 dark:divide-zinc-800/80">
            {items.map((a) => (
              <li key={a.id} className="flex items-center gap-3 px-5 py-2.5 text-sm">
                <Badge tone={a.kind === 'remove' ? 'red' : a.kind === 'add' ? 'green' : 'brand'}>{KIND_LABEL[a.kind]}</Badge>
                <span className="text-zinc-600 tabular-nums dark:text-zinc-400">{clock(a.starts_at)} - {clock(a.ends_at)}</span>
                <span className="min-w-0 flex-1 truncate">
                  {a.kind === 'remove'
                    ? (a.from_project_name ? `from ${a.from_project_name}` : 'everything')
                    : `${a.from_project_name ? `${a.from_project_name} → ` : ''}${a.project_name ?? ''}`}
                  {a.note && <span className="text-zinc-400"> · {a.note}</span>}
                </span>
                <Button size="sm" variant="ghost" icon={<IconUndo size={14} />} disabled={undo.isPending} onClick={() => undo.mutate(a.id)}>
                  Undo
                </Button>
              </li>
            ))}
          </ul>
        )}
    </Card>
  );
}
