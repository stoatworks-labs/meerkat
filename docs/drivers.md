# Adding a brand

Every brand Meerkat knows is a **driver**: one file in
`src-tauri/crates/meerkat-drivers/src/` implementing `meerkat_core::Driver`, listed in
`meerkat_drivers::all()`. The engine, the desktop app and the CLI find it there. Nothing else
needs to know about it: the UI draws its rows, its detail panel, its login fields and its notes
from what the driver reports.

`vhd.rs` is the smallest complete example (discovery only). `sony.rs` is a layer-2 write by MAC,
`birddog.rs` an HTTP write followed by a reboot, and `onvif.rs` a write that needs a login.

## The trait

```rust
#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn info(&self) -> DriverInfo;
    async fn discover(&self, ctx: &ScanCtx) -> Result<()>;                         // default: nothing
    async fn probe(&self, ctx: &ScanCtx, ip: Ipv4Addr) -> Result<Option<Camera>>;  // default: None
    fn can_probe(&self) -> bool;                                                   // default: false
    async fn sweep(&self, ctx: &ScanCtx, targets: &[Ipv4Addr], via: &str) -> Result<()>;
    async fn read_network(&self, op: &OpCtx, cam: &Camera) -> Result<NetConfig>;
    async fn write_network(&self, op: &OpCtx, cam: &Camera, cfg: &NetConfig) -> Result<WriteOutcome>;
    fn web_url(&self, cam: &Camera) -> Option<String>;                             // default: http://ip[:web_port]/
    async fn check_firmware(&self, op: &OpCtx, cam: &Camera) -> Result<FirmwareCheck>;      // default: Unsupported
    async fn install_firmware(&self, op: &OpCtx, cam: &Camera, file: &FirmwareFile,
                              stage: &(dyn Fn(InstallStage) + Send + Sync)) -> Result<WriteOutcome>;  // default: Unsupported
}
```

Implement only what the brand can do. A driver that only discovers is still useful: its rows
get a web UI link.

### `info()`

| Field | What it is for |
| --- | --- |
| `id` | Stable, lower case. It goes in `Camera::driver`, the CLI's `--driver`, and port-map keys. |
| `vendor`, `protocol` | Shown in the UI. Use the protocol's name as the vendor's own tool calls it. |
| `priority` | Who owns a camera when several drivers see it: vendor drivers 50+, ONVIF 30, mDNS/NDI 10. The higher one wins, and a driver of equal priority cannot take a row from another driver. |
| `discovery`, `sweep` | One line each, for the driver list: how it finds cameras, and how it probes one address (or `None`). |
| `notes` | The limits a user needs to know before pressing Apply. Write them as plain sentences. |
| `enabled_by_default` | Off for anything slow or noisy. |
| `firmware` | How the driver sends a firmware file, in one line, or `None`. Set it only with `check_firmware` and `install_firmware`, for a published upload: see below. |
| `recovery` | The vendor's own tool for a camera nothing can reach, such as one given a bad address (BirdDog's Neuralyzer), or `None`. It holds the tool's name and download URL, the models it covers, what it does, the steps, what the camera does afterwards, and the `fallback` address it comes back on, which the watch probes as well. The UI links to the tool and then watches for the camera. |

### Discovery: `discover(ctx)`

Send the brand's broadcast or multicast out of every leg in `ctx.legs`. Each leg is a local
address to bind, an interface index to pin to, and the destinations for that interface: the
limited broadcast, the interface's directed broadcast and, on the `routed` leg, the directed
broadcasts of the subnets the user listed. Listen until `ctx.listen` runs out or `ctx.cancel`
fires, and report each answer with `ctx.found(camera, Method::…, &leg.iface)`.

The rules that keep a driver testable and safe:

- **Address every datagram with `ctx.to(ip, port)`**, never with `SocketAddrV4::new` directly,
  and take ports from `ctx.port("name", DEFAULT)`. In demo mode those two calls send the
  traffic to the simulator instead of the network. A driver that builds its own addresses would
  send demo-mode traffic to real cameras.
- **Bind to `leg.bind`**, not `0.0.0.0`, so a show laptop with two networks sends each
  broadcast out of the interface it is meant for.
- **Use `ctx.http`** for HTTP. It carries the same rerouting.
- Do not log every silent address. `ctx.log(Level::Debug, …)` is for things a user debugging a
  scan would want.

### The camera you report

`Camera::new(ID, vendor, ip)`, then fill in what the protocol said:

- `mac`: report it if the protocol carries it. The MAC is the row's key. Without one the row is
  keyed by IP and is lost when the address changes.
- `net`: the full settings, if discovery carried them (Easy IP and Sony do). It saves a read.
- `web_port`, when the web UI is not on 80.
- `caps`: what this camera can do through this driver: `read_network`, `write_network`,
  `write_by_mac` (a layer-2 write that reaches a camera on the wrong subnet), `needs_auth`,
  `dhcp`, `dns`, `hostname`, `reboot`. Be honest: the UI offers exactly what `caps` says.
- `fields`: anything else the protocol said, verbatim. It is shown in the detail panel.

The engine fills in `id`, `reach` (Local, Routed or Mismatch, from the camera's address against
the interface that heard it), `web_url` and `last_seen`, and merges your report with any other
driver's report of the same camera.

### Routed subnets: `probe` / `sweep`

If one address can be asked "are you one of mine?" (an HTTP GET, a unicast UDP inquiry), set
`can_probe() -> true` and implement `probe`. The default `sweep` runs it `ctx.concurrency` at a
time. A driver whose probe is a single datagram should override `sweep`: send to every target
from one socket, then listen once (Sony and ONVIF do this).

Each vendor driver is also asked to probe rows that only mDNS found (NDI, `_http._tcp`), when
the vendor matches or is unknown. That is how BirdDog's API confirms a camera NDI heard.

### Changing settings: `write_network`

Before your `write_network` is called the engine has already checked that `caps.write_network`
is set, that the settings are valid (`net::validate`), that DHCP is only asked of a driver with
`caps.dhcp`, that no other camera in the table holds the address, and that an HTTP-only driver
is not being asked to reach a camera on the wrong subnet. What is left for the driver:

- Read `op.creds()` if the write needs a login. Return `DriverError::NeedsAuth` when it is
  missing, so the UI asks for one.
- Send the change. Return an error the user can act on. Unsupported fields
  (`DriverError::Unsupported`) and refusals (`DriverError::Protocol("…")`) both reach the UI as
  their message, word for word.
- Return a `WriteOutcome`: whether the camera restarts, the address to look for it at, and how
  long to wait before looking (`settle_ms`). `engine.verify` then rediscovers it there and
  keeps its row, because the row is keyed by MAC.

Never send a write you cannot vouch for. If two dialects exist and neither is confirmed (the
VHD platform), make the driver discovery-only and say so in `notes`.

### Firmware: `check_firmware` / `install_firmware`

Only for an upload the brand **publishes**: ONVIF's `StartFirmwareUpgrade` (Core §8.3.9) is the
one implemented. A driver that sets `info().firmware` can send a file to any camera it has seen,
even one another driver owns: the engine records the driver in `Camera::firmware_via`.

- `check_firmware` reads only. Return the camera's maker, model and version, and `accepts:
  true` only when the camera itself says it takes a file this way (ONVIF's
  `HttpFirmwareUpgrade` capability). Otherwise `accepts: false` and a `note` that says why.
- `install_firmware` is called after the engine has checked that no scan is running, that the
  file is neither empty nor over 1 GiB, that the camera is not on the wrong subnet, and that
  `check_firmware` accepts. Call `stage` as each step starts (`Pausing`, `Sending`), send the
  file **to `cam.ip` only**, whatever host the camera names, and return a `WriteOutcome` with
  `restarts: true`, `expect_ip: Some(cam.ip)` and `settle_ms` set to the time the camera says
  it will be away. The engine waits it out, rediscovers the camera and calls `check_firmware`
  again: only that version says the install took.
- Map the camera's refusals to messages a user can act on: HTTP 415 is a file the camera does
  not take, 401 is the login. A connection that drops after the file has gone may mean the
  camera is already restarting: say so rather than call it a failure.
- Send it with `op.http.upload`, which takes the digest challenge with a GET first, so a large
  file crosses the network once, and gives the POST a timeout that grows with the file.

## Factory logins and firmware pages

What a brand's cameras log in with out of the box, and where its maker publishes firmware, are
rows in two tables, not driver code: `meerkat-drivers/src/logins.rs` and `firmware.rs`. The
shape and the matching rule are in `meerkat-core/src/catalog.rs`. The engine matches every
camera it finds against them and puts the ids in `Camera::catalog`, and the UI and the CLI show
the rows. A brand with no driver can still have rows: an ONVIF camera that says it is an AVer
finds AVer's.

A row names its brand with `vendors`, lower-case pieces of the camera's vendor string, and its
models with `models`. Each pattern is matched against the start of the model, ignoring case,
spaces and punctuation, so `AW-UE150` also matches `aw ue150` and `AW-UE150W`:

- `*` stands for anything: `PT*-G3`.
- A pattern ending in a digit does not run on into more digits: `AW-UE4` is not `AW-UE40`.
- A trailing `$` means the model ends there: `X1$` is the X1 but not the X1 Ultra.
- An empty `models` means every model of the brand.

When no pattern matches, or the camera did not report a model, the brand's rows are still
offered, marked as a hint (`exact: false`).

The rules for a row:

- **Every fact has a source**, with the document and the place in it. Prefer the maker's own
  document (`SourceKind::Vendor`). Use `VendorExcerpt` when only an excerpt of it could be
  checked, and `ThirdParty` for a mirror or a reseller's FAQ. The UI flags both.
- **When firmware decides the login**, write two rows with the same models and a `firmware` line
  each. Do not guess which applies.
- **A camera with no factory login** (the account is made at first use) is `Login::Create`, not
  a blank password.
- **Say what the maker says about lockouts.** Meerkat never tries a login by itself: "Use this
  login" only fills the fields.
- **A firmware row links the maker's page.** Never a file, and never a mirror. `how` and
  `warnings` repeat the maker's steps. `web_path` is the camera's own firmware page, only when
  the maker's procedure shows its address. The app opens it only if a row names it. `upload`
  records a published network upload, for information: the catalogue never sends a file. A
  driver's `install_firmware` does, and only over a protocol it implements and tests.

`cargo test -p meerkat-drivers` checks every row is sourced and well formed. Add your models to
the tests that check each camera finds its own row.

## Testing it

Unit tests sit next to the code: packet builders and parsers against captures or the vendor's
documented examples.

End to end, add the brand to the simulator in `src-tauri/crates/meerkat-sim/src/lib.rs`:

1. A `Kind` variant, and one or two cameras in `demo_cameras()`: one on the right subnet, one on
   the wrong one, and one that refuses, if the brand can refuse.
2. A responder task bound to loopback on an ephemeral port, answering the way the docs say the
   camera does. Add its port to `Ports` and `port_map()` under the key your driver reads with
   `ctx.port`.
3. If the camera has HTTP, give it an entry in `http_ports` and handle its paths in `serve`.
   Each camera gets its own listener, and the sim's `route()` sends the camera's address and
   port there, wherever it has been moved to.
4. A test in `tests/e2e.rs`: the scan finds it with the right reach and caps, a write lands and
   `verify` finds it at its new address in the same row, and each refusal says why.
5. For a firmware upload, have the simulated camera take the simulator's own image (`firmware_file(model,
   version)`: three text lines, `MEERKAT-SIM-FIRMWARE`, `model=…`, `version=…`), refuse one for
   another model, drop off the network for the time it said, and come back on the new version.
   `onvif_upload` in the simulator is the example, with HTTP digest on the upload.

`cargo test --workspace` runs the lot, including the desktop app's command layer
(`src-tauri/src/lib.rs`, on Tauri's mock runtime). `npm run app:demo` shows the new brand in the
window, against the simulator only.

## Before it touches a real camera

Read first, then write. Run a scan and check the row, then `meerkat-cli set … ` without `--yes`
(a dry run), and only then the write, on a camera that nobody is using. A firmware upload is the
same: `meerkat-cli install <camera>` alone asks, with a file and no `--yes` it is a dry run, and
it goes to a bench camera with the maker's file, never one in use.
