//! Runs scans, reads and writes over the registered drivers.
//!
//! A scan starts every enabled driver at once: each broadcasts on every leg
//! and, when routed ranges were given and it has a unicast probe, sweeps them.
//! Reports stream back through one channel, are merged by the registry, and go
//! out as [`Event`]s the moment they land — the UI fills in while the scan runs.

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::alias::{self, Alias, AliasBackend, NoAliases, SystemAliases};
use crate::catalog::Catalog;
use crate::driver::{Driver, DriverInfo, Leg, Level, Msg, OpCtx, PortMap, ScanCtx, WriteOutcome};
use crate::http::{Http, Route};
use crate::model::{Camera, Credentials, Method, NetConfig, Reach};
use crate::net::{self, LocalIface, Range};
use crate::registry::Registry;

/// Drivers at or above this priority are vendor drivers (they own what they
/// find); below it, generic ones whose finds the vendors are asked to confirm.
pub const VENDOR_PRIORITY: u8 = 50;

/// What to scan. Every field has a default, so `{}` from the UI is a full
/// broadcast scan on every usable interface.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanRequest {
    /// Interface names to broadcast on; empty means every usable one.
    pub ifaces: Vec<String>,
    /// Broadcast discovery at all (off for "add by address").
    pub broadcast: bool,
    /// Routed ranges to sweep by unicast: `10.20.0.0/24`, `10.20.0.10-60`, …
    pub ranges: Vec<String>,
    /// Also send each driver's broadcast to the directed broadcast of every
    /// subnet in `ranges`.
    pub directed: bool,
    /// Driver ids to run; empty means each driver's default.
    pub drivers: Vec<String>,
    pub listen_ms: u64,
    pub probe_timeout_ms: u64,
    /// Refuse a sweep bigger than this many hosts.
    pub max_hosts: u64,
}

impl Default for ScanRequest {
    fn default() -> Self {
        ScanRequest {
            ifaces: Vec::new(),
            broadcast: true,
            ranges: Vec::new(),
            directed: true,
            drivers: Vec::new(),
            listen_ms: 2500,
            probe_timeout_ms: 800,
            max_hosts: 4096,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub cameras: usize,
    pub new: usize,
    pub hosts_swept: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

/// What a scan (or a verify) tells its caller as it goes.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Camera { camera: Box<Camera>, replaces: Vec<String> },
    Log { level: Level, driver: String, message: String },
    Done { summary: ScanSummary },
}

/// A stand-in network for tests and the demo. Scans see `ifaces` instead of
/// the real interfaces, every socket binds to `bind` (loopback), protocols
/// move to the ports in `ports`, and `route` decides where each datagram and
/// request really goes — the simulator points camera addresses at its
/// listeners and everything else at a closed port, so nothing leaves the
/// machine.
#[derive(Clone)]
pub struct Sandbox {
    pub ifaces: Vec<LocalIface>,
    pub bind: Ipv4Addr,
    pub ports: PortMap,
    pub route: Route,
}

pub struct Engine {
    drivers: Vec<Arc<dyn Driver>>,
    registry: Mutex<Registry>,
    /// Every interface at the last scan, loopback included, for reach.
    ifaces: Mutex<Vec<LocalIface>>,
    /// The interfaces chosen at the last scan, for writes that need legs.
    chosen: Mutex<Vec<LocalIface>>,
    running: Mutex<Option<CancellationToken>>,
    sandbox: Option<Sandbox>,
    /// Temporary addresses this engine added, to reach cameras on the wrong
    /// subnet (see [`crate::alias`]). Removed by [`Engine::remove_aliases`].
    aliases: Mutex<Vec<Alias>>,
    alias_backend: Arc<dyn AliasBackend>,
    /// Factory logins and firmware pages, matched to each camera as it lands.
    catalog: Catalog,
}

impl Engine {
    pub fn new(drivers: Vec<Arc<dyn Driver>>, catalog: Catalog) -> Self {
        Engine {
            drivers,
            catalog,
            registry: Mutex::new(Registry::default()),
            ifaces: Mutex::new(Vec::new()),
            chosen: Mutex::new(Vec::new()),
            running: Mutex::new(None),
            sandbox: None,
            aliases: Mutex::new(Vec::new()),
            alias_backend: Arc::new(SystemAliases),
        }
    }

    /// An engine that only ever talks to a simulator. Its temporary addresses
    /// are pretend too: they join the sandbox's interface list and nothing
    /// else.
    pub fn sandboxed(drivers: Vec<Arc<dyn Driver>>, catalog: Catalog, sandbox: Sandbox) -> Self {
        Engine { sandbox: Some(sandbox), alias_backend: Arc::new(NoAliases), ..Engine::new(drivers, catalog) }
    }

    /// The catalogue's tables, for the UI's reference list.
    pub fn catalog(&self) -> Catalog {
        self.catalog
    }

    pub fn is_sandboxed(&self) -> bool {
        self.sandbox.is_some()
    }

    fn ports(&self) -> PortMap {
        self.sandbox.as_ref().map(|s| s.ports.clone()).unwrap_or_default()
    }

    fn http(&self, timeout: Duration) -> Http {
        Http::new(timeout).with_route(self.sandbox.as_ref().map(|s| s.route.clone()))
    }

    fn legs(&self, ifaces: &[LocalIface], directed: &[Ipv4Addr]) -> Vec<Leg> {
        let mut legs = Leg::for_ifaces(ifaces, directed);
        if let Some(s) = &self.sandbox {
            for l in &mut legs {
                l.bind = s.bind;
                l.index = None;
            }
        }
        legs
    }

    /// A camera's web UI address as a browser should open it (in the demo,
    /// the simulator's page on loopback).
    pub fn browser_url(&self, url: &str) -> String {
        self.http(Duration::from_secs(1)).url(url)
    }

    pub fn drivers(&self) -> Vec<DriverInfo> {
        self.drivers.iter().map(|d| d.info()).collect()
    }

    /// This computer's addresses (the simulator's, in a sandbox), temporary
    /// ones included.
    pub fn interfaces(&self) -> Result<Vec<LocalIface>, String> {
        match &self.sandbox {
            Some(s) => {
                let mut all = s.ifaces.clone();
                all.extend(self.aliases.lock().unwrap().iter().map(Alias::as_iface));
                Ok(all)
            }
            None => net::interfaces().map_err(|e| format!("could not list network interfaces: {e}")),
        }
    }

    fn is_alias(&self, i: &LocalIface) -> bool {
        self.aliases.lock().unwrap().iter().any(|a| a.is(i))
    }

    pub fn aliases(&self) -> Vec<Alias> {
        self.aliases.lock().unwrap().clone()
    }

    /// The temporary address that would put this computer on `id`'s subnet.
    pub fn plan_alias(&self, id: &str) -> Result<Alias, String> {
        let cam = self.camera(id).ok_or("that camera is no longer in the list; scan again")?;
        let all = self.interfaces()?;
        let taken: Vec<Ipv4Addr> = self.cameras().iter().flat_map(|c| [Some(c.ip), c.net.as_ref().and_then(|n| n.gateway)]).flatten().collect();
        alias::plan(&cam, &all, &taken)
    }

    /// Add a temporary address (the system asks for an admin password), then
    /// work out every camera's reach again. Returns the cameras as they are
    /// now; scan again to have the vendor drivers confirm what became
    /// reachable.
    pub async fn add_alias(&self, a: Alias) -> Result<Vec<Camera>, String> {
        if self.is_running() {
            return Err("stop the scan first".into());
        }
        let all = self.interfaces()?;
        alias::check(&a, &all)?;
        if let Some(c) = self.cameras().into_iter().find(|c| c.ip == a.ip) {
            return Err(format!("{} is in use by {}", a.ip, c.name.or(c.model).unwrap_or(c.vendor)));
        }
        let a = Alias { index: all.iter().find(|i| i.name == a.iface).and_then(|i| i.index), ..a };
        self.alias_backend.add(std::slice::from_ref(&a)).await?;
        self.aliases.lock().unwrap().push(a);
        self.refresh_reach()
    }

    /// Remove temporary addresses: the ones listed, or every one when `only`
    /// is None (as the app quits). One admin prompt covers them all.
    pub async fn remove_aliases(&self, only: Option<&[Alias]>) -> Result<Vec<Camera>, String> {
        let list: Vec<Alias> = self.aliases().into_iter().filter(|a| only.is_none_or(|o| o.iter().any(|x| x.iface == a.iface && x.ip == a.ip))).collect();
        if list.is_empty() {
            return self.refresh_reach();
        }
        // One the system already dropped (the cable came out, say) would make
        // the removal fail; it is gone either way.
        let up = if self.sandbox.is_some() { None } else { net::interfaces().ok() };
        let live: Vec<Alias> = list.iter().filter(|a| up.as_ref().is_none_or(|u| u.iter().any(|i| a.is(i)))).cloned().collect();
        self.alias_backend.remove(&live).await?;
        self.aliases.lock().unwrap().retain(|a| !list.contains(a));
        self.refresh_reach()
    }

    /// Recompute every camera's reach and web UI address against the
    /// interfaces as they are now.
    pub fn refresh_reach(&self) -> Result<Vec<Camera>, String> {
        let all = self.interfaces()?;
        *self.ifaces.lock().unwrap() = all.clone();
        let mut reg = self.registry.lock().unwrap();
        for id in reg.all().into_iter().map(|c| c.id) {
            if let Some(c) = reg.get_mut(&id) {
                c.reach = reach(c, &all);
                c.web_url = self.driver(&c.driver).and_then(|d| d.web_url(c));
            }
        }
        Ok(reg.all())
    }

    pub fn cameras(&self) -> Vec<Camera> {
        self.registry.lock().unwrap().all()
    }

    pub fn camera(&self, id: &str) -> Option<Camera> {
        self.registry.lock().unwrap().get(id).cloned()
    }

    pub fn clear(&self) {
        self.registry.lock().unwrap().clear();
    }

    pub fn forget(&self, id: &str) {
        self.registry.lock().unwrap().remove(id);
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// Stop the scan in progress, if any. Drivers return at their next await.
    pub fn cancel(&self) {
        if let Some(t) = self.running.lock().unwrap().as_ref() {
            t.cancel();
        }
    }

    fn driver(&self, id: &str) -> Option<Arc<dyn Driver>> {
        self.drivers.iter().find(|d| d.info().id == id).cloned()
    }

    fn chosen_drivers(&self, ids: &[String]) -> Vec<Arc<dyn Driver>> {
        self.drivers
            .iter()
            .filter(|d| {
                let i = d.info();
                if ids.is_empty() {
                    i.enabled_by_default
                } else {
                    ids.iter().any(|x| x == i.id)
                }
            })
            .cloned()
            .collect()
    }

    /// Run one scan to completion, calling `emit` for every merged camera and
    /// log line as they arrive. Only one scan runs at a time.
    pub async fn scan(&self, req: ScanRequest, emit: impl Fn(Event) + Send + Sync) -> Result<ScanSummary, String> {
        let started = Instant::now();
        let all = self.interfaces()?;
        let chosen: Vec<LocalIface> = if req.broadcast {
            // A temporary address only exists to reach cameras by unicast: a
            // second set of broadcasts out of the same wire would send every
            // layer-2 write twice.
            all.iter()
                .filter(|i| !self.is_alias(i))
                .filter(|i| if req.ifaces.is_empty() { i.usable() } else { req.ifaces.contains(&i.name) })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let mut ranges = Vec::new();
        for r in &req.ranges {
            if !r.trim().is_empty() {
                ranges.push(Range::parse(r)?);
            }
        }
        let hosts: u64 = ranges.iter().map(Range::len).sum();
        if hosts > req.max_hosts {
            return Err(format!("those ranges hold {hosts} addresses; the limit is {} — sweep a smaller range", req.max_hosts));
        }
        let own: HashSet<Ipv4Addr> = all.iter().map(|i| i.ip).collect();
        let directed: Vec<Ipv4Addr> = if req.directed && req.broadcast {
            ranges
                .iter()
                .filter_map(|r| r.subnet)
                .filter(|(n, p)| *p < 31 && !chosen.iter().any(|i| i.network() == *n && i.prefix == *p))
                .map(|(n, p)| net::broadcast(n, p))
                .collect()
        } else {
            Vec::new()
        };
        let legs = self.legs(&chosen, &directed);
        let drivers = self.chosen_drivers(&req.drivers);
        if drivers.is_empty() {
            return Err("no drivers are enabled".into());
        }
        if legs.is_empty() && ranges.is_empty() {
            return Err("nothing to scan: no interface is selected and no range was given".into());
        }

        let cancel = CancellationToken::new();
        {
            let mut r = self.running.lock().unwrap();
            if r.is_some() {
                return Err("a scan is already running".into());
            }
            *r = Some(cancel.clone());
        }
        *self.ifaces.lock().unwrap() = all.clone();
        *self.chosen.lock().unwrap() = chosen.clone();

        let names: Vec<String> = chosen.iter().map(|i| format!("{} ({})", i.name, i.cidr())).collect();
        if !names.is_empty() {
            emit(Event::Log { level: Level::Info, driver: "scan".into(), message: format!("broadcasting on {}", names.join(", ")) });
        }
        for r in &ranges {
            emit(Event::Log { level: Level::Info, driver: "scan".into(), message: format!("sweeping {} ({} addresses)", r.label, r.len()) });
        }
        if !directed.is_empty() {
            let d: Vec<String> = directed.iter().map(|d| d.to_string()).collect();
            emit(Event::Log { level: Level::Info, driver: "scan".into(), message: format!("directed broadcast to {}", d.join(", ")) });
        }

        let (tx, rx) = mpsc::unbounded_channel::<Msg>();
        let http = self.http(Duration::from_millis(req.probe_timeout_ms.max(200) * 2));
        let mut tasks = Vec::new();
        for d in drivers.clone() {
            let info = d.info();
            let ctx = ScanCtx {
                driver: info.id,
                ifaces: chosen.clone(),
                legs: legs.clone(),
                listen: Duration::from_millis(req.listen_ms),
                probe_timeout: Duration::from_millis(req.probe_timeout_ms),
                concurrency: 48,
                ports: self.ports(),
                http: http.clone(),
                cancel: cancel.clone(),
                tx: tx.clone(),
            };
            let ranges = ranges.clone();
            let own = own.clone();
            let broadcast = !legs.is_empty();
            tasks.push(tokio::spawn(async move {
                if broadcast {
                    if let Err(e) = d.discover(&ctx).await {
                        ctx.log(Level::Warn, format!("discovery failed: {e}"));
                    }
                }
                if d.can_probe() {
                    for r in &ranges {
                        if ctx.cancelled() {
                            break;
                        }
                        let targets: Vec<Ipv4Addr> = r.iter().filter(|ip| !own.contains(ip)).collect();
                        if let Err(e) = d.sweep(&ctx, &targets, &r.label).await {
                            ctx.log(Level::Warn, format!("sweep of {} failed: {e}", r.label));
                        }
                    }
                }
            }));
        }
        drop(tx);

        let before: HashSet<String> = self.registry.lock().unwrap().all().into_iter().map(|c| c.id).collect();
        let mut found = HashSet::new();
        self.pump(rx, &all, &emit, |c| {
            found.insert(c.id.clone());
        })
        .await;
        for t in tasks {
            let _ = t.await;
        }

        // Phase 2: a camera only mDNS heard (an NDI source) goes to the probe
        // of the vendor driver its name points at — or every vendor driver,
        // when the name says nothing — so a BirdDog announced over NDI is
        // confirmed, and made writable, over its own API.
        if !cancel.is_cancelled() {
            let targets: Vec<(Ipv4Addr, String, String)> = {
                let reg = self.registry.lock().unwrap();
                found
                    .iter()
                    .filter_map(|id| reg.get(id))
                    .filter(|c| self.driver(&c.driver).map(|d| d.info().priority).unwrap_or(0) < VENDOR_PRIORITY)
                    .filter(|c| !c.seen.is_empty() && c.seen.iter().all(|s| s.method == Method::Mdns))
                    .filter(|c| matches!(c.reach, Reach::Local { .. } | Reach::Routed))
                    .map(|c| (c.ip, c.seen[0].via.clone(), c.vendor.clone()))
                    .collect()
            };
            let vendors: Vec<Arc<dyn Driver>> = drivers.iter().filter(|d| d.can_probe() && d.info().priority >= VENDOR_PRIORITY).cloned().collect();
            if !targets.is_empty() && !vendors.is_empty() {
                let (tx, rx) = mpsc::unbounded_channel::<Msg>();
                let mut tasks = Vec::new();
                let known: Vec<&'static str> = vendors.iter().map(|d| d.info().vendor).collect();
                for d in vendors {
                    let mine = d.info().vendor;
                    let targets: Vec<(Ipv4Addr, String)> = targets
                        .iter()
                        .filter(|(_, _, v)| v == mine || !known.contains(&v.as_str()))
                        .map(|(ip, via, _)| (*ip, via.clone()))
                        .collect();
                    if targets.is_empty() {
                        continue;
                    }
                    let ctx = ScanCtx {
                        driver: d.info().id,
                        ifaces: chosen.clone(),
                        legs: Vec::new(),
                        listen: Duration::from_millis(req.listen_ms),
                        probe_timeout: Duration::from_millis(req.probe_timeout_ms),
                        concurrency: 8,
                        ports: self.ports(),
                        http: http.clone(),
                        cancel: cancel.clone(),
                        tx: tx.clone(),
                    };
                    tasks.push(tokio::spawn(async move {
                        for (ip, via) in targets {
                            if ctx.cancelled() {
                                break;
                            }
                            if let Ok(Ok(Some(c))) = tokio::time::timeout(ctx.probe_timeout * 4, d.probe(&ctx, ip)).await {
                                ctx.found(c, Method::Unicast, &via);
                            }
                        }
                    }));
                }
                drop(tx);
                self.pump(rx, &all, &emit, |c| {
                    found.insert(c.id.clone());
                })
                .await;
                for t in tasks {
                    let _ = t.await;
                }
            }
        }
        let cancelled = cancel.is_cancelled();
        *self.running.lock().unwrap() = None;
        // Rows folded into another (an `ip:` row that turned out to have a MAC)
        // were found too, but are not cameras of their own.
        {
            let reg = self.registry.lock().unwrap();
            found.retain(|id| reg.get(id).is_some());
        }
        let summary = ScanSummary {
            cameras: found.len(),
            new: found.iter().filter(|id| !before.contains(*id)).count(),
            hosts_swept: hosts,
            elapsed_ms: started.elapsed().as_millis() as u64,
            cancelled,
        };
        emit(Event::Done { summary: summary.clone() });
        Ok(summary)
    }

    /// Drain driver messages into the registry until every sender is gone.
    async fn pump(&self, mut rx: mpsc::UnboundedReceiver<Msg>, all: &[LocalIface], emit: &(impl Fn(Event) + Send + Sync), mut seen: impl FnMut(&Camera)) {
        while let Some(m) = rx.recv().await {
            match m {
                Msg::Log { level, driver, message } => emit(Event::Log { level, driver, message }),
                Msg::Found(cam) => {
                    let ev = self.absorb(*cam, all);
                    if let Event::Camera { camera, .. } = &ev {
                        seen(camera);
                    }
                    emit(ev);
                }
            }
        }
    }

    fn absorb(&self, mut cam: Camera, all: &[LocalIface]) -> Event {
        let prio = self.driver(&cam.driver).map(|d| d.info().priority).unwrap_or(0);
        cam.last_seen = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let up = {
            let mut reg = self.registry.lock().unwrap();
            reg.upsert(cam, prio)
        };
        let mut merged = up.camera;
        merged.reach = reach(&merged, all);
        merged.web_url = self.driver(&merged.driver).and_then(|d| d.web_url(&merged));
        // The vendor and model may only now be known (ONVIF names a camera
        // NDI found first), so this is worked out again on every sighting.
        merged.catalog = self.catalog.about(&merged);
        {
            let mut reg = self.registry.lock().unwrap();
            if let Some(c) = reg_get_mut(&mut reg, &merged.id) {
                c.reach = merged.reach.clone();
                c.web_url = merged.web_url.clone();
                c.catalog = merged.catalog.clone();
            }
        }
        Event::Camera { camera: Box::new(merged), replaces: up.replaces }
    }

    fn op_ctx(&self, cam: &Camera, creds: Option<Credentials>, driver: &'static str) -> OpCtx {
        let chosen = self.chosen.lock().unwrap().clone();
        let heard_on: Vec<&str> = cam.seen.iter().filter(|s| s.method != Method::Unicast).map(|s| s.via.as_str()).collect();
        let on: Vec<LocalIface> = chosen.iter().filter(|i| heard_on.contains(&i.name.as_str())).cloned().collect();
        let legs = self.legs(if on.is_empty() { &chosen } else { &on }, &[]);
        OpCtx { creds, legs, ifaces: self.ifaces.lock().unwrap().clone(), ports: self.ports(), http: self.http(Duration::from_secs(5)), timeout: Duration::from_secs(4), driver }
    }

    pub async fn read_network(&self, id: &str, creds: Option<Credentials>) -> Result<NetConfig, String> {
        let cam = self.camera(id).ok_or("that camera is no longer in the list; scan again")?;
        let d = self.driver(&cam.driver).ok_or("its driver is not loaded")?;
        if !cam.caps.read_network {
            return Err(format!("the {} driver cannot read this camera's network settings", d.info().vendor));
        }
        let op = self.op_ctx(&cam, creds, d.info().id);
        let cfg = d.read_network(&op, &cam).await.map_err(|e| e.to_string())?;
        if let Some(c) = reg_get_mut(&mut self.registry.lock().unwrap(), id) {
            c.net = Some(cfg.clone());
        }
        Ok(cfg)
    }

    /// Validate and send new settings. Does not wait for the camera to come
    /// back — call [`Engine::verify`] with the outcome for that.
    pub async fn write_network(&self, id: &str, cfg: NetConfig, creds: Option<Credentials>) -> Result<WriteOutcome, String> {
        let cam = self.camera(id).ok_or("that camera is no longer in the list; scan again")?;
        let d = self.driver(&cam.driver).ok_or("its driver is not loaded")?;
        if !cam.caps.write_network {
            return Err(format!("the {} driver cannot change this camera's network settings", d.info().vendor));
        }
        let problems = net::validate(&cfg);
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        if cfg.dhcp && !cam.caps.dhcp {
            return Err("this camera's driver cannot switch it to DHCP".into());
        }
        if cam.caps.write_by_mac && !cam.seen.iter().any(|s| s.driver == cam.driver && s.method == Method::Broadcast) {
            return Err(format!(
                "{} changes addresses by layer-2 broadcast, which cannot cross a router, and this camera was only reached through one — change it from its web UI, or scan from a computer on its network",
                d.info().protocol
            ));
        }
        if let Reach::Mismatch { iface } = &cam.reach {
            if !cam.caps.write_by_mac {
                return Err(format!(
                    "{} is outside {iface}'s subnet, and this driver changes settings over HTTP, which cannot reach it — add a temporary address on {iface} to reach it, or put this computer on the camera's subnet",
                    cam.ip
                ));
            }
        }
        if !cfg.dhcp {
            if let Some(other) = self.cameras().into_iter().find(|c| c.ip == cfg.ip && c.id != cam.id) {
                return Err(format!("{} is already in use by {}", cfg.ip, other.name.or(other.model).unwrap_or(other.vendor)));
            }
        }
        let op = self.op_ctx(&cam, creds, d.info().id);
        d.write_network(&op, &cam, &cfg).await.map_err(|e| e.to_string())
    }

    /// Look for a camera after a write: rediscover with its driver (and probe
    /// the expected address, if the driver can) until it answers or
    /// `timeout` runs out. Returns the camera as it is now.
    pub async fn verify(&self, id: &str, outcome: &WriteOutcome, timeout: Duration, emit: impl Fn(Event) + Send + Sync) -> Result<Option<Camera>, String> {
        let cam = self.camera(id).ok_or("that camera is no longer in the list")?;
        let d = self.driver(&cam.driver).ok_or("its driver is not loaded")?;
        let info = d.info();
        let deadline = Instant::now() + timeout;
        tokio::time::sleep(Duration::from_millis(outcome.settle_ms)).await;
        let all = self.interfaces()?;
        let chosen = self.chosen.lock().unwrap().clone();
        let legs = self.legs(&chosen, &[]);
        while Instant::now() < deadline {
            let (tx, rx) = mpsc::unbounded_channel::<Msg>();
            let ctx = ScanCtx {
                driver: info.id,
                ifaces: chosen.clone(),
                legs: legs.clone(),
                listen: Duration::from_millis(1500),
                probe_timeout: Duration::from_millis(800),
                concurrency: 4,
                ports: self.ports(),
                http: self.http(Duration::from_millis(1500)),
                cancel: CancellationToken::new(),
                tx,
            };
            let d2 = d.clone();
            let expect = outcome.expect_ip;
            let task = tokio::spawn(async move {
                if !ctx.legs.is_empty() {
                    let _ = d2.discover(&ctx).await;
                }
                if let (Some(ip), true) = (expect, d2.can_probe()) {
                    if let Ok(Some(c)) = d2.probe(&ctx, ip).await {
                        ctx.found(c, Method::Unicast, &ip.to_string());
                    }
                }
            });
            let mut hit: Option<Camera> = None;
            self.pump(rx, &all, &emit, |c| {
                let same = match (cam.mac, c.mac) {
                    (Some(a), Some(b)) => a == b,
                    _ => Some(c.ip) == outcome.expect_ip,
                };
                if same && (outcome.expect_ip.is_none() || Some(c.ip) == outcome.expect_ip) {
                    hit = Some(c.clone());
                }
            })
            .await;
            let _ = task.await;
            if hit.is_some() {
                return Ok(hit);
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        Ok(None)
    }
}

fn reg_get_mut<'a>(reg: &'a mut Registry, id: &str) -> Option<&'a mut Camera> {
    reg.get_mut(id)
}

/// Where a camera sits relative to this computer, from its address and the
/// ways it has been heard.
pub fn reach(cam: &Camera, all: &[LocalIface]) -> Reach {
    if let Some(i) = net::local_for(all, cam.ip) {
        return Reach::Local { iface: i.name.clone() };
    }
    // A probe from a swept range crossed a router. One sent to a camera an
    // interface heard (the vendor confirming an NDI find) says nothing about
    // routers: it carries that interface's name.
    if cam.seen.iter().any(|s| s.method == Method::Unicast && !all.iter().any(|i| i.name == s.via)) {
        return Reach::Routed;
    }
    match cam.seen.iter().find(|s| s.via != "routed") {
        Some(s) => Reach::Mismatch { iface: s.via.clone() },
        None => Reach::Unknown,
    }
}
