// A browser-only stand-in for the engine, for working on the UI with
// `npm run dev`. The fixtures are the simulator's real output
// (`meerkat scan --demo --json` and friends), so the shapes are exact; the
// behaviour is a sketch: a write moves the camera and nothing more, and a
// firmware install takes the simulator's three-line image (FIRMWARE_MAGIC in
// meerkat-sim) and nothing else. The protocols themselves are tested in Rust
// against meerkat-sim.

import type { Alias, Camera, Catalog, DriverInfo, EngineEvent, InstallEvent, InstallStage, LocalIface, NetConfig, WriteOutcome } from '../types';
import type { Backend } from './ipc';
import { fmtIp, inSubnet, network, parseIp } from './net';
import fixtureCameras from './fixtures/cameras.json';
import fixtureCatalog from './fixtures/catalog.json';
import fixtureDrivers from './fixtures/drivers.json';
import fixtureIfaces from './fixtures/ifaces.json';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function mockBackend(): Backend {
  const all = new Map<string, Camera>((fixtureCameras as unknown as Camera[]).map((c) => [c.id, structuredClone(c)]));
  const found = new Map<string, Camera>();
  const listeners = new Set<(e: EngineEvent) => void>();
  const emit = (e: EngineEvent) => listeners.forEach((l) => l(e));
  const installListeners = new Set<(e: InstallEvent) => void>();
  const stage = (id: string, s: InstallStage) => installListeners.forEach((l) => l({ id, stage: s }));
  let staged: { token: string; data: Uint8Array } | null = null;
  const firmwareTarget = (id: string, creds: { username: string } | null) => {
    const c = found.get(id);
    if (!c) throw 'that camera is no longer in the list; scan again';
    if (!c.firmwareVia) throw "no driver here can send this camera a firmware file; follow its maker's steps";
    if (c.reach.kind === 'mismatch') throw `${c.ip} is outside ${c.reach.iface}'s subnet, and a firmware file goes over HTTP, which cannot reach it`;
    if (!creds?.username) throw 'this camera needs a username and password';
    return c;
  };
  const sim0 = fixtureIfaces[0];
  let aliases: Alias[] = [];
  let cancel = false;

  const ifaces = (): LocalIface[] => [
    ...fixtureIfaces,
    ...aliases.map((a) => ({ name: a.iface, index: a.index, ip: a.ip, prefix: a.prefix, broadcast: null, loopback: false, p2p: false, linkLocal: false })),
  ];
  const reach = (c: Camera): Camera['reach'] =>
    ifaces().some((i) => inSubnet(c.ip, i.ip, i.prefix)) ? { kind: 'local', iface: 'sim0' } : { kind: 'mismatch', iface: 'sim0' };
  const refresh = () => {
    for (const [id, c] of found) found.set(id, { ...c, reach: reach(c) });
    for (const [id, c] of all) all.set(id, { ...c, reach: reach(c) });
    return [...found.values()];
  };

  return {
    appInfo: async () => ({ version: '0.1.0', demo: true, platform: 'browser' }),
    setDemo: async () => ({ version: '0.1.0', demo: true, platform: 'browser' }),
    interfaces: async () => ifaces(),
    drivers: async () => fixtureDrivers as DriverInfo[],
    catalog: async () => fixtureCatalog as Catalog,
    cameras: async () => [...found.values()],
    async scan(req) {
      cancel = false;
      const t0 = performance.now();
      emit({ type: 'log', level: 'info', driver: 'scan', message: 'broadcasting on sim0 (192.168.10.20/24) — browser mock' });
      for (const r of req.ranges) emit({ type: 'log', level: 'info', driver: 'scan', message: `sweeping ${r}` });
      let n = 0;
      for (const c of all.values()) {
        if (cancel) break;
        if (req.drivers.length && !c.seen.some((s) => req.drivers.includes(s.driver))) continue;
        await sleep(90 + Math.random() * 160);
        const isNew = !found.has(c.id);
        const now = { ...structuredClone(c), reach: reach(c) };
        found.set(c.id, now);
        emit({ type: 'camera', camera: structuredClone(now), replaces: [] });
        if (isNew) n++;
      }
      const summary = { cameras: found.size, new: n, hostsSwept: 0, elapsedMs: Math.round(performance.now() - t0), cancelled: cancel };
      emit({ type: 'done', summary });
      return summary;
    },
    stopScan: async () => {
      cancel = true;
    },
    forget: async (id) => {
      found.delete(id);
    },
    clear: async () => {
      found.clear();
    },
    async readNetwork(id, creds) {
      const c = found.get(id);
      if (!c) throw 'that camera is no longer in the list; scan again';
      if (c.caps.needsAuth && !creds?.username) throw 'this camera needs a username and password';
      return c.net ?? { dhcp: false, ip: c.ip, prefix: 24, gateway: null, dns: [], hostname: null };
    },
    async writeNetwork(id, cfg: NetConfig, creds) {
      const c = found.get(id);
      if (!c) throw 'that camera is no longer in the list; scan again';
      if (!c.caps.writeNetwork) throw `the ${c.vendor} driver cannot change this camera's network settings`;
      if (c.caps.needsAuth && !creds?.username) throw 'this camera needs a username and password';
      const other = [...found.values()].find((o) => o.ip === cfg.ip && o.id !== id);
      if (!cfg.dhcp && other) throw `${cfg.ip} is already in use by ${other.name ?? other.model ?? other.vendor}`;
      await sleep(600);
      const moved = { ...c, ip: cfg.dhcp ? c.ip : cfg.ip, net: { ...cfg } };
      moved.reach = reach(moved);
      moved.webUrl = `http://${moved.ip}/`;
      all.set(id, moved);
      found.set(id, moved);
      const out: WriteOutcome = { message: `The camera accepted ${cfg.ip}/${cfg.prefix}`, restarts: true, expectIp: cfg.dhcp ? null : cfg.ip, settleMs: 800 };
      return out;
    },
    async verify(id, outcome) {
      await sleep(outcome.settleMs);
      const c = found.get(id) ?? null;
      if (c) emit({ type: 'camera', camera: structuredClone(c), replaces: [] });
      return c;
    },
    async firmwareCheck(id, creds) {
      const c = firmwareTarget(id, creds);
      await sleep(300);
      return { driver: 'onvif', method: 'ONVIF StartFirmwareUpgrade (HTTP POST)', manufacturer: c.vendor, model: c.model, version: c.firmware ?? '1.00', accepts: true, note: null };
    },
    async firmwareStage(data) {
      if (data.length === 0) throw 'that file is empty';
      staged = { token: String(Date.now()), data };
      return { token: staged.token, size: data.length };
    },
    async firmwareInstall(id, token, _name, creds) {
      if (!staged || staged.token !== token) throw 'that file is no longer held; choose it again';
      const data = staged.data;
      staged = null;
      const c = firmwareTarget(id, creds);
      const before = c.firmware ?? '1.00';
      stage(id, { kind: 'preparing' });
      await sleep(400);
      stage(id, { kind: 'pausing', secs: 1 });
      await sleep(300);
      stage(id, { kind: 'sending', bytes: data.length });
      await sleep(800);
      const [magic, ...rest] = new TextDecoder().decode(data.slice(0, 4096)).split('\n');
      const field = (k: string) => rest.find((l) => l.startsWith(`${k}=`))?.slice(k.length + 1).trim();
      if (magic !== 'MEERKAT-SIM-FIRMWARE' || field('model') !== c.model || !field('version')) {
        throw "The camera refused the file: HTTP 415, ONVIF's answer for a file that is not valid for it. Check that it is the maker's firmware for this model.";
      }
      stage(id, { kind: 'installing', secs: 2 });
      await sleep(2000);
      stage(id, { kind: 'looking' });
      await sleep(1200);
      const now = { ...c, firmware: field('version')! };
      all.set(id, now);
      found.set(id, now);
      emit({ type: 'camera', camera: structuredClone(now), replaces: [] });
      return { message: 'The camera took the file', before, after: now.firmware, back: true, waitedSecs: 1 };
    },
    async onInstall(cb) {
      installListeners.add(cb);
      return () => installListeners.delete(cb);
    },
    async openWebUi(id, path) {
      const c = found.get(id);
      if (!c?.webUrl) throw 'this camera has no web UI address';
      if (!path) return c.webUrl;
      if (!(fixtureCatalog as Catalog).firmware.some((r) => r.webPath === path)) throw `${path} is not a page Meerkat knows`;
      return new URL(path, c.webUrl).toString();
    },
    aliases: async () => aliases,
    async planAlias(id) {
      const c = found.get(id);
      if (!c) throw 'that camera is no longer in the list; scan again';
      if (c.reach.kind !== 'mismatch') throw `${c.ip} is already on ${c.reach.kind === 'local' ? c.reach.iface : 'a reachable'} subnet`;
      const taken = new Set([...found.values()].map((o) => o.ip));
      const base = parseIp(network(c.ip, 24))!;
      for (let h = 253; h > 1; h--) {
        const ip = fmtIp(base + h);
        if (!taken.has(ip)) return { iface: sim0.name, index: null, ip, prefix: 24 };
      }
      throw 'no free address left';
    },
    async addAlias(a) {
      const clash = ifaces().find((i) => inSubnet(a.ip, i.ip, Math.min(i.prefix, a.prefix)));
      if (clash) throw `${a.ip}/${a.prefix} overlaps ${clash.ip}/${clash.prefix} on ${clash.name}, a network this computer is already on`;
      await sleep(700); // the admin password dialog
      aliases = [...aliases, a];
      return refresh();
    },
    async removeAliases(only) {
      await sleep(400);
      aliases = only ? aliases.filter((a) => !only.some((o) => o.iface === a.iface && o.ip === a.ip)) : [];
      return refresh();
    },
    async onEvent(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
