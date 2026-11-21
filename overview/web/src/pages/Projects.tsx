// Projects and mapping rules (OV-5, D17): the user creates projects and binds
// evidence to them with rules; each rule shows what it would match before it
// is saved. Detected repositories are only suggestions.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState, type ReactNode } from 'react';

import { api, type Project, type ProjectFields, type RuleSignal } from '../api';
import { IconAppWindow, IconArchive, IconChevronDown, IconCoins, IconFolder, IconGitBranch, IconGlobe,
  IconPencil, IconPlus, IconSparkles, IconTrash, IconType, IconUndo, IconX } from '../components/icons';
import { Badge, Button, Card, cx, Dialog, EmptyState, ErrorText, Field, IconButton, Input, Notice, PageHeader,
  Segmented, Skeleton, Switch } from '../components/ui';
import { plural, relative } from '../format';

const SIGNALS: { signal: RuleSignal; label: string; icon: ReactNode; placeholder: string; help: string }[] = [
  { signal: 'path', label: 'Folder', icon: <IconFolder size={14} />, placeholder: 'C:\\work\\acme or ~/code/*/client-x',
    help: 'The folder and everything below it. * matches within a folder name, ** across folders.' },
  { signal: 'remote', label: 'Repository', icon: <IconGitBranch size={14} />, placeholder: 'github.com/acme or git@github.com:acme/api.git',
    help: 'A repository or a whole owner. Clone URLs are accepted.' },
  { signal: 'domain', label: 'Website', icon: <IconGlobe size={14} />, placeholder: 'jira.acme.com',
    help: 'The domain and its subdomains. Needs the browser extension (Phase 2); use a title rule until then.' },
  { signal: 'app', label: 'App', icon: <IconAppWindow size={14} />, placeholder: 'figma', help: 'Process name, with or without .exe.' },
  { signal: 'title', label: 'Title', icon: <IconType size={14} />, placeholder: 'Acme Portal',
    help: 'Text in the window title. With * or ? it must match the whole title.' },
];
const signalMeta = (s: RuleSignal) => SIGNALS.find((x) => x.signal === s)!;

const COLORS = ['#6366f1', '#8b5cf6', '#d946ef', '#ec4899', '#f43f5e', '#f97316', '#eab308', '#22c55e', '#14b8a6', '#06b6d4', '#3b82f6', '#64748b'];
const colorOf = (p: Pick<Project, 'id' | 'color'>) => p.color ?? COLORS[p.id % COLORS.length]!;

function useInvalidate() {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: ['projects'] });
    void queryClient.invalidateQueries({ queryKey: ['suggestions'] });
    void queryClient.invalidateQueries({ queryKey: ['preview'] });
  };
}

export function ProjectsPage() {
  const projects = useQuery({ queryKey: ['projects'], queryFn: api.projects });
  const [creating, setCreating] = useState(false);
  const [showArchived, setShowArchived] = useState(false);
  const all = projects.data ?? [];
  const active = all.filter((p) => !p.archived_at);
  const archived = all.filter((p) => p.archived_at);
  const ruleCount = active.reduce((n, p) => n + p.rules.length, 0);

  return (
    <>
      <PageHeader eyebrow="Projects" title="Projects & rules"
                  description="Create your projects and add rules that bind activity to them. Activity no rule matches goes to the review inbox. Changing rules recalculates the last 30 days."
                  actions={<Button variant="primary" icon={<IconPlus size={16} />} onClick={() => setCreating(true)}>New project</Button>} />

      {projects.isError && <ErrorText error={projects.error} />}
      <Suggestions />

      {projects.isPending && (
        <div className="grid gap-5 lg:grid-cols-2">{[0, 1].map((i) => <Skeleton key={i} className="h-64 rounded-2xl" />)}</div>
      )}
      {projects.isSuccess && active.length === 0 && (
        <Card>
          <EmptyState icon={<IconFolder />} title="No projects yet"
                      action={<Button variant="primary" icon={<IconPlus size={16} />} onClick={() => setCreating(true)}>Create your first project</Button>}>
            A project is a client, a product or anything you want time for. Rules decide which activity belongs to it.
          </EmptyState>
        </Card>
      )}
      {active.length > 0 && (
        <p className="mb-4 text-sm text-zinc-500">{plural(active.length, 'project')} · {plural(ruleCount, 'rule')}</p>
      )}
      <div className="grid gap-5 lg:grid-cols-2">
        {active.map((p) => <ProjectCard key={p.id} project={p} />)}
      </div>

      {archived.length > 0 && (
        <div className="mt-10">
          <button type="button" onClick={() => setShowArchived((v) => !v)}
                  className="mb-3 flex items-center gap-1.5 text-sm font-medium text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100">
            <IconChevronDown size={16} className={cx('transition', !showArchived && '-rotate-90')} />
            Archived ({archived.length})
          </button>
          {showArchived && (
            <Card bodyClassName="p-0">
              <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
                {archived.map((p) => <ArchivedRow key={p.id} project={p} />)}
              </ul>
            </Card>
          )}
        </div>
      )}

      {creating && <ProjectDialog onClose={() => setCreating(false)} />}
    </>
  );
}

function Suggestions() {
  const suggestions = useQuery({ queryKey: ['suggestions'], queryFn: api.suggestions });
  const invalidate = useInvalidate();
  const create = useMutation({
    mutationFn: async (s: { name: string; remote: string }) => {
      const project = await api.addProject({ name: s.name });
      await api.addRule(project.id, { signal: 'remote', pattern: s.remote });
    },
    onSuccess: invalidate,
  });
  const repos = suggestions.data ?? [];
  if (repos.length === 0) return null;
  return (
    <Card className="mb-8 bg-gradient-to-br from-brand-50/80 to-white dark:from-brand-500/[0.07] dark:to-zinc-900/70"
          title={<span className="flex items-center gap-2"><IconSparkles size={16} className="text-brand-500" />Detected repositories</span>}
          description="Recent repositories no rule covers. Optional: one click creates a project with a repository rule.">
      <div className="grid gap-2 sm:grid-cols-2">
        {repos.map((r) => (
          <div key={r.remote} className="flex items-center gap-3 rounded-xl bg-white/80 px-3 py-2.5 ring-1 ring-zinc-200/70 dark:bg-zinc-950/40 dark:ring-zinc-800">
            <IconGitBranch size={16} className="shrink-0 text-zinc-400" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium" title={r.remote}>{r.remote}</div>
              <div className="text-xs text-zinc-500">{plural(r.events, 'event')} · {relative(r.last_seen)}</div>
            </div>
            <Button size="sm" variant="subtle" icon={<IconPlus size={14} />} disabled={create.isPending}
                    onClick={() => create.mutate({ name: r.name, remote: r.remote })}>
              Create
            </Button>
          </div>
        ))}
      </div>
      <div className="mt-2"><ErrorText error={create.error} /></div>
    </Card>
  );
}

function ProjectCard({ project }: { project: Project }) {
  const [editing, setEditing] = useState(false);
  const [addingRule, setAddingRule] = useState(false);
  const invalidate = useInvalidate();
  const archive = useMutation({ mutationFn: () => api.editProject(project.id, { archived: true }), onSuccess: invalidate });
  const removeRule = useMutation({ mutationFn: (id: number) => api.deleteRule(id), onSuccess: invalidate });
  const color = colorOf(project);

  return (
    <Card accent={color} bodyClassName="p-0">
      <div className="flex items-start gap-3 px-5 pt-5">
        <div className="flex size-10 shrink-0 items-center justify-center rounded-xl text-sm font-bold text-white shadow-sm"
             style={{ background: `linear-gradient(135deg, ${color}, color-mix(in oklab, ${color} 70%, black))` }}>
          {project.name.slice(0, 1).toUpperCase()}
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-base font-semibold">{project.name}</h3>
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            {project.client_name && <Badge>{project.client_name}</Badge>}
            {project.hourly_rate ? (
              <Badge tone="green"><IconCoins size={12} />{project.hourly_rate} {project.currency ?? ''}/h</Badge>
            ) : <Badge>No rate</Badge>}
            {!project.billable && <Badge tone="amber">Not billable</Badge>}
          </div>
        </div>
        <div className="flex">
          <IconButton label="Edit project" onClick={() => setEditing(true)}><IconPencil size={15} /></IconButton>
          <IconButton label="Archive project" disabled={archive.isPending} onClick={() => archive.mutate()}><IconArchive size={15} /></IconButton>
        </div>
      </div>

      <div className="px-5 pt-5 pb-5">
        <div className="mb-2 flex items-center justify-between">
          <span className="text-[11px] font-semibold tracking-wider text-zinc-400 uppercase">Rules</span>
          <Button size="sm" variant="ghost" icon={<IconPlus size={14} />} onClick={() => setAddingRule(true)}>Add rule</Button>
        </div>
        {project.rules.length === 0 ? (
          <button type="button" onClick={() => setAddingRule(true)}
                  className="w-full rounded-xl border border-dashed border-zinc-300 px-4 py-5 text-sm text-zinc-500 transition hover:border-brand-400 hover:bg-brand-50/40 hover:text-brand-700 dark:border-zinc-700 dark:hover:border-brand-500/50 dark:hover:bg-brand-500/5 dark:hover:text-brand-300">
            No rules yet, so nothing is assigned. Add a folder, repository, app or title.
          </button>
        ) : (
          <ul className="space-y-1.5">
            {project.rules.map((r) => {
              const meta = signalMeta(r.signal);
              return (
                <li key={r.id} className="group flex items-center gap-2.5 rounded-lg bg-zinc-50 px-2.5 py-1.5 text-sm ring-1 ring-zinc-200/60 dark:bg-zinc-950/40 dark:ring-zinc-800">
                  <span className="flex items-center gap-1.5 text-xs font-medium text-zinc-500" title={meta.label}>
                    {meta.icon}<span className="w-[4.5rem]">{meta.label}</span>
                  </span>
                  <code className="min-w-0 flex-1 truncate text-zinc-800 dark:text-zinc-200" title={r.pattern}>{r.pattern}</code>
                  {r.priority !== 0 && <Badge tone="brand">P{r.priority}</Badge>}
                  <button type="button" aria-label="Remove rule" disabled={removeRule.isPending} onClick={() => removeRule.mutate(r.id)}
                          className="rounded-md p-1 text-zinc-400 opacity-0 transition group-hover:opacity-100 hover:bg-red-50 hover:text-red-600 focus:opacity-100 dark:hover:bg-red-500/10">
                    <IconX size={14} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {editing && <ProjectDialog project={project} onClose={() => setEditing(false)} />}
      {addingRule && <RuleDialog project={project} onClose={() => setAddingRule(false)} />}
    </Card>
  );
}

function ProjectDialog({ project, onClose }: { project?: Project; onClose: () => void }) {
  const [name, setName] = useState(project?.name ?? '');
  const [client, setClient] = useState(project?.client_name ?? '');
  const [rate, setRate] = useState(project?.hourly_rate ?? '');
  const [currency, setCurrency] = useState(project?.currency ?? '');
  const [billable, setBillable] = useState(project?.billable ?? true);
  const [color, setColor] = useState(project ? colorOf(project) : COLORS[0]!);
  const invalidate = useInvalidate();
  const save = useMutation({
    mutationFn: (body: ProjectFields) => (project ? api.editProject(project.id, body) : api.addProject(body)),
    onSuccess: () => { invalidate(); onClose(); },
  });
  const submit = () => {
    if (!name.trim()) return;
    save.mutate({
      name: name.trim(), client_name: client.trim() || null, hourly_rate: rate ? Number(rate) : null,
      currency: currency.trim() || null, billable, color,
    });
  };
  return (
    <Dialog title={project ? 'Edit project' : 'New project'} onClose={onClose}
            description={project ? undefined : 'Add rules after creating it, so activity lands in the project.'}
            footer={<>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" type="submit" form="project-form" disabled={save.isPending || !name.trim()}>
                {project ? 'Save' : 'Create project'}
              </Button>
            </>}>
      <form id="project-form" className="space-y-4" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <Field label="Name">{(id) => <Input id={id} value={name} maxLength={200} autoFocus placeholder="Acme website" onChange={(e) => setName(e.target.value)} />}</Field>
        <Field label="Client (optional)">{(id) => <Input id={id} value={client} maxLength={200} placeholder="Acme Inc." onChange={(e) => setClient(e.target.value)} />}</Field>
        <div className="grid grid-cols-[1fr_6rem] gap-3">
          <Field label="Hourly rate" help="Cost = calculated time x rate.">
            {(id) => <Input id={id} type="number" min="0" step="0.01" value={rate} placeholder="0.00" onChange={(e) => setRate(e.target.value)} />}
          </Field>
          <Field label="Currency">{(id) => <Input id={id} value={currency} maxLength={3} placeholder="EUR" className="uppercase" onChange={(e) => setCurrency(e.target.value)} />}</Field>
        </div>
        <div>
          <div className="mb-1.5 text-xs font-medium text-zinc-600 dark:text-zinc-400">Color</div>
          <div className="flex flex-wrap gap-2">
            {COLORS.map((c) => (
              <button key={c} type="button" aria-label={`Color ${c}`} aria-pressed={c === color} onClick={() => setColor(c)}
                      className={cx('size-7 rounded-full ring-offset-2 transition hover:scale-110 dark:ring-offset-zinc-900',
                        c === color && 'ring-2 ring-zinc-900 dark:ring-white')}
                      style={{ background: c }} />
            ))}
          </div>
        </div>
        <Switch checked={billable} onChange={setBillable} label="Billable" />
        <ErrorText error={save.error} />
      </form>
    </Dialog>
  );
}

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(id);
  }, [value, ms]);
  return debounced;
}

function RuleDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const [signal, setSignal] = useState<RuleSignal>('path');
  const [pattern, setPattern] = useState('');
  const [priority, setPriority] = useState(0);
  const debounced = useDebounced(pattern.trim(), 350);
  const invalidate = useInvalidate();
  const preview = useQuery({
    queryKey: ['preview', project.id, signal, debounced],
    queryFn: () => api.previewRule({ signal, pattern: debounced, project_id: project.id }),
    enabled: debounced.length > 0,
    retry: false,
  });
  const add = useMutation({
    mutationFn: () => api.addRule(project.id, { signal, pattern: pattern.trim(), priority }),
    onSuccess: () => { invalidate(); onClose(); },
  });
  const meta = signalMeta(signal);
  const p = preview.data && debounced ? preview.data : null;

  return (
    <Dialog title={`Add a rule to ${project.name}`} onClose={onClose}
            description="Activity that matches the rule is assigned to this project."
            footer={<>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" type="submit" form="rule-form" disabled={add.isPending || !pattern.trim()}>Add rule</Button>
            </>}>
      <form id="rule-form" className="space-y-4" onSubmit={(e) => { e.preventDefault(); if (pattern.trim()) add.mutate(); }}>
        <div className="overflow-x-auto">
          <Segmented<RuleSignal> value={signal} onChange={setSignal}
                                 options={SIGNALS.map((s) => ({ value: s.signal, label: <>{s.icon}{s.label}</> }))} />
        </div>
        <Field label="Pattern" help={meta.help}>
          {(id) => <Input id={id} value={pattern} maxLength={500} autoFocus placeholder={meta.placeholder} className="font-mono"
                          onChange={(e) => setPattern(e.target.value)} />}
        </Field>
        <Field label="Priority" help="When rules of different projects match the same activity, the higher priority wins, then the longer pattern.">
          {(id) => <Input id={id} type="number" min={-100} max={100} value={priority} className="w-24"
                          onChange={(e) => setPriority(Math.max(-100, Math.min(100, Number(e.target.value) || 0)))} />}
        </Field>

        <div className="rounded-xl bg-zinc-50 p-4 ring-1 ring-zinc-200/70 dark:bg-zinc-950/50 dark:ring-zinc-800" aria-live="polite">
          <div className="mb-2 text-[11px] font-semibold tracking-wider text-zinc-400 uppercase">Preview · last 30 days</div>
          {!debounced && <p className="text-sm text-zinc-500">Type a pattern to see what it matches.</p>}
          {debounced && preview.isFetching && !p && <Skeleton className="h-14" />}
          {debounced && preview.isError && <ErrorText error={preview.error} />}
          {p && (
            <div className={cx('space-y-3 transition', preview.isFetching && 'opacity-60')}>
              <div className="flex items-baseline gap-2">
                <span className={cx('text-3xl font-semibold tracking-tight tabular-nums', p.matched_events ? 'text-brand-600 dark:text-brand-300' : 'text-zinc-400')}>
                  {p.matched_events.toLocaleString()}
                </span>
                <span className="text-sm text-zinc-500">
                  matching event{p.matched_events === 1 ? '' : 's'}{p.truncated ? ' (most frequent values only)' : ''}
                </span>
              </div>
              {p.pattern !== pattern.trim() && (
                <p className="text-xs text-zinc-500">Saved as <code className="text-zinc-700 dark:text-zinc-300">{p.pattern}</code></p>
              )}
              {p.examples.length > 0 && (
                <ul className="scroll-thin max-h-40 space-y-1 overflow-y-auto">
                  {p.examples.map((e) => (
                    <li key={e.value} className="flex items-center gap-2 rounded-md bg-white px-2 py-1 text-xs ring-1 ring-zinc-200/60 dark:bg-zinc-900 dark:ring-zinc-800">
                      <code className="min-w-0 flex-1 truncate" title={e.value}>{e.value}</code>
                      <span className="shrink-0 text-zinc-500 tabular-nums">{e.events}x · {relative(e.last_seen)}</span>
                    </li>
                  ))}
                  {p.distinct_values > p.examples.length && (
                    <li className="px-2 text-xs text-zinc-500">and {p.distinct_values - p.examples.length} more values</li>
                  )}
                </ul>
              )}
              {p.matched_events === 0 && <p className="text-xs text-zinc-500">Nothing recent matches. The rule still applies to future activity.</p>}
              {p.conflicts.map((c) => (
                <Notice key={c.project_id} tone="warn">
                  {plural(c.events, 'event')} also match a rule of <strong>{c.name}</strong>.
                </Notice>
              ))}
            </div>
          )}
        </div>
        <ErrorText error={add.error} />
      </form>
    </Dialog>
  );
}

function ArchivedRow({ project }: { project: Project }) {
  const invalidate = useInvalidate();
  const [confirming, setConfirming] = useState(false);
  const restore = useMutation({ mutationFn: () => api.editProject(project.id, { archived: false }), onSuccess: invalidate });
  const remove = useMutation({ mutationFn: () => api.deleteProject(project.id), onSuccess: invalidate });
  return (
    <li className="flex items-center gap-3 px-5 py-3 text-sm">
      <span className="size-2.5 shrink-0 rounded-full" style={{ background: colorOf(project) }} />
      <span className="font-medium">{project.name}</span>
      <span className="text-xs text-zinc-500">{plural(project.rules.length, 'rule')}, inactive while archived</span>
      <div className="ml-auto flex gap-1.5">
        <Button size="sm" icon={<IconUndo size={14} />} disabled={restore.isPending} onClick={() => restore.mutate()}>Restore</Button>
        <Button size="sm" variant="danger" icon={<IconTrash size={14} />} onClick={() => setConfirming(true)}>Delete</Button>
      </div>
      {confirming && (
        <Dialog title={`Delete "${project.name}"?`} onClose={() => setConfirming(false)}
                footer={<>
                  <Button onClick={() => setConfirming(false)}>Cancel</Button>
                  <Button variant="danger" disabled={remove.isPending} onClick={() => remove.mutate()}>Delete project</Button>
                </>}>
          <Notice tone="warn">
            Its rules and calculated time are removed. Raw activity stays and goes back to the review inbox.
          </Notice>
        </Dialog>
      )}
    </li>
  );
}
