import { useEffect, useMemo, useRef, useState } from 'react';

import { ipKey, nth, parsePrefix, problems } from '../lib/net';
import { useStore } from '../store';
import type { Camera } from '../types';
import { label, Spinner, title } from './ui';

/**
 * Address several cameras in one go: a start address, counted up in the
 * order shown, sent one camera at a time (each waits for the last to come
 * back, so two never fight over an address mid-change).
 */
export function BulkDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { cameras, checked, jobs, apply } = useStore();
  const ref = useRef<HTMLDialogElement>(null);
  const [start, setStart] = useState('');
  const [mask, setMask] = useState('24');
  const [gateway, setGateway] = useState('');
  const [running, setRunning] = useState(false);

  const list: Camera[] = useMemo(
    () =>
      checked
        .map((id) => cameras[id])
        .filter(Boolean)
        .sort((a, b) => ipKey(a.ip) - ipKey(b.ip)),
    [checked, cameras],
  );
  const writable = list.filter((c) => c.caps.writeNetwork && !c.caps.needsAuth && !(c.reach.kind === 'mismatch' && !c.caps.writeByMac));

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) {
      d.showModal();
      // Start where the first camera is, in its subnet.
      const first = writable[0];
      if (first && !start) {
        setMask(String(first.net?.prefix ?? 24));
        setStart(first.ip);
      }
    } else if (!open && d.open) d.close();
  }, [open]); // the camera list is read once, as the dialog opens

  const prefix = parsePrefix(mask);
  const plan = writable.map((c, i) => ({ cam: c, ip: prefix === null ? null : nth(start, i, prefix) }));
  const errs = prefix === null || !start ? ['give a start address and a mask'] : problems(start, prefix, gateway);
  const overflow = plan.some((p) => p.ip === null);
  const clash = plan.find((p) => p.ip && Object.values(cameras).some((o) => o.ip === p.ip && !writable.some((w) => w.id === o.id)));

  const run = async () => {
    setRunning(true);
    for (const p of plan) {
      if (!p.ip) break;
      if (p.cam.ip === p.ip && p.cam.net?.prefix === prefix && (p.cam.net?.gateway ?? '') === gateway.trim()) continue;
      const ok = await apply(p.cam.id, { dhcp: false, ip: p.ip, prefix: prefix!, gateway: gateway.trim() || null, dns: p.cam.net?.dns ?? [], hostname: null }, null);
      if (!ok) break;
    }
    setRunning(false);
  };

  return (
    <dialog ref={ref} className="bulk" onClose={onClose} onCancel={(e) => running && e.preventDefault()}>
      <h2>Address {writable.length} camera{writable.length === 1 ? '' : 's'} in sequence</h2>
      {writable.length < list.length && (
        <p className="muted small">
          {list.length - writable.length} of the selected cannot be changed from here (web UI only, needs a login, or out of reach) and are left out.
        </p>
      )}
      <div className="grid three">
        <label>
          First address
          <input className="mono" value={start} onChange={(e) => setStart(e.target.value)} disabled={running} spellCheck={false} />
        </label>
        <label>
          Mask
          <input className="mono" value={mask} onChange={(e) => setMask(e.target.value)} disabled={running} spellCheck={false} />
        </label>
        <label>
          Gateway
          <input className="mono" value={gateway} onChange={(e) => setGateway(e.target.value)} disabled={running} placeholder="none" spellCheck={false} />
        </label>
      </div>
      <table className="plan">
        <thead>
          <tr>
            <th>Camera</th>
            <th>Now</th>
            <th>New</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {plan.map((p) => {
            const job = jobs[p.cam.id];
            return (
              <tr key={p.cam.id}>
                <td>
                  {label(p.cam)} <span className="muted small">{title(p.cam)}</span>
                </td>
                <td className="mono">{p.cam.ip}</td>
                <td className="mono">{p.ip ?? <span className="bad">off the subnet</span>}</td>
                <td className="small">
                  {job && (job.state === 'sending' || job.state === 'waiting') && <Spinner />}
                  {job?.state === 'done' && <span className="ok-mark">✓</span>}
                  {job?.state === 'failed' && <span className="bad" title={job.text}>failed</span>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {errs.length > 0 && <p className="errs">{errs[0]}</p>}
      {clash && <p className="errs">{clash.ip} is already used by a camera that is not in this batch.</p>}
      <div className="row end">
        <button type="button" onClick={onClose} disabled={running}>
          Close
        </button>
        <button type="button" className="sw-primary" disabled={running || errs.length > 0 || overflow || !!clash || writable.length === 0} onClick={() => void run()}>
          {running ? 'Working…' : `Change ${writable.length}`}
        </button>
      </div>
    </dialog>
  );
}
