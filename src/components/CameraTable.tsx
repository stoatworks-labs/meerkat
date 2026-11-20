import { useMemo } from 'react';

import { ipKey } from '../lib/net';
import { useStore } from '../store';
import type { Camera } from '../types';
import { foundBy, ReachBadge, Spinner } from './ui';

function matches(c: Camera, q: string): boolean {
  if (!q) return true;
  const hay = [c.vendor, c.model, c.name, c.ip, c.mac, c.firmware, c.serial].filter(Boolean).join(' ').toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .every((w) => hay.includes(w));
}

export function useVisibleCameras(): Camera[] {
  const { cameras, filter } = useStore();
  return useMemo(
    () =>
      Object.values(cameras)
        .filter((c) => matches(c, filter))
        .sort((a, b) => ipKey(a.ip) - ipKey(b.ip) || a.vendor.localeCompare(b.vendor)),
    [cameras, filter],
  );
}

export function CameraTable() {
  const { drivers, selected, select, checked, toggleCheck, setChecked, jobs, scanning, cameras, info } = useStore();
  const rows = useVisibleCameras();
  const total = Object.keys(cameras).length;
  const allChecked = rows.length > 0 && rows.every((c) => checked.includes(c.id));

  if (total === 0) {
    return (
      <div className="empty">
        {scanning ? (
          <p>
            <Spinner /> Listening for cameras…
          </p>
        ) : (
          <>
            <h2>No cameras yet</h2>
            <p>
              <b>Scan</b> broadcasts every brand's discovery out of each interface, so cameras on the wrong subnet answer too. Add a routed
              subnet in the sidebar for cameras behind a router.
            </p>
            {!info?.demo && <p className="muted">No cameras to hand? Tick “Demo cameras” to try it against the built-in simulator.</p>}
          </>
        )}
      </div>
    );
  }

  return (
    <div className="table-wrap">
      <table className="cams">
        <thead>
          <tr>
            <th className="check">
              <input
                type="checkbox"
                aria-label="Select all"
                checked={allChecked}
                onChange={() => setChecked(allChecked ? [] : rows.map((c) => c.id))}
              />
            </th>
            <th>Camera</th>
            <th>Name</th>
            <th>Address</th>
            <th>Reach</th>
            <th>MAC</th>
            <th className="opt">Firmware</th>
            <th className="opt">Found by</th>
            <th className="state" aria-label="Status" />
          </tr>
        </thead>
        <tbody>
          {rows.map((c) => {
            const job = jobs[c.id];
            return (
              <tr key={c.id} className={c.id === selected ? 'sel' : undefined} onClick={() => select(c.id)} aria-selected={c.id === selected}>
                <td className="check" onClick={(e) => e.stopPropagation()}>
                  <input type="checkbox" aria-label={`Select ${c.name ?? c.ip}`} checked={checked.includes(c.id)} onChange={() => toggleCheck(c.id)} />
                </td>
                <td>
                  <div className="vendor">{c.vendor}</div>
                  <div className="muted small">{c.model ?? '—'}</div>
                </td>
                <td>{c.name ?? <span className="muted">—</span>}</td>
                <td className="mono">
                  {c.net?.dhcp && <span className="tag">DHCP</span>}
                  {c.ip}
                  {c.net && <span className="muted">/{c.net.prefix}</span>}
                </td>
                <td>
                  <ReachBadge reach={c.reach} />
                </td>
                <td className="mono small">{c.mac ?? <span className="muted">—</span>}</td>
                <td className="small opt">{c.firmware ?? <span className="muted">—</span>}</td>
                <td className="small opt">
                  {foundBy(c, drivers)}
                  {!c.caps.writeNetwork && <span className="muted"> · web UI</span>}
                </td>
                <td className="state">
                  {job && (job.state === 'sending' || job.state === 'waiting') && <Spinner />}
                  {job?.state === 'done' && <span className="ok-mark" title={job.text}>✓</span>}
                  {job?.state === 'failed' && <span className="bad-mark" title={job.text}>!</span>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {rows.length === 0 && <p className="muted pad">Nothing matches the filter.</p>}
    </div>
  );
}
