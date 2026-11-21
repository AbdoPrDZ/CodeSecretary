import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { api, type Device } from '../api';
import { buttonClass, Card, inputClass, primaryButtonClass, relative } from '../components/Layout';

export function DevicesPage() {
  const devices = useQuery({ queryKey: ['devices'], queryFn: api.devices, refetchInterval: 30_000 });

  return (
    <>
      <h1 className="mb-4 text-xl font-semibold">Devices</h1>
      <Card title="Signed-in agents">
        {devices.isPending && <p className="text-sm text-slate-500">Loading…</p>}
        {devices.isError && <p className="text-sm text-red-600">Could not load devices.</p>}
        {devices.data?.length === 0 && (
          <p className="text-sm text-slate-500">
            No devices yet. Run <code className="rounded bg-slate-100 px-1 dark:bg-slate-800">secretary-agent login</code> on a computer.
          </p>
        )}
        {devices.data && devices.data.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="text-xs text-slate-500 uppercase">
                <tr>
                  <th className="py-2 pr-3 font-medium">Name</th>
                  <th className="py-2 pr-3 font-medium">OS</th>
                  <th className="py-2 pr-3 font-medium">Agent</th>
                  <th className="py-2 pr-3 font-medium">Last seen</th>
                  <th className="py-2 pr-3 font-medium">Queued</th>
                  <th className="py-2 pr-3 font-medium">Status</th>
                  <th className="py-2 font-medium" />
                </tr>
              </thead>
              <tbody>
                {devices.data.map((d) => <DeviceRow key={d.id} device={d} />)}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </>
  );
}

function DeviceRow({ device }: { device: Device }) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(device.name);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['devices'] });
  const rename = useMutation({
    mutationFn: () => api.renameDevice(device.id, name.trim()),
    onSuccess: () => { setEditing(false); void refresh(); },
  });
  const revoke = useMutation({ mutationFn: () => api.revokeDevice(device.id), onSuccess: refresh });
  const revoked = device.revoked_at !== null;
  const online = !revoked && device.last_seen_at !== null && Date.now() - Date.parse(device.last_seen_at) < 3 * 60_000;

  return (
    <tr className={`border-t border-slate-100 dark:border-slate-800 ${revoked ? 'opacity-60' : ''}`}>
      <td className="py-2 pr-3">
        {editing ? (
          <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); if (name.trim()) rename.mutate(); }}>
            <input className={inputClass} value={name} maxLength={255} autoFocus onChange={(e) => setName(e.target.value)} />
            <button type="submit" className={primaryButtonClass} disabled={rename.isPending}>Save</button>
            <button type="button" className={buttonClass} onClick={() => { setEditing(false); setName(device.name); }}>Cancel</button>
          </form>
        ) : (
          <button type="button" className="hover:underline" title="Rename" onClick={() => setEditing(true)} disabled={revoked}>
            {device.name}
          </button>
        )}
      </td>
      <td className="py-2 pr-3">{device.os}</td>
      <td className="py-2 pr-3">{device.agent_version ?? '—'}</td>
      <td className="py-2 pr-3">{relative(device.last_seen_at)}</td>
      <td className="py-2 pr-3 tabular-nums">{device.queue_depth ?? '—'}</td>
      <td className="py-2 pr-3">
        {revoked ? (
          <span className="text-red-600">Revoked</span>
        ) : (
          <span className="flex items-center gap-1.5">
            <span className={`inline-block size-2 rounded-full ${online ? 'bg-emerald-500' : 'bg-slate-400'}`} />
            {online ? 'Online' : 'Offline'}
          </span>
        )}
      </td>
      <td className="py-2 text-right">
        {!revoked && (
          <button type="button" className={`${buttonClass} text-red-600`} disabled={revoke.isPending}
                  onClick={() => {
                    if (window.confirm(`Revoke "${device.name}"? Its agent stops uploading and must sign in again.`)) revoke.mutate();
                  }}>
            Revoke
          </button>
        )}
      </td>
    </tr>
  );
}
