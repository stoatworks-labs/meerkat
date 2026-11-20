// The engine's types as they cross the IPC boundary (serde camelCase).
// Mirrors meerkat-core: model.rs, net.rs, driver.rs, engine.rs.

export type Method = 'broadcast' | 'multicast' | 'mdns' | 'unicast';

export interface NetConfig {
  dhcp: boolean;
  ip: string;
  prefix: number;
  gateway?: string | null;
  dns: string[];
  hostname?: string | null;
}

export interface Sighting {
  driver: string;
  method: Method;
  via: string;
}

export type Reach =
  | { kind: 'local'; iface: string }
  | { kind: 'routed' }
  | { kind: 'mismatch'; iface: string }
  | { kind: 'unknown' };

export interface Caps {
  readNetwork: boolean;
  writeNetwork: boolean;
  writeByMac: boolean;
  needsAuth: boolean;
  dhcp: boolean;
  dns: boolean;
  hostname: boolean;
  reboot: boolean;
}

export interface Camera {
  id: string;
  driver: string;
  vendor: string;
  model: string | null;
  name: string | null;
  mac: string | null;
  ip: string;
  net: NetConfig | null;
  firmware: string | null;
  serial: string | null;
  webPort: number | null;
  webUrl: string | null;
  caps: Caps;
  fields: Record<string, string>;
  seen: Sighting[];
  reach: Reach;
  lastSeen: number;
}

export interface Credentials {
  username: string;
  password: string;
}

export interface LocalIface {
  name: string;
  index: number | null;
  ip: string;
  prefix: number;
  broadcast: string | null;
  loopback: boolean;
  p2p: boolean;
  linkLocal: boolean;
}

export interface DriverInfo {
  id: string;
  vendor: string;
  protocol: string;
  priority: number;
  discovery: string[];
  sweep: string | null;
  defaultLogin: { username: string; password: string } | null;
  notes: string[];
  enabledByDefault: boolean;
  recovery: Recovery | null;
}

/** A vendor's own tool for a camera nothing else can reach (driver.rs). */
export interface Recovery {
  tool: string;
  url: string;
  models: string;
  does: string;
  steps: string[];
  after: string;
  fallback: string | null;
}

/** A temporary second address on an interface (alias.rs). */
export interface Alias {
  iface: string;
  index: number | null;
  ip: string;
  prefix: number;
}

export interface ScanRequest {
  ifaces: string[];
  broadcast: boolean;
  ranges: string[];
  directed: boolean;
  drivers: string[];
  listenMs: number;
  probeTimeoutMs: number;
  maxHosts: number;
}

export interface ScanSummary {
  cameras: number;
  new: number;
  hostsSwept: number;
  elapsedMs: number;
  cancelled: boolean;
}

export interface WriteOutcome {
  message: string;
  restarts: boolean;
  expectIp: string | null;
  settleMs: number;
}

export type Level = 'debug' | 'info' | 'warn' | 'error';

export type EngineEvent =
  | { type: 'camera'; camera: Camera; replaces: string[] }
  | { type: 'log'; level: Level; driver: string; message: string }
  | { type: 'done'; summary: ScanSummary };

export interface AppInfo {
  version: string;
  demo: boolean;
  platform: string;
}
