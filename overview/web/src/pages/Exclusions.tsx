import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { api, ApiError, type Exclusion, type ExclusionKind } from '../api';
import { buttonClass, Card, inputClass, primaryButtonClass } from '../components/Layout';

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const KINDS: { kind: Exclude<ExclusionKind, 'time_window'>; label: string; placeholder: string; help: string }[] = [
  { kind: 'path', label: 'Folder', placeholder: '~/clients/acme or **/private', help: 'The folder and everything below it; * and ** work.' },
  { kind: 'app', label: 'App', placeholder: 'Slack', help: 'Process name, with or without .exe.' },
  { kind: 'domain', label: 'Website', placeholder: 'bank.example', help: 'The domain and its subdomains.' },
];

export function ExclusionsPage() {
  const exclusions = useQuery({ queryKey: ['exclusions'], queryFn: api.exclusions });
  const all = exclusions.data ?? [];

  return (
    <>
      <h1 className="mb-1 text-xl font-semibold">Exclusions & pause windows</h1>
      <p className="mb-4 text-sm text-slate-500">
        Agents never collect what is excluded here. Changes reach every agent within 5 minutes.
      </p>
      {exclusions.isError && <p className="mb-4 text-sm text-red-600">Could not load exclusions.</p>}
      <Card title="Never track">
        <AddPattern />
        <List items={all.filter((e) => e.kind !== 'time_window')}
              render={(e) => (
                <>
                  <span className="w-20 shrink-0 text-xs text-slate-500 uppercase">
                    {KINDS.find((k) => k.kind === e.kind)?.label}
                  </span>
                  <code className="break-all">{e.pattern}</code>
                </>
              )} />
      </Card>
      <Card title="Pause windows">
        <AddWindow />
        <List items={all.filter((e) => e.kind === 'time_window')}
              render={(e) => (
                <>
                  <span className="tabular-nums">{e.start}–{e.end}</span>
                  <span className="text-slate-500">
                    {e.days.length === 0 ? 'every day' : e.days.map((d) => DAYS[d - 1]).join(', ')}
                  </span>
                </>
              )} />
      </Card>
    </>
  );
}

function useAdd() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: Partial<Exclusion>) => api.addExclusion(body),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['exclusions'] }),
  });
}

function AddPattern() {
  const [kind, setKind] = useState<(typeof KINDS)[number]['kind']>('path');
  const [pattern, setPattern] = useState('');
  const add = useAdd();
  const meta = KINDS.find((k) => k.kind === kind)!;
  return (
    <form className="mb-4 flex flex-wrap items-start gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (pattern.trim()) add.mutate({ kind, pattern: pattern.trim() }, { onSuccess: () => setPattern('') });
          }}>
      <select className={inputClass} value={kind} onChange={(e) => setKind(e.target.value as typeof kind)}>
        {KINDS.map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
      </select>
      <div className="min-w-64 flex-1">
        <input className={`${inputClass} w-full`} value={pattern} maxLength={500} placeholder={meta.placeholder}
               onChange={(e) => setPattern(e.target.value)} />
        <p className="mt-1 text-xs text-slate-500">{meta.help}</p>
      </div>
      <button type="submit" className={primaryButtonClass} disabled={add.isPending || !pattern.trim()}>Add</button>
      <ErrorText error={add.error} />
    </form>
  );
}

function AddWindow() {
  const [start, setStart] = useState('12:00');
  const [end, setEnd] = useState('13:00');
  const [days, setDays] = useState<number[]>([]);
  const add = useAdd();
  const toggle = (d: number) => setDays((cur) => (cur.includes(d) ? cur.filter((x) => x !== d) : [...cur, d]));
  return (
    <form className="mb-4 flex flex-wrap items-center gap-2"
          onSubmit={(e) => { e.preventDefault(); add.mutate({ kind: 'time_window', start, end, days }); }}>
      <input type="time" className={inputClass} value={start} onChange={(e) => setStart(e.target.value)} required />
      <span className="text-slate-500">to</span>
      <input type="time" className={inputClass} value={end} onChange={(e) => setEnd(e.target.value)} required />
      <div className="flex flex-wrap gap-1">
        {DAYS.map((label, i) => (
          <button key={label} type="button" onClick={() => toggle(i + 1)}
                  className={`rounded-md border px-2 py-1 text-xs ${
                    days.includes(i + 1)
                      ? 'border-slate-900 bg-slate-900 text-white dark:border-white dark:bg-white dark:text-slate-900'
                      : 'border-slate-300 dark:border-slate-700'
                  }`}>
            {label}
          </button>
        ))}
      </div>
      <span className="text-xs text-slate-500">{days.length === 0 ? 'every day' : ''}</span>
      <button type="submit" className={primaryButtonClass} disabled={add.isPending}>Add</button>
      <ErrorText error={add.error} />
    </form>
  );
}

function List({ items, render }: { items: Exclusion[]; render: (e: Exclusion) => React.ReactNode }) {
  const queryClient = useQueryClient();
  const remove = useMutation({
    mutationFn: (id: number) => api.deleteExclusion(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['exclusions'] }),
  });
  if (items.length === 0) return <p className="text-sm text-slate-500">Nothing yet.</p>;
  return (
    <ul className="divide-y divide-slate-100 dark:divide-slate-800">
      {items.map((e) => (
        <li key={e.id} className="flex items-center gap-3 py-2 text-sm">
          {render(e)}
          <button type="button" className={`${buttonClass} ml-auto`} disabled={remove.isPending}
                  onClick={() => remove.mutate(e.id)}>
            Remove
          </button>
        </li>
      ))}
    </ul>
  );
}

function ErrorText({ error }: { error: Error | null }) {
  if (!error) return null;
  const code = error instanceof ApiError ? error.code : 'error';
  const text: Record<string, string> = {
    invalid_window: 'Start and end must be different times.',
    invalid_pattern: 'Enter a pattern.',
  };
  return <p className="w-full text-sm text-red-600">{text[code] ?? `Could not save (${code}).`}</p>;
}
