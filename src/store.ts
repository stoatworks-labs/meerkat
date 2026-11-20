import { create } from 'zustand';

import { api, message } from './lib/ipc';
import type { AppInfo, Camera, Credentials, DriverInfo, EngineEvent, Level, LocalIface, NetConfig, ScanSummary } from './types';

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
  openWeb(id: string): Promise<void>;
  toggleLog(): void;
  dismissError(): void;
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

  return {
    ready: false,
    info: null,
    ifaces: [],
    drivers: [],
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

    async init() {
      const b = await api();
      unlisten?.();
      unlisten = await b.onEvent(onEvent);
      try {
        const [info, ifaces, drivers, cams] = await Promise.all([b.appInfo(), b.interfaces(), b.drivers(), b.cameras()]);
        set({ info, ifaces, drivers, cameras: Object.fromEntries(cams.map((c) => [c.id, c])), ready: true });
      } catch (e) {
        set({ error: message(e), ready: true });
      }
    },

    async scan() {
      if (get().scanning) return;
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
        });
      } catch (e) {
        set({ error: message(e) });
        log('error', 'scan', message(e));
      } finally {
        set({ scanning: false });
      }
    },

    async stop() {
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
        const ifaces = await b.interfaces();
        set({ info, ifaces, cameras: {}, selected: null, checked: [], jobs: {}, summary: null, error: null });
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

    async openWeb(id) {
      try {
        const url = await (await api()).openWebUi(id);
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
  };
});
