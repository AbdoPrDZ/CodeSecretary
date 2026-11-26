// "Your data" on the Privacy page (OV-12): download every collected event,
// or delete the events of a day, a device, or everything. Deleting removes
// what was derived from those events too; the engine recalculates the days.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { api, type PrivacyScope, PRIVACY_EXPORT_URL } from '../api';
import { longDay, plural } from '../format';
import { isoDay } from '../timeline';
import { IconTrash } from './icons';
import { Button, Card, Dialog, ErrorText, Field, Input, Notice, Segmented, Select } from './ui';

export function YourData() {
  const [scope, setScope] = useState<PrivacyScope>('day');
  const [day, setDay] = useState(isoDay(new Date()));
  const [device, setDevice] = useState('');
  const [confirming, setConfirming] = useState(false);
  const devices = useQuery({ queryKey: ['devices'], queryFn: api.devices });
  const ready = scope === 'all' || (scope === 'day' ? Boolean(day) : Boolean(device));
  const deviceName = devices.data?.find((d) => String(d.id) === device)?.name;

  return (
    <Card title="Your data" description="Everything the agents collected for you, to download or delete">
      <div className="space-y-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="max-w-prose text-sm text-zinc-600 dark:text-zinc-400">
            Download all your raw events as NDJSON: one JSON object per line, with the device, the times and the
            collected (already masked) metadata.
          </p>
          <Button onClick={() => window.location.assign(PRIVACY_EXPORT_URL)}>Download events</Button>
        </div>

        <div className="border-t border-zinc-100 pt-5 dark:border-zinc-800">
          <p className="mb-3 text-sm font-medium">Delete collected data</p>
          <div className="flex flex-wrap items-end gap-3">
            <Segmented<PrivacyScope> value={scope} onChange={setScope}
                                     options={[{ value: 'day', label: 'One day' }, { value: 'device', label: 'One device' },
                                       { value: 'all', label: 'Everything' }]} />
            {scope === 'day' && (
              <Field label="Day" className="w-44">
                {(id) => <Input id={id} type="date" value={day} max={isoDay(new Date())} onChange={(e) => setDay(e.target.value)} />}
              </Field>
            )}
            {scope === 'device' && (
              <Field label="Device" className="w-56">
                {(id) => (
                  <Select id={id} value={device} onChange={(e) => setDevice(e.target.value)}>
                    <option value="">Choose a device</option>
                    {devices.data?.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
                  </Select>
                )}
              </Field>
            )}
            <Button variant="danger" icon={<IconTrash size={14} />} disabled={!ready} onClick={() => setConfirming(true)}>
              Delete...
            </Button>
          </div>
          <p className="mt-3 text-xs text-zinc-500">
            {scope === 'all'
              ? 'Deletes every event, session, assignment and correction. Projects, rules, exclusions and devices stay.'
              : 'Deletes the events and the sessions and assignments built from them. Your own corrections stay. The time of the affected days is recalculated.'}
          </p>
        </div>
      </div>
      {confirming && (
        <ConfirmDelete scope={scope} day={day} deviceId={Number(device)} onClose={() => setConfirming(false)}
                       what={scope === 'day' ? longDay(day) : scope === 'device' ? `the device "${deviceName ?? ''}"` : 'everything'} />
      )}
    </Card>
  );
}

function ConfirmDelete({ scope, day, deviceId, what, onClose }: {
  scope: PrivacyScope; day: string; deviceId: number; what: string; onClose: () => void;
}) {
  const client = useQueryClient();
  const [typed, setTyped] = useState('');
  const remove = useMutation({
    mutationFn: () => api.privacyDelete({ scope, confirm: true,
      ...(scope === 'day' ? { day } : {}), ...(scope === 'device' ? { device_id: deviceId } : {}) }),
    onSuccess: () => void client.invalidateQueries(),
  });
  // Everything is hard to undo: type the word, like a repository delete.
  const armed = scope !== 'all' || typed === 'DELETE';

  return (
    <Dialog title={`Delete ${what}?`} onClose={onClose}
            description="This cannot be undone. Download your events first if you may want them later."
            footer={remove.isSuccess ? <Button onClick={onClose}>Close</Button> : (
              <>
                <Button variant="ghost" onClick={onClose}>Cancel</Button>
                <Button variant="danger" disabled={!armed || remove.isPending} onClick={() => remove.mutate()}>
                  {remove.isPending ? 'Deleting...' : 'Delete'}
                </Button>
              </>
            )}>
      {remove.isSuccess ? (
        <Notice>
          Deleted {plural(remove.data.deleted_events, 'event')}.
          {remove.data.queued_days > 0 && ` ${plural(remove.data.queued_days, 'day')} will be recalculated in a moment.`}
        </Notice>
      ) : (
        <div className="space-y-3">
          {scope === 'all' && (
            <Field label='Type "DELETE" to confirm'>
              {(id) => <Input id={id} value={typed} autoComplete="off" onChange={(e) => setTyped(e.target.value)} />}
            </Field>
          )}
          {remove.isError && <ErrorText error={remove.error} />}
        </div>
      )}
    </Dialog>
  );
}
