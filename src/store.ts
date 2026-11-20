import { create } from 'zustand';

import { api, message } from './lib/ipc';
import type { Alias, AppInfo, Camera, Catalog, Credentials, DriverInfo, EngineEvent, Level, LocalIface, NetConfig, ScanRequest, ScanSummary } from './types';

export interface Scope {
  /** Interfaces to broadcast on; null means every usable one. */
  ifaces: string[] | null;
  /** Routed ranges, one per line or comma-separated. */
  ranges: string;
  /** Drivers to run; null means each driver's default. */
  drivers: string[] | null;
  listenSecs: number;
}

export interface LogLine {
  t: number;
  level: Level;
  driver: string;
  message: string;
}

/** Scanning for a camera after its vendor's recovery tool has reset it. */
export interface Watch {
  driver: string;
  running: boolean;
  rounds: number;
  until: number;
  /** Cameras of that vendor that are new, or at a new address, since the watch began. */
  found: string[];
}

/** How long a watch keeps scanning: a camera reset to DHCP can take a minute to settle. */
export const WATCH_MS = 5 * 60_000;

/** The reference list of factory logins and firmware pages, open on one table. */
export interface Reference {
  tab: 'logins' | 'firmware';
  /** Shown first, or alone; null for every brand. */
  brand: string | null;
}

/** Where a change to one camera has got to. */
export interface Job {
  state: 'sending' | 'waiting' | 'done' | 'failed';
  text: string;
}

interface State {
  ready: boolean;
  info: AppInfo | null;
  ifaces: LocalIface[];
  drivers: DriverInfo[];
  /** Factory logins and firmware links, fetched once. */
  catalog: Catalog;
  cameras: Record<string, Camera>;
  selected: string | null;
  checked: string[];
  scanning: boolean;
  summary: ScanSummary | null;
  error: string | null;
  logs: LogLine[];
  showLog: boolean;
  scope: Scope;
  jobs: Record<string, Job>;
  filter: string;
  /** Temporary addresses Meerkat has added to reach cameras on the wrong subnet. */
  aliases: Alias[];
  /** The driver whose recovery tool dialog is open. */
  recoveryFor: string | null;
  watch: Watch | null;
  reference: Reference | null;

  init(): Promise<void>;
  scan(): Promise<void>;
  stop(): Promise<void>;
  setScope(p: Partial<Scope>): void;
  select(id: string | null): void;
  toggleCheck(id: string): void;
  setChecked(ids: string[]): void;
  setFilter(s: string): void;
  clearTable(): Promise<void>;
  forget(id: string): Promise<void>;
  setDemo(on: boolean): Promise<void>;
  apply(id: string, cfg: NetConfig, creds: Credentials | null): Promise<boolean>;
  readNetwork(id: string, creds: Credentials | null): Promise<NetConfig | null>;
  /** `path`: a page inside it that the catalogue names. */
  openWeb(id: string, path?: string): Promise<void>;
  toggleLog(): void;
  dismissError(): void;
  planAlias(id: string): Promise<{ alias: Alias } | { error: string }>;
  /** Add a temporary address, then scan so the vendor drivers confirm what it reaches. Returns an error, or null. */
  addAlias(alias: Alias): Promise<string | null>;
  removeAlias(alias: Alias | null): Promise<void>;
  openRecovery(driver: string | null): void;
  openReference(r: Reference | null): void;
  startWatch(driver: string): Promise<void>;
  stopWatch(): Promise<void>;
}

const SCOPE_KEY = 'meerkat.scope';

function loadScope(): Scope {
  const d: Scope = { ifaces: null, ranges: '', drivers: null, listenSecs: 2.5 };
  try {
    const raw = localStorage.getItem(SCOPE_KEY);
    return raw ? { ...d, ...JSON.parse(raw) } : d;
  } catch {
    return d;
  }
}

function saveScope(s: Scope) {
  try {
    localStorage.setItem(SCOPE_KEY, JSON.stringify(s));
  } catch {
    /* private window: the scope just isn't remembered */
  }
}

export function rangesOf(text: string): string[] {
  return text
    .split(/[\n,;]+/)
    .map((r) => r.trim())
    .filter(Boolean);
}

export function usable(i: LocalIface): boolean {
  return !i.loopback && !i.p2p && !i.linkLocal;
}

let unlisten: (() => void) | null = null;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export const useStore = create<State>((set, get) => {
  const log = (level: Level, driver: string, msg: string) =>
    set((s) => ({ logs: [...s.logs.slice(-499), { t: Date.now(), level, driver, message: msg }] }));

  const onEvent = (e: EngineEvent) => {
    if (e.type === 'camera') {
      set((s) => {
        const cameras = { ...s.cameras };
        for (const r of e.replaces) delete cameras[r];
        cameras[e.camera.id] = e.camera;
        const moved = (id: string) => (e.replaces.includes(id) ? e.camera.id : id);
        return {
          cameras,
          selected: s.selected ? moved(s.selected) : s.selected,
          checked: s.checked.map(moved),
        };
      });
    } else if (e.type === 'log') {
      log(e.level, e.driver, e.message);
    } else {
      set({ summary: e.summary });
    }
  };

  const setJob = (id: string, job: Job) => set((s) => ({ jobs: { ...s.jobs, [id]: job } }));

  /** One scan over the chosen interfaces; `over` replaces parts of the scope. */
  const runScan = async (over: Partial<ScanRequest> = {}) => {
    const b = await api();
    const { scope, ifaces } = get();
    // The interface list can change between scans (Wi-Fi, a USB adapter).
    const fresh = await b.interfaces().catch(() => ifaces);
    set({ ifaces: fresh, scanning: true, error: null, summary: null });
    const chosen = scope.ifaces === null ? [] : scope.ifaces.filter((n) => fresh.some((i) => i.name === n));
    try {
      await b.scan({
        ifaces: chosen,
        broadcast: scope.ifaces === null || chosen.length > 0,
        ranges: rangesOf(scope.ranges),
        directed: true,
        drivers: scope.drivers ?? [],
        listenMs: Math.round(scope.listenSecs * 1000),
        probeTimeoutMs: 800,
        maxHosts: 4096,
        ...over,
      });
    } catch (e) {
      set({ error: message(e) });
      log('error', 'scan', message(e));
    } finally {
      set({ scanning: false });
    }
  };

  const takeCameras = (cams: Camera[]) => set({ cameras: Object.fromEntries(cams.map((c) => [c.id, c])) });

  return {
    ready: false,
    info: null,
    ifaces: [],
    drivers: [],
    catalog: { logins: [], firmware: [] },
    cameras: {},
    selected: null,
    checked: [],
    scanning: false,
    summary: null,
    error: null,
    logs: [],
    showLog: false,
    scope: loadScope(),
    jobs: {},
    filter: '',
    aliases: [],
    recoveryFor: null,
    watch: null,
    reference: null,

    async init() {
      const b = await api();
      unlisten?.();
      unlisten = await b.onEvent(onEvent);
      try {
        const [info, ifaces, drivers, catalog, cams, aliases] = await Promise.all([b.appInfo(), b.interfaces(), b.drivers(), b.catalog(), b.cameras(), b.aliases()]);
        set({ info, ifaces, drivers, catalog, aliases, cameras: Object.fromEntries(cams.map((c) => [c.id, c])), ready: true });
      } catch (e) {
        set({ error: message(e), ready: true });
      }
    },

    async scan() {
      if (get().scanning) return;
      await runScan();
    },

    async stop() {
      set((s) => (s.watch?.running ? { watch: { ...s.watch, running: false } } : {}));
      await (await api()).stopScan();
    },

    setScope(p) {
      const scope = { ...get().scope, ...p };
      saveScope(scope);
      set({ scope });
    },

    select(id) {
      set({ selected: id });
    },

    toggleCheck(id) {
      set((s) => ({ checked: s.checked.includes(id) ? s.checked.filter((x) => x !== id) : [...s.checked, id] }));
    },

    setChecked(ids) {
      set({ checked: ids });
    },

    setFilter(filter) {
      set({ filter });
    },

    async clearTable() {
      try {
        await (await api()).clear();
        set({ cameras: {}, selected: null, checked: [], jobs: {}, summary: null });
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async forget(id) {
      await (await api()).forget(id);
      set((s) => {
        const cameras = { ...s.cameras };
        delete cameras[id];
        return { cameras, selected: s.selected === id ? null : s.selected, checked: s.checked.filter((x) => x !== id) };
      });
    },

    async setDemo(on) {
      try {
        const b = await api();
        const info = await b.setDemo(on);
        const [ifaces, aliases] = await Promise.all([b.interfaces(), b.aliases()]);
        set({ info, ifaces, aliases, cameras: {}, selected: null, checked: [], jobs: {}, summary: null, error: null, watch: null });
        log('info', 'app', on ? 'demo mode: scanning the built-in simulator, not the network' : 'demo mode off: scanning the network');
      } catch (e) {
        set({ error: message(e) });
      }
    },

    async apply(id, cfg, creds) {
      const b = await api();
      const cam = get().cameras[id];
      const label = cam?.name ?? cam?.model ?? id;
      setJob(id, { state: 'sending', text: 'Sending…' });
      try {
        const out = await b.writeNetwork(id, cfg, creds);
        log('info', cam?.driver ?? 'write', `${label}: ${out.message}`);
        if (!out.restarts && !out.expectIp) {
          setJob(id, { state: 'done', text: out.message });
          return true;
        }
        setJob(id, { state: 'waiting', text: out.expectIp ? `${out.message}. Looking for it at ${out.expectIp}…` : `${out.message}. Waiting for it to come back…` });
        const back = await b.verify(id, out);
        if (back) {
          setJob(id, { state: 'done', text: `Back at ${back.ip}` });
          log('info', cam?.driver ?? 'write', `${label}: answering at ${back.ip}`);
        } else {
          setJob(id, {
            state: 'done',
            text: `${out.message}, but it has not answered yet — it may still be restarting, or be on a subnet this computer cannot hear. Scan again in a minute.`,
          });
        }
        return true;
      } catch (e) {
        setJob(id, { state: 'failed', text: message(e) });
        log('error', cam?.driver ?? 'write', `${label}: ${message(e)}`);
        return false;
      }
    },

    async readNetwork(id, creds) {
      try {
        const cfg = await (await api()).readNetwork(id, creds);
        set((s) => (s.cameras[id] ? { cameras: { ...s.cameras, [id]: { ...s.cameras[id], net: cfg } } } : {}));
        return cfg;
      } catch (e) {
        setJob(id, { state: 'failed', text: message(e) });
        return null;
      }
    },

    async openWeb(id, path) {
      try {
        const url = await (await api()).openWebUi(id, path);
        if (!('__TAURI_INTERNALS__' in window)) window.open(url, '_blank', 'noopener');
      } catch (e) {
        set({ error: message(e) });
      }
    },

    toggleLog() {
      set((s) => ({ showLog: !s.showLog }));
    },

    dismissError() {
      set({ error: null });
    },

    async planAlias(id) {
      try {
        return { alias: await (await api()).planAlias(id) };
      } catch (e) {
        return { error: message(e) };
      }
    },

    async addAlias(alias) {
      const b = await api();
      try {
        takeCameras(await b.addAlias(alias));
      } catch (e) {
        log('error', 'address', message(e));
        return message(e);
      }
      const [ifaces, aliases] = await Promise.all([b.interfaces(), b.aliases()]);
      set({ ifaces, aliases });
      log('info', 'address', `added ${alias.ip}/${alias.prefix} to ${alias.iface} for now`);
      // What the address now reaches, the vendor drivers can confirm.
      if (!get().scanning) await runScan();
      return null;
    },

    async removeAlias(alias) {
      const b = await api();
      try {
        takeCameras(await b.removeAliases(alias ? [alias] : null));
        const [ifaces, aliases] = await Promise.all([b.interfaces(), b.aliases()]);
        set({ ifaces, aliases });
        log('info', 'address', alias ? `removed ${alias.ip} from ${alias.iface}` : 'removed the temporary addresses');
      } catch (e) {
        set({ error: message(e) });
      }
    },

    openRecovery(driver) {
      set({ recoveryFor: driver });
    },

    openReference(reference) {
      set({ reference });
    },

    async startWatch(driver) {
      const d = get().drivers.find((x) => x.id === driver);
      if (!d?.recovery || get().watch?.running) return;
      // Wait for a scan already under way rather than fail beside it.
      while (get().scanning) await sleep(200);
      const before = new Map(
        Object.values(get().cameras)
          .filter((c) => c.vendor === d.vendor)
          .map((c) => [c.id, c.ip]),
      );
      const until = Date.now() + WATCH_MS;
      set({ watch: { driver, running: true, rounds: 0, until, found: [] } });
      log('info', 'watch', `watching for a ${d.vendor} camera to come back`);
      // Every brand's discovery (the camera may answer NDI before its API),
      // plus a probe of where the tool leaves it without DHCP.
      const ranges = d.recovery.fallback ? [d.recovery.fallback] : [];
      while (get().watch?.running && Date.now() < until) {
        await runScan({ drivers: [], ranges, listenMs: 3000 });
        const w = get().watch;
        if (!w?.running) break;
        const found = Object.values(get().cameras)
          .filter((c) => c.vendor === d.vendor && before.get(c.id) !== c.ip)
          .map((c) => c.id);
        set({ watch: { ...w, rounds: w.rounds + 1, found } });
        if (found.length) {
          log('info', 'watch', `${found.length} ${d.vendor} camera${found.length === 1 ? '' : 's'} turned up`);
          break;
        }
        await sleep(2000);
      }
      set((s) => (s.watch ? { watch: { ...s.watch, running: false } } : {}));
    },

    async stopWatch() {
      set((s) => (s.watch ? { watch: { ...s.watch, running: false } } : {}));
      if (get().scanning) await (await api()).stopScan();
    },
  };
});
