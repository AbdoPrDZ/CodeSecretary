import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type ReactNode } from 'react';

import { api, type Exclusion, type ExclusionKind } from '../api';
import { YourData } from '../components/YourData';
import { IconAppWindow, IconClock, IconFolder, IconGlobe, IconPause, IconShield, IconTrash } from '../components/icons';
import { Button, Card, cx, EmptyState, ErrorText, IconButton, Input, Notice, PageHeader, Segmented,
  Skeleton } from '../components/ui';

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
type PatternKind = Exclude<ExclusionKind, 'time_window'>;
const KINDS: { kind: PatternKind; label: string; icon: ReactNode; placeholder: string; help: string }[] = [
  { kind: 'path', label: 'Folder', icon: <IconFolder size={14} />, placeholder: '~/clients/acme or **/private',
    help: 'The folder and everything below it; * and ** work.' },
  { kind: 'app', label: 'App', icon: <IconAppWindow size={14} />, placeholder: 'Slack', help: 'Process name, with or without .exe.' },
  { kind: 'domain', label: 'Website', icon: <IconGlobe size={14} />, placeholder: 'bank.example', help: 'The domain and its subdomains.' },
];

function useList() {
  const queryClient = useQueryClient();
  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['exclusions'] });
  const add = useMutation({ mutationFn: (body: Partial<Exclusion>) => api.addExclusion(body), onSuccess: invalidate });
  const remove = useMutation({ mutationFn: (id: number) => api.deleteExclusion(id), onSuccess: invalidate });
  return { add, remove };
}

export function ExclusionsPage() {
  const exclusions = useQuery({ queryKey: ['exclusions'], queryFn: api.exclusions });
  const all = exclusions.data ?? [];
  const patterns = all.filter((e) => e.kind !== 'time_window');
  const windows = all.filter((e) => e.kind === 'time_window');

  return (
    <>
      <PageHeader eyebrow="Privacy" title="Exclusions & pauses"
                  description="Agents never collect what is excluded here, and stay quiet during pause windows. Changes reach every agent within 5 minutes." />
      <div className="mb-6">
        <Notice>
          Never collected at all: file contents, keystrokes, clipboard and screenshots. Passwords, tokens and keys are
          masked on the device before anything is sent.
        </Notice>
      </div>
      {exclusions.isError && <ErrorText error={exclusions.error} />}
      <div className="grid gap-6 lg:grid-cols-2">
        <Card title="Never track" description="Folders, apps and websites the agents ignore">
          <AddPattern />
          {exclusions.isPending ? <Skeleton className="mt-5 h-24" /> : patterns.length === 0 ? (
            <EmptyState icon={<IconShield />} title="Nothing excluded">Everything the agents see is collected.</EmptyState>
          ) : (
            <ul className="mt-5 space-y-2">
              {patterns.map((e) => {
                const kind = KINDS.find((k) => k.kind === e.kind);
                return (
                  <Row key={e.id} id={e.id} icon={kind?.icon}>
                    <div className="min-w-0">
                      <div className="text-[11px] font-semibold tracking-wider text-zinc-400 uppercase">{kind?.label}</div>
                      <code className="block truncate text-zinc-800 dark:text-zinc-200" title={e.pattern ?? ''}>{e.pattern}</code>
                    </div>
                  </Row>
                );
              })}
            </ul>
          )}
        </Card>
        <Card title="Pause windows" description="Recurring times nothing is collected, e.g. lunch">
          <AddWindow />
          {exclusions.isPending ? <Skeleton className="mt-5 h-24" /> : windows.length === 0 ? (
            <EmptyState icon={<IconPause />} title="No pause windows">Add one for lunch or evenings.</EmptyState>
          ) : (
            <ul className="mt-5 space-y-2">
              {windows.map((e) => (
                <Row key={e.id} id={e.id} icon={<IconClock size={14} />}>
                  <div>
                    <div className="font-semibold tabular-nums">{e.start} - {e.end}</div>
                    <div className="text-xs text-zinc-500">
                      {e.days.length === 0 ? 'Every day' : e.days.map((d) => DAYS[d - 1]).join(', ')}
                    </div>
                  </div>
                </Row>
              ))}
            </ul>
          )}
        </Card>
      </div>
      <div className="mt-6">
        <YourData />
      </div>
    </>
  );
}

function Row({ id, icon, children }: { id: number; icon: ReactNode; children: ReactNode }) {
  const { remove } = useList();
  return (
    <li className="group flex items-center gap-3 rounded-xl bg-zinc-50 px-3 py-2.5 text-sm ring-1 ring-zinc-200/60 dark:bg-zinc-950/40 dark:ring-zinc-800">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-white text-zinc-500 ring-1 ring-zinc-200 dark:bg-zinc-900 dark:ring-zinc-700">
        {icon}
      </span>
      <div className="min-w-0 flex-1">{children}</div>
      <IconButton label="Remove" disabled={remove.isPending} onClick={() => remove.mutate(id)}
                  className="hover:!bg-red-50 hover:!text-red-600 dark:hover:!bg-red-500/10">
        <IconTrash size={15} />
      </IconButton>
    </li>
  );
}

function AddPattern() {
  const [kind, setKind] = useState<PatternKind>('path');
  const [pattern, setPattern] = useState('');
  const { add } = useList();
  const meta = KINDS.find((k) => k.kind === kind)!;
  return (
    <form className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (pattern.trim()) add.mutate({ kind, pattern: pattern.trim() }, { onSuccess: () => setPattern('') });
          }}>
      <Segmented<PatternKind> value={kind} onChange={setKind}
                              options={KINDS.map((k) => ({ value: k.kind, label: <>{k.icon}{k.label}</> }))} />
      <div className="flex gap-2">
        <Input value={pattern} maxLength={500} placeholder={meta.placeholder} onChange={(e) => setPattern(e.target.value)} />
        <Button type="submit" variant="primary" disabled={add.isPending || !pattern.trim()}>Exclude</Button>
      </div>
      <p className="text-xs text-zinc-500">{meta.help}</p>
      <ErrorText error={add.error} />
    </form>
  );
}

function AddWindow() {
  const [start, setStart] = useState('12:00');
  const [end, setEnd] = useState('13:00');
  const [days, setDays] = useState<number[]>([]);
  const { add } = useList();
  const toggle = (d: number) => setDays((cur) => (cur.includes(d) ? cur.filter((x) => x !== d) : [...cur, d].sort()));
  return (
    <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); add.mutate({ kind: 'time_window', start, end, days }); }}>
      <div className="flex flex-wrap items-center gap-2">
        <Input type="time" className="w-32" value={start} onChange={(e) => setStart(e.target.value)} required />
        <span className="text-sm text-zinc-400">to</span>
        <Input type="time" className="w-32" value={end} onChange={(e) => setEnd(e.target.value)} required />
        <Button type="submit" variant="primary" className="ml-auto" disabled={add.isPending}>Add pause</Button>
      </div>
      <div className="flex flex-wrap items-center gap-1">
        {DAYS.map((label, i) => (
          <button key={label} type="button" onClick={() => toggle(i + 1)} aria-pressed={days.includes(i + 1)}
                  className={cx('h-8 w-11 rounded-lg text-xs font-medium ring-1 ring-inset transition',
                    days.includes(i + 1)
                      ? 'bg-brand-500 text-white ring-brand-500'
                      : 'text-zinc-600 ring-zinc-200 hover:bg-zinc-50 dark:text-zinc-400 dark:ring-zinc-700 dark:hover:bg-zinc-800')}>
            {label}
          </button>
        ))}
        <span className="ml-2 text-xs text-zinc-500">{days.length === 0 ? 'every day' : ''}</span>
      </div>
      <ErrorText error={add.error} />
    </form>
  );
}
