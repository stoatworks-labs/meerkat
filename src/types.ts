// The engine's types as they cross the IPC boundary (serde camelCase).
// Mirrors meerkat-core: model.rs, net.rs, driver.rs, engine.rs, catalog.rs.

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
  /** The catalogue rows about this camera, by id (catalog.rs). */
  catalog: CatalogMatch;
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

/**
 * Rows of one catalogue table. `exact`: for this model, or for every model of
 * its brand. Not exact: the brand's rows for other models, offered because
 * Meerkat does not know this one (or the camera did not say).
 */
export interface Matched {
  ids: string[];
  exact: boolean;
}

export interface CatalogMatch {
  logins: Matched;
  firmware: Matched;
}

/** vendor: the maker's own document, read. vendorExcerpt: only an excerpt of it checked. thirdParty: a mirror or reseller. */
export type SourceKind = 'vendor' | 'vendorExcerpt' | 'thirdParty';

export interface Source {
  title: string;
  url: string;
  kind: SourceKind;
}

/** factory: an empty username means password only; an empty password means blank. create: no factory login, the account is made at first login. */
export type Login = { kind: 'factory'; username: string; password: string } | { kind: 'create'; username: string | null };

export interface LoginEntry {
  id: string;
  brand: string;
  family: string;
  /** The firmware this row is for, when the firmware decides. */
  firmware: string | null;
  login: Login;
  changeAtFirstLogin: boolean;
  lockout: string | null;
  notes: string[];
  sources: Source[];
}

export interface FirmwareEntry {
  id: string;
  brand: string;
  family: string;
  downloads: string;
  downloadsNote: string | null;
  how: string[];
  /** The camera's own firmware page, to open on its web address. */
  webPath: string | null;
  warnings: string[];
  /** A published network upload: information only, Meerkat does not upload firmware. */
  upload: string | null;
  sources: Source[];
}

export interface Catalog {
  logins: LoginEntry[];
  firmware: FirmwareEntry[];
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
