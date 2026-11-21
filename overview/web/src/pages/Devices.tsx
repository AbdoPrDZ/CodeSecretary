import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { api, type Device } from '../api';
import { IconLaptop, IconMonitor, IconPencil } from '../components/icons';
import { Badge, Button, Card, Dialog, EmptyState, ErrorText, IconButton, Input, PageHeader, Skeleton,
  StatusDot } from '../components/ui';
import { relative } from '../format';

const ONLINE_MS = 3 * 60_000;
const OS_LABEL: Record<string, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux', android: 'Android' };

export function DevicesPage() {
  const devices = useQuery({ queryKey: ['devices'], queryFn: api.devices, refetchInterval: 30_000 });
  const list = devices.data ?? [];
  const online = list.filter(isOnline).length;

  return (
    <>
      <PageHeader eyebrow="Agents" title="Devices"
                  description="Computers signed in with the CodeSecretary agent. Revoking a device stops its uploads within a minute."
                  actions={list.length > 0 && (
                    <Badge tone={online ? 'green' : 'neutral'} className="h-7 px-2.5 text-xs">
                      <StatusDot tone={online ? 'green' : 'gray'} pulse={online > 0} /> {online} of {list.length} online
                    </Badge>
                  )} />
      {devices.isPending && (
        <div className="grid gap-4 md:grid-cols-2">{[0, 1].map((i) => <Skeleton key={i} className="h-48 rounded-2xl" />)}</div>
      )}
      {devices.isError && <ErrorText error={devices.error} />}
      {devices.data?.length === 0 && (
        <Card>
          <EmptyState icon={<IconLaptop />} title="No devices yet">
            Install the agent and run <code className="rounded bg-zinc-100 px-1.5 py-0.5 dark:bg-zinc-800">codesecretary</code>,
            then sign in from its widget.
          </EmptyState>
        </Card>
      )}
      <div className="grid gap-4 md:grid-cols-2">
        {list.map((d) => <DeviceCard key={d.id} device={d} />)}
      </div>
    </>
  );
}

function isOnline(d: Device) {
  return d.revoked_at === null && d.last_seen_at !== null && Date.now() - Date.parse(d.last_seen_at) < ONLINE_MS;
}

function DeviceCard({ device }: { device: Device }) {
  const queryClient = useQueryClient();
  const [renaming, setRenaming] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['devices'] });
  const revoke = useMutation({ mutationFn: () => api.revokeDevice(device.id), onSuccess: () => { setConfirming(false); void refresh(); } });
  const revoked = device.revoked_at !== null;
  const online = isOnline(device);

  return (
    <Card className={revoked ? 'opacity-60' : ''}>
      <div className="flex items-start gap-4">
        <div className={`flex size-12 shrink-0 items-center justify-center rounded-2xl ring-1 ${
          online ? 'bg-emerald-50 text-emerald-600 ring-emerald-200 dark:bg-emerald-500/10 dark:text-emerald-300 dark:ring-emerald-500/20'
            : 'bg-zinc-100 text-zinc-500 ring-zinc-200 dark:bg-zinc-800 dark:ring-zinc-700'}`}>
          <IconMonitor size={22} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1">
            <h3 className="truncate text-base font-semibold">{device.name}</h3>
            {!revoked && <IconButton label="Rename" onClick={() => setRenaming(true)}><IconPencil size={14} /></IconButton>}
          </div>
          <div className="mt-0.5 flex flex-wrap items-center gap-1.5">
            {revoked ? <Badge tone="red">Revoked</Badge> : online ? (
              <Badge tone="green"><StatusDot tone="green" pulse /> Online</Badge>
            ) : <Badge><StatusDot tone="gray" /> Offline</Badge>}
            <Badge>{OS_LABEL[device.os] ?? device.os}</Badge>
            {device.agent_version && <Badge tone="brand">v{device.agent_version}</Badge>}
          </div>
        </div>
      </div>
      <dl className="mt-5 grid grid-cols-3 gap-3 rounded-xl bg-zinc-50 p-3 text-sm dark:bg-zinc-950/40">
        <div>
          <dt className="text-xs text-zinc-500">Last seen</dt>
          <dd className="mt-0.5 font-medium">{relative(device.last_seen_at)}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500">Queued</dt>
          <dd className="mt-0.5 font-medium tabular-nums">{device.queue_depth ?? '-'}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500">Clock offset</dt>
          <dd className="mt-0.5 font-medium tabular-nums">{Math.round(device.clock_offset_ms / 1000)} s</dd>
        </div>
      </dl>
      {!revoked && (
        <div className="mt-4 flex justify-end">
          <Button size="sm" variant="danger" onClick={() => setConfirming(true)}>Revoke access</Button>
        </div>
      )}
      {renaming && <RenameDialog device={device} onClose={() => setRenaming(false)} />}
      {confirming && (
        <Dialog title={`Revoke "${device.name}"?`} onClose={() => setConfirming(false)}
                description="Its agent stops uploading and must sign in again. Collected activity stays."
                footer={<>
                  <Button onClick={() => setConfirming(false)}>Cancel</Button>
                  <Button variant="danger" disabled={revoke.isPending} onClick={() => revoke.mutate()}>Revoke</Button>
                </>}>
          <ErrorText error={revoke.error} />
        </Dialog>
      )}
    </Card>
  );
}

function RenameDialog({ device, onClose }: { device: Device; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [name, setName] = useState(device.name);
  const rename = useMutation({
    mutationFn: () => api.renameDevice(device.id, name.trim()),
    onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ['devices'] }); onClose(); },
  });
  return (
    <Dialog title="Rename device" onClose={onClose}
            footer={<>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" type="submit" form="rename-device" disabled={rename.isPending || !name.trim()}>Save</Button>
            </>}>
      <form id="rename-device" onSubmit={(e) => { e.preventDefault(); if (name.trim()) rename.mutate(); }} className="space-y-2">
        <Input value={name} maxLength={255} autoFocus onChange={(e) => setName(e.target.value)} />
        <ErrorText error={rename.error} />
      </form>
    </Dialog>
  );
}
