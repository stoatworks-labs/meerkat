import type { Camera, DriverInfo, Method, Reach } from '../types';

export function label(c: Camera): string {
  return c.name || c.model || `${c.vendor} camera`;
}

export function title(c: Camera): string {
  return [c.vendor, c.model].filter(Boolean).join(' ');
}

const METHOD: Record<Method, string> = {
  broadcast: 'broadcast',
  multicast: 'multicast',
  mdns: 'mDNS',
  unicast: 'unicast probe',
};

/** "Easy IP Setup (broadcast on en0)", one per way the camera was heard. */
export function sightings(c: Camera, drivers: DriverInfo[]): string[] {
  const out: string[] = [];
  for (const s of c.seen) {
    const d = drivers.find((x) => x.id === s.driver);
    const what = d ? d.protocol : s.driver;
    const where = s.method === 'unicast' ? `from ${s.via}` : `on ${s.via}`;
    const line = `${what} — ${METHOD[s.method]} ${where}`;
    if (!out.includes(line)) out.push(line);
  }
  return out;
}

export function foundBy(c: Camera, drivers: DriverInfo[]): string {
  const names: string[] = [];
  for (const s of c.seen) {
    const d = drivers.find((x) => x.id === s.driver);
    const n = d ? (d.id === 'onvif' ? 'ONVIF' : d.id === 'ndi' ? 'NDI' : d.vendor) : s.driver;
    if (!names.includes(n)) names.push(n);
  }
  return names.join(' + ');
}

export function ReachBadge({ reach }: { reach: Reach }) {
  switch (reach.kind) {
    case 'local':
      return <span className="badge ok" title={`On ${reach.iface}'s subnet: reachable directly`}>{reach.iface}</span>;
    case 'routed':
      return <span className="badge info" title="Answered through a router">routed</span>;
    case 'mismatch':
      return (
        <span className="badge warn" title={`Heard on ${reach.iface}, but its address is outside ${reach.iface}'s subnet`}>
          wrong subnet
        </span>
      );
    default:
      return <span className="badge">?</span>;
  }
}

/** One sentence on where the camera sits and what that means for changing it. */
export function reachNote(c: Camera): string | null {
  const r = c.reach;
  if (r.kind === 'mismatch') {
    if (c.caps.writeNetwork && c.caps.writeByMac)
      return `${c.ip} is outside ${r.iface}'s subnet. Its discovery protocol works by MAC address at layer 2, so it can still be moved onto this computer's subnet from here.`;
    return `${c.ip} is outside ${r.iface}'s subnet, so its web UI and API cannot be reached until this computer has an address on its subnet too.`;
  }
  if (r.kind === 'routed') {
    if (c.caps.writeByMac) return 'Reached through a router. Its re-addressing protocol is layer 2 only, so change it from its web UI or from a computer on its own network.';
    return 'Reached through a router.';
  }
  return null;
}

export function Spinner() {
  return <span className="spinner" aria-hidden="true" />;
}
