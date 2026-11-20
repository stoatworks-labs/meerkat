// A browser-only stand-in for the engine, for working on the UI with
// `npm run dev`. The fixtures are the simulator's real output
// (`meerkat scan --demo --json` and friends), so the shapes are exact; the
// behaviour is a sketch: a write moves the camera and nothing more. The
// protocols themselves are tested in Rust against meerkat-sim.

import type { Camera, DriverInfo, EngineEvent, NetConfig, WriteOutcome } from '../types';
import type { Backend } from './ipc';
import { inSubnet } from './net';
import fixtureCameras from './fixtures/cameras.json';
import fixtureDrivers from './fixtures/drivers.json';
import fixtureIfaces from './fixtures/ifaces.json';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function mockBackend(): Backend {
  const all = new Map<string, Camera>((fixtureCameras as unknown as Camera[]).map((c) => [c.id, structuredClone(c)]));
  const found = new Map<string, Camera>();
  const listeners = new Set<(e: EngineEvent) => void>();
  const emit = (e: EngineEvent) => listeners.forEach((l) => l(e));
  const sim0 = fixtureIfaces[0];
  let cancel = false;

  const reach = (c: Camera): Camera['reach'] => (inSubnet(c.ip, sim0.ip, sim0.prefix) ? { kind: 'local', iface: 'sim0' } : { kind: 'mismatch', iface: 'sim0' });

  return {
    appInfo: async () => ({ version: '0.1.0', demo: true, platform: 'browser' }),
    setDemo: async () => ({ version: '0.1.0', demo: true, platform: 'browser' }),
    interfaces: async () => fixtureIfaces,
    drivers: async () => fixtureDrivers as DriverInfo[],
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
        found.set(c.id, structuredClone(c));
        emit({ type: 'camera', camera: structuredClone(c), replaces: [] });
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
    async openWebUi(id) {
      const c = found.get(id);
      if (!c?.webUrl) throw 'this camera has no web UI address';
      return c.webUrl;
    },
    async onEvent(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
