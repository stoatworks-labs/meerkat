# AGENTS.md — bringing an LLM up to speed on Meerkat

Orientation for an AI assistant (or a new human) picking this project up cold. `CLAUDE.md`
holds the short command reference. This file explains the model and the traps.

## 1. What this is

A desktop app that finds PTZ cameras on the network, including cameras on the wrong subnet and
cameras behind a router, changes their IP settings where the brand allows it, and opens their
built-in web UI. Tauri v2, a React + zustand front end, and a Rust engine with one driver per
brand. No accounts, no telemetry, no cloud.

It is a desktop app and not a web tool because discovery needs raw UDP broadcast, multicast,
mDNS and sockets bound to each interface. A browser can do none of them.

## 2. Layout

```
src/                     React UI: store.ts (zustand), lib/ipc.ts (Tauri or mock), lib/net.ts
src-tauri/src/lib.rs     Tauri commands: a thin layer over the engine, plus its mock-runtime test
src-tauri/crates/
  meerkat-core/          Engine, registry (merge), Driver trait, contexts, net helpers, Http
  meerkat-drivers/       panasonic, sony, birddog, vhd, onvif, ndi (+ udp/util helpers)
  meerkat-sim/           Simulated cameras for every protocol, and tests/e2e.rs
  meerkat-cli/           The engine from a terminal (binary meerkat-cli)
docs/drivers.md          How to add a brand
```

## 3. How a scan works

1. **Legs.** For each chosen interface the engine makes a leg: bind to that interface's address,
   pin to its index, and send to 255.255.255.255 and the interface's directed broadcast. If the
   user listed routed subnets, a `routed` leg also sends to their directed broadcasts.
2. **Phase 1.** Every enabled driver's `discover` runs at once over the legs. Panasonic (Easy
   IP) and Sony (IP Setup) are layer-2 broadcasts, so they hear a camera whatever its address.
   NDI is mDNS. ONVIF is WS-Discovery multicast, and its answers are unicast, so it misses
   cameras on the wrong subnet. VHD is multicast.
3. **Routed sweep.** For each routed range, every driver with `can_probe` sweeps it by unicast.
4. **Phase 2 (enrichment).** Rows owned by a low-priority driver (<50) and seen only over mDNS
   are probed by the vendor drivers whose vendor matches, or by all of them if the vendor is
   unknown. That is how an NDI sighting becomes a BirdDog row with API access.
5. **Merge** (registry.rs). A report with a MAC absorbs an `ip:` row at the same address. A
   report without a MAC attaches to the row at its IP. The higher-priority driver owns the row,
   and an equal-priority driver cannot steal it. Every sighting is kept in `seen`.
6. **Reach.** Local (inside an interface's subnet), Routed (answered a unicast probe across a
   router), Mismatch (heard at layer 2 on an interface whose subnet it is outside), or Unknown.

Writes go through `Engine::write_network`, which checks caps, validity, DHCP support, address
clashes and reachability before calling the driver. Then `Engine::verify` rediscovers the
camera at its new address in the same row.

## 4. The sandbox (demo mode), and why `ctx.to()` matters

`Engine::sandboxed(drivers, Sandbox)` replaces the interface list with the simulator's `sim0`
(192.168.10.20/24), binds every socket to 127.0.0.1, overrides discovery ports through the port
map, and rewrites every destination through a `Route`:

- loopback passes through;
- 255.255.255.255, sim0's broadcast and multicast groups go to the responders on 127.0.0.1;
- a camera's address and HTTP port go to that camera's own listener, wherever it has moved to;
- **anything else goes to a closed "dead" port.**

So demo mode cannot reach the network, **provided every driver addresses its datagrams with
`ctx.to(ip, port)` / `op.to(ip, port)` and its HTTP through `ctx.http`**. `Http` rewrites URLs
through the same route, and `Engine::browser_url` does it for Open web UI. A driver that calls
`SocketAddrV4::new(ip, port)` for a destination breaks that guarantee.

## 5. Brand facts that shape the code

- **Panasonic Easy IP:** a write is mode 03 (set) then 02 (confirm), by MAC. Cameras take it only
  in the first 20 minutes after power-on, unless the camera is set to unlimited. No DHCP.
  UE160-generation cameras with authentication on use an RSA variant that is not implemented.
- **Sony IP Setup:** `ENQ:network` broadcast on 52380, then a set by MAC. `WRITE:off` in the
  reply means the camera has locked it (BRC-AM7 and FR7 lock 20 minutes after power-on). The name
  is at most 8 characters.
- **BirdDog:** found through NDI and confirmed on :8080. X-series and MAX (firmware 6.x) take
  `/EthSetup` and then a reboot. P-series and converters (5.5.x) have no network API: they get
  a web UI link only. Out of the box a BirdDog uses DHCP, falling back to 192.168.100.100.
- **ONVIF:** UsernameToken digest. It needs the camera's login for both reads and writes.
- **VHD (PTZOptics):** discovery only, because the set dialect is unverified.
- **NDI:** sources at this computer's own addresses are skipped (its own NDI outputs).

## 6. Traps

- The CLI binary is `meerkat-cli`. The app binary is `meerkat`, and naming them the same breaks
  `cargo build`.
- The simulator's Panasonic SET is idempotent: a repeated SET must not re-arm the pending change
  after the confirm has applied it, or readdress tests flake.
- Commands that take an `AppHandle` are generic over `R: Runtime`, so the mock-runtime test can
  drive them. Keep them that way.
- The app raises its file-descriptor limit at start. A /22 sweep holds a socket per in-flight
  probe per driver, and macOS starts apps at 256.
- **Nothing has been run against a real camera.** Treat every driver as written from documents
  until a bench test says otherwise. Read before you write, and never write to a camera on a
  live show.
