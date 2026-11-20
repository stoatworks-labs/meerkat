import { useEffect, useState } from 'react';

import { inSubnet, network, parseIp } from '../lib/net';
import { useStore } from '../store';
import type { Alias, Camera } from '../types';
import { Spinner } from './ui';

const SYSTEM: Record<string, string> = { macos: 'macOS', windows: 'Windows', linux: 'Linux' };

/**
 * A camera heard on the wrong subnet: offer a temporary second address in its
 * subnet on the interface that heard it, so HTTP reaches it. Its web UI opens
 * then, and drivers that work over HTTP (BirdDog's API, ONVIF) can change it.
 */
export function ReachIt({ cam }: { cam: Camera }) {
  const { planAlias, addAlias, aliases, scanning, info } = useStore();
  const [plan, setPlan] = useState<Alias | null>(null);
  const [why, setWhy] = useState<string | null>(null);
  const [ip, setIp] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // Panasonic and Sony re-address by MAC anyway; for them this is only about
  // the web UI, so it starts folded away.
  const [open, setOpen] = useState(!(cam.caps.writeNetwork && cam.caps.writeByMac));

  useEffect(() => {
    let live = true;
    void planAlias(cam.id).then((r) => {
      if (!live) return;
      if ('alias' in r) {
        setPlan(r.alias);
        setIp(r.alias.ip);
        setWhy(null);
      } else {
        setPlan(null);
        setWhy(r.error);
      }
    });
    return () => {
      live = false;
    };
  }, [cam.id, cam.reach.kind, aliases.length, planAlias]);

  if (!plan) return why ? <p className="muted small">{why}</p> : null;
  if (!open) {
    return (
      <button type="button" className="link" onClick={() => setOpen(true)}>
        Open its web UI from here too: add a temporary address…
      </button>
    );
  }

  const bad =
    parseIp(ip) === null
      ? 'not an IPv4 address'
      : !inSubnet(ip, plan.ip, plan.prefix)
        ? `it must stay inside ${network(plan.ip, plan.prefix)}/${plan.prefix}`
        : ip.trim() === cam.ip
          ? 'that is the camera’s own address'
          : null;
  const system = info ? SYSTEM[info.platform] : undefined;

  return (
    <section className="reach-it" aria-label="Reach it from here">
      <h3>Reach it from here</h3>
      <p className="small">
        Give <span className="mono">{plan.iface}</span> a temporary second address in the camera’s subnet and this computer can talk to it directly:
        its web UI opens{cam.caps.writeByMac ? '' : ', and a driver that works over HTTP can change its settings'}.
      </p>
      <div className="row">
        <label className="addr">
          Address
          <input className="mono" value={ip} onChange={(e) => setIp(e.target.value)} spellCheck={false} aria-invalid={!!bad} />
        </label>
        <span className="mono muted">
          /{plan.prefix} on {plan.iface}
        </span>
      </div>
      {bad && <p className="bad small">The address {bad}.</p>}
      <p className="muted small">
        {info?.demo
          ? 'Demo mode: the address is pretend, and nothing on this computer changes.'
          : `${system ?? 'The system'} asks for an administrator password. Meerkat removes the address when it quits (asking once more), and a restart removes it too. Pick an address nothing else on that network uses.`}
      </p>
      {err && <p className="bad small">{err}</p>}
      <button
        type="button"
        className="sw-primary"
        disabled={!!bad || busy || scanning}
        onClick={async () => {
          setBusy(true);
          setErr(await addAlias({ ...plan, ip: ip.trim() }));
          setBusy(false);
        }}
      >
        {busy ? (
          <>
            <Spinner /> Adding…
          </>
        ) : (
          'Add temporary address'
        )}
      </button>
    </section>
  );
}
