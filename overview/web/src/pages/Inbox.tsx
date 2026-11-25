// Inbox (OV-10): unassigned time, grouped by the folder or app it was spent
// in. Assign a group's sessions once, or add a rule so the engine assigns it
// from now on (and recalculates the last 30 days).
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';

import { api, type InboxGroup } from '../api';
import { ProjectSelect, useProjects } from '../components/corrections';
import { IconAppWindow, IconCheck, IconFolder, IconLayers } from '../components/icons';
import { Button, Card, EmptyState, ErrorText, Notice, PageHeader, Segmented, Skeleton, Stat } from '../components/ui';
import { clock, duration, plural } from '../format';
import { isoDay, shiftDay } from '../timeline';

type Range = '7' | '14' | '30';

export function InboxPage() {
  const [range, setRange] = useState<Range>('14');
  const today = isoDay(new Date());
  const start = shiftDay(today, -(Number(range) - 1));
  const inbox = useQuery({
    queryKey: ['inbox', start, today],
    queryFn: () => api.inbox(start, today),
    refetchInterval: (q) => ((q.state.data?.pending_days ?? 0) > 0 ? 5_000 : false),
  });
  const data = inbox.data;
  return (
    <>
      <PageHeader eyebrow="Inbox" title="Unassigned time"
                  description="Time no rule matched, grouped by the folder or app it was spent in. Assign it once, or add a rule so it is assigned automatically from now on."
                  actions={<Segmented<Range> value={range} onChange={setRange}
                                             options={[{ value: '7', label: '7 days' }, { value: '14', label: '14 days' }, { value: '30', label: '30 days' }]} />} />
      <div className="mb-6 grid grid-cols-2 gap-4 lg:grid-cols-3">
        {inbox.isPending ? [0, 1, 2].map((i) => <Skeleton key={i} className="h-[104px] rounded-2xl" />) : (
          <>
            <Stat icon={<IconLayers size={14} />} label="Unassigned" value={duration(data?.total_seconds ?? 0)} hint={`last ${range} days`} />
            <Stat icon={<IconFolder size={14} />} label="Groups" value={data?.groups.length ?? 0} hint="folders and apps" />
            <Stat icon={<IconCheck size={14} />} label="Sessions" value={data?.sessions ?? 0} hint={data?.truncated ? 'first 1,000' : 'to review'} />
          </>
        )}
      </div>
      {(data?.pending_days ?? 0) > 0 && <div className="mb-4"><Notice>Recalculating {plural(data!.pending_days, 'day')}...</Notice></div>}
      {inbox.isError && <Notice tone="error">Could not load the inbox.</Notice>}
      {data && data.groups.length === 0 && (
        <Card><EmptyState icon={<IconCheck />} title="Nothing to review">All calculated time in this range belongs to a project.</EmptyState></Card>
      )}
      <div className="space-y-4">
        {data?.groups.map((g) => <GroupCard key={`${g.signal}:${g.value}`} group={g} />)}
      </div>
    </>
  );
}

function GroupCard({ group }: { group: InboxGroup }) {
  const projects = useProjects();
  const client = useQueryClient();
  const [project, setProject] = useState('');
  const [open, setOpen] = useState(false);
  const refresh = () => {
    for (const key of ['inbox', 'time', 'sessions', 'projects']) void client.invalidateQueries({ queryKey: [key] });
  };
  const assign = useMutation({
    mutationFn: () => api.assign({ session_ids: group.sessions.map((s) => s.id), project_id: Number(project) }),
    onSuccess: refresh,
  });
  const rule = useMutation({
    mutationFn: () => api.addRule(Number(project), { signal: group.signal === 'path' ? 'path' : 'app', pattern: group.value }),
    onSuccess: refresh,
  });
  const canRule = group.signal !== 'other';
  return (
    <Card>
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <span className="text-zinc-400">{group.signal === 'path' ? <IconFolder size={16} /> : <IconAppWindow size={16} />}</span>
            <span className="truncate" title={group.value}>{group.value}</span>
          </div>
          <div className="mt-1 text-xs text-zinc-500">
            <b className="text-zinc-900 tabular-nums dark:text-zinc-100">{duration(group.seconds)}</b> in {plural(group.sessions.length, 'session')}
            {group.titles.length > 0 && <> · {group.titles.slice(0, 3).join(' · ')}</>}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div className="w-52"><ProjectSelect value={project} onChange={setProject} projects={projects} /></div>
          <Button disabled={!project || assign.isPending} onClick={() => assign.mutate()}>Assign these</Button>
          {canRule && (
            <Button variant="primary" disabled={!project || rule.isPending} onClick={() => rule.mutate()}
                    title={`Add a ${group.signal === 'path' ? 'folder' : 'app'} rule: ${group.value}`}>
              Always (add rule)
            </Button>
          )}
          <Button variant="ghost" size="sm" onClick={() => setOpen((v) => !v)}>{open ? 'Hide' : 'Sessions'}</Button>
        </div>
      </div>
      {projects.length === 0 && (
        <p className="mt-3 text-xs text-zinc-500"><Link to="/projects" className="font-medium text-brand-600 hover:underline">Create a project</Link> first.</p>
      )}
      <ErrorText error={assign.error ?? rule.error} />
      {open && (
        <ul className="mt-4 grid gap-1.5 text-xs text-zinc-600 sm:grid-cols-2 lg:grid-cols-3 dark:text-zinc-400">
          {group.sessions.map((s) => (
            <li key={s.id} className="tabular-nums">{s.day} · {clock(s.start)} - {clock(s.end)} · {duration(s.seconds)}</li>
          ))}
        </ul>
      )}
    </Card>
  );
}
