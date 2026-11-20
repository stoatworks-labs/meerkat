// The app's commands (src-tauri/src/lib.rs). Outside Tauri — `npm run dev`
// in a browser — a mock built from the simulator's real output stands in, so
// the UI can be worked on without the desktop shell.

import type {
  Alias,
  AppInfo,
  Camera,
  Catalog,
  Credentials,
  DriverInfo,
  EngineEvent,
  FirmwareCheck,
  InstallEvent,
  InstallReport,
  LocalIface,
  NetConfig,
  ScanRequest,
  ScanSummary,
  StagedFile,
  WriteOutcome,
} from '../types';

export const inTauri = '__TAURI_INTERNALS__' in window;

export interface Backend {
  appInfo(): Promise<AppInfo>;
  setDemo(on: boolean): Promise<AppInfo>;
  interfaces(): Promise<LocalIface[]>;
  drivers(): Promise<DriverInfo[]>;
  catalog(): Promise<Catalog>;
  cameras(): Promise<Camera[]>;
  scan(req: ScanRequest): Promise<ScanSummary>;
  stopScan(): Promise<void>;
  forget(id: string): Promise<void>;
  clear(): Promise<void>;
  readNetwork(id: string, creds: Credentials | null): Promise<NetConfig>;
  writeNetwork(id: string, cfg: NetConfig, creds: Credentials | null): Promise<WriteOutcome>;
  verify(id: string, outcome: WriteOutcome, timeoutSecs?: number): Promise<Camera | null>;
  /** What the camera runs, and whether it takes a firmware file. Reads only. */
  firmwareCheck(id: string, creds: Credentials | null): Promise<FirmwareCheck>;
  /** Hand the app a firmware file's bytes, held until an install takes it. */
  firmwareStage(data: Uint8Array): Promise<StagedFile>;
  firmwareInstall(id: string, token: string, name: string, creds: Credentials | null): Promise<InstallReport>;
  onInstall(cb: (e: InstallEvent) => void): Promise<() => void>;
  /** `path`: a page inside the web UI that the catalogue names (a firmware page). */
  openWebUi(id: string, path?: string): Promise<string>;
  aliases(): Promise<Alias[]>;
  planAlias(id: string): Promise<Alias>;
  addAlias(alias: Alias): Promise<Camera[]>;
  removeAliases(only: Alias[] | null): Promise<Camera[]>;
  onEvent(cb: (e: EngineEvent) => void): Promise<() => void>;
}

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');
  return {
    appInfo: () => invoke('app_info'),
    setDemo: (on) => invoke('set_demo', { on }),
    interfaces: () => invoke('interfaces'),
    drivers: () => invoke('drivers'),
    catalog: () => invoke('catalog'),
    cameras: () => invoke('cameras'),
    scan: (req) => invoke('scan', { req }),
    stopScan: () => invoke('stop_scan'),
    forget: (id) => invoke('forget', { id }),
    clear: () => invoke('clear'),
    readNetwork: (id, creds) => invoke('read_network', { id, creds }),
    writeNetwork: (id, cfg, creds) => invoke('write_network', { id, cfg, creds }),
    verify: (id, outcome, timeoutSecs) => invoke('verify', { id, outcome, timeoutSecs }),
    firmwareCheck: (id, creds) => invoke('firmware_check', { id, creds }),
    // The bytes go as the raw request body, not as JSON.
    firmwareStage: (data) => invoke('firmware_stage', data),
    firmwareInstall: (id, token, name, creds) => invoke('firmware_install', { id, token, name, creds }),
    onInstall: async (cb) => listen<InstallEvent>('meerkat://install', (e) => cb(e.payload)),
    openWebUi: (id, path) => invoke('open_web_ui', { id, path: path ?? null }),
    aliases: () => invoke('aliases'),
    planAlias: (id) => invoke('plan_alias', { id }),
    addAlias: (alias) => invoke('add_alias', { alias }),
    removeAliases: (only) => invoke('remove_aliases', { only }),
    onEvent: async (cb) => listen<EngineEvent>('meerkat://event', (e) => cb(e.payload)),
  };
}

let backend: Promise<Backend> | null = null;

export function api(): Promise<Backend> {
  if (!backend) backend = inTauri ? tauriBackend() : import('./mock').then((m) => m.mockBackend());
  return backend;
}

/** An error from a command, as the sentence the engine wrote. */
export function message(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
