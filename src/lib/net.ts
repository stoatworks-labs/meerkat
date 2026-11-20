// IPv4 helpers for the forms. The engine validates every write again; these
// only keep the form honest while typing.

export function parseIp(s: string): number | null {
  const m = /^\s*(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\s*$/.exec(s);
  if (!m) return null;
  const o = m.slice(1).map(Number);
  if (o.some((x) => x > 255)) return null;
  return ((o[0] << 24) | (o[1] << 16) | (o[2] << 8) | o[3]) >>> 0;
}

export function fmtIp(n: number): string {
  return [n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255].join('.');
}

export function maskOf(prefix: number): number {
  return prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
}

export function prefixToMask(prefix: number): string {
  return fmtIp(maskOf(prefix));
}

/** "24", "/24" or "255.255.255.0" → 24; null if it is neither. */
export function parsePrefix(s: string): number | null {
  const t = s.trim().replace(/^\//, '');
  if (/^\d{1,2}$/.test(t)) {
    const p = Number(t);
    return p >= 1 && p <= 32 ? p : null;
  }
  const m = parseIp(t);
  if (m === null) return null;
  const inv = ~m >>> 0;
  if ((inv & (inv + 1)) !== 0) return null; // not contiguous
  let p = 0;
  for (let b = 31; b >= 0 && (m >>> b) & 1; b--) p++;
  return p;
}

export function inSubnet(a: string, b: string, prefix: number): boolean {
  const x = parseIp(a);
  const y = parseIp(b);
  if (x === null || y === null) return false;
  const m = maskOf(prefix);
  return ((x & m) >>> 0) === ((y & m) >>> 0);
}

export function network(ip: string, prefix: number): string {
  const x = parseIp(ip);
  return x === null ? ip : fmtIp((x & maskOf(prefix)) >>> 0);
}

/** What is wrong with a static setting, in the engine's words. */
export function problems(ip: string, prefix: number | null, gateway: string): string[] {
  const out: string[] = [];
  const a = parseIp(ip);
  if (a === null) out.push('the address is not an IPv4 address');
  if (prefix === null) out.push('the mask is not a prefix length or a netmask');
  else if (prefix > 30) out.push('a /31 or /32 leaves no room for a camera');
  if (a !== null && prefix !== null && prefix <= 30) {
    const m = maskOf(prefix);
    const host = (a & ~m) >>> 0;
    if (host === 0) out.push(`${ip} is the network address of its subnet`);
    if (host === (~m >>> 0)) out.push(`${ip} is the broadcast address of its subnet`);
    const first = a >>> 24;
    if (first === 0 || first === 127 || first >= 224) out.push(`${ip} cannot be a camera's address`);
  }
  if (gateway.trim()) {
    const g = parseIp(gateway);
    if (g === null) out.push('the gateway is not an IPv4 address');
    else if (a !== null && prefix !== null && !inSubnet(ip, gateway, prefix)) out.push(`the gateway ${gateway} is outside ${network(ip, prefix)}/${prefix}`);
    else if (g === a) out.push('the gateway cannot be the camera itself');
  }
  return out;
}

/**
 * A free address in `subnet/prefix` for a camera: its current last octet if
 * that is free there, else the first free one from .100 up, then from .2.
 */
export function suggest(subnetIp: string, prefix: number, wantHost: number | null, taken: Set<string>): string | null {
  const base = parseIp(subnetIp);
  if (base === null || prefix > 30) return null;
  const m = maskOf(prefix);
  const net = (base & m) >>> 0;
  const size = (~m >>> 0) + 1;
  const ok = (h: number) => h > 0 && h < size - 1 && !taken.has(fmtIp((net + h) >>> 0));
  if (wantHost !== null && ok(wantHost)) return fmtIp((net + wantHost) >>> 0);
  for (const start of [100, 2]) {
    for (let h = start; h < size - 1; h++) if (ok(h)) return fmtIp((net + h) >>> 0);
  }
  return null;
}

/** Sort key for an IPv4 string. */
export function ipKey(s: string): number {
  return parseIp(s) ?? 0;
}

/** `start` plus `n` addresses, or null when it runs off the subnet. */
export function nth(start: string, n: number, prefix: number): string | null {
  const a = parseIp(start);
  if (a === null) return null;
  const m = maskOf(prefix);
  const next = (a + n) >>> 0;
  if (((next & m) >>> 0) !== ((a & m) >>> 0)) return null;
  const host = (next & ~m) >>> 0;
  if (host === 0 || host === (~m >>> 0)) return null;
  return fmtIp(next);
}
