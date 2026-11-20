import { useEffect, useRef } from 'react';

import { useStore, WATCH_MS } from '../store';
import { label, ReachBadge, Spinner } from './ui';

/**
 * A vendor's own recovery tool (BirdDog Neuralyzer): what it does, where to
 * get it, then a watch that scans until the camera turns up again.
 *
 * Meerkat cannot do the reset itself: the vendor has not published how its
 * tool talks to the camera.
 */
export function RecoveryDialog() {
  const { drivers, recoveryFor, openRecovery, watch, startWatch, stopWatch, cameras, select } = useStore();
  const ref = useRef<HTMLDialogElement>(null);
  const d = drivers.find((x) => x.id === recoveryFor);
  const r = d?.recovery ?? null;
  const mine = watch && watch.driver === d?.id ? watch : null;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (r && !el.open) el.showModal();
    else if (!r && el.open) el.close();
  }, [r]);

  const found = (mine?.found ?? []).map((id) => cameras[id]).filter(Boolean);
  const minutes = Math.round(WATCH_MS / 60_000);

  return (
    <dialog ref={ref} className="recovery" onClose={() => openRecovery(null)} aria-label={r?.tool}>
      {d && r && (
        <>
          <h2>{r.tool}</h2>
          <p>
            {r.does} For {d.vendor} {r.models} cameras.
          </p>
          <p className="muted small">
            Meerkat cannot do this reset itself: {d.vendor} has not published how {r.tool} talks to the camera. So it sends you to {d.vendor}’s
            download, then watches for the camera to come back.
          </p>
          <ol className="steps">
            {r.steps.map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ol>
          <p className="note">{r.after}</p>

          <div className="row">
            <a className="sw-btn" href={r.url} target="_blank" rel="noreferrer">
              Get {r.tool}
            </a>
            <span className="spacer" />
            {mine?.running ? (
              <button type="button" onClick={() => void stopWatch()}>
                <Spinner /> Stop watching
              </button>
            ) : (
              <button type="button" className="sw-primary" onClick={() => void startWatch(d.id)}>
                {mine ? 'Watch again' : 'Watch for it'}
              </button>
            )}
            <button type="button" onClick={() => openRecovery(null)}>
              Close
            </button>
          </div>

          {mine && (
            <p className="watch muted small" role="status">
              {mine.running
                ? `Scanning every interface${r.fallback ? ` and ${r.fallback}` : ''} for a ${d.vendor} camera that is new or at a new address. Round ${mine.rounds + 1}; it gives up after ${minutes} minutes.`
                : found.length
                  ? `${found.length} ${d.vendor} camera${found.length === 1 ? '' : 's'} turned up:`
                  : `Stopped after ${mine.rounds} round${mine.rounds === 1 ? '' : 's'}; no new ${d.vendor} camera turned up. Is it powered, and on the same switch as this computer?`}
            </p>
          )}
          {found.length > 0 && (
            <ul className="found">
              {found.map((c) => (
                <li key={c.id}>
                  <span>{label(c)}</span>
                  <span className="mono">{c.ip}</span>
                  <ReachBadge reach={c.reach} />
                  <span className="spacer" />
                  <button
                    type="button"
                    onClick={() => {
                      select(c.id);
                      openRecovery(null);
                    }}
                  >
                    Show
                  </button>
                </li>
              ))}
            </ul>
          )}
          {found.some((c) => c.reach.kind === 'mismatch') && (
            <p className="muted small">On another subnet: select it, and use Reach it from here to open its web UI.</p>
          )}
        </>
      )}
    </dialog>
  );
}
