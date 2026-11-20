//! The driver trait: everything a vendor module implements.
//!
//! A driver can do any subset of four things, and says which in
//! [`DriverInfo`] and the [`Caps`](crate::Caps) it puts on each camera:
//!
//! - **discover** — broadcast or multicast on every scan leg (one leg per
//!   chosen interface, plus one for directed broadcasts to routed subnets) and
//!   report what answers within `ctx.listen`;
//! - **probe** — ask one address whether it is one of ours (how routed ranges
//!   are swept); `sweep` defaults to probing each address with bounded
//!   concurrency, and a UDP driver can override it to blast the whole range;
//! - **read_network / write_network** — the IP settings;
//! - **web_url** — where the camera's own web UI is.
//!
//! Drivers never talk to the UI or to each other. They report cameras and log
//! lines through the context, and the engine merges and forwards them. To add
//! a vendor, see `docs/drivers.md`.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::http::{Http, HttpError};
use crate::model::{Camera, Credentials, Method, NetConfig, Sighting};
use crate::net::LocalIface;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("this camera's driver cannot {0}")]
    Unsupported(&'static str),
    #[error("this camera needs a username and password")]
    NeedsAuth,
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Protocol(String),
    #[error("no reply from the camera")]
    Timeout,
    #[error("{0}")]
    Unreachable(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T, E = DriverError> = std::result::Result<T, E>;

/// Default login for a vendor, offered in the UI and never sent unasked.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultLogin {
    pub username: &'static str,
    pub password: &'static str,
}

/// What a driver is, for the UI's driver list and for merging.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverInfo {
    pub id: &'static str,
    pub vendor: &'static str,
    /// The protocol, as the vendor's own tool calls it.
    pub protocol: &'static str,
    /// Merge priority: when two drivers see one camera, the higher one owns
    /// it. Vendor drivers 50+, ONVIF 30, mDNS 10.
    pub priority: u8,
    /// How it discovers, one line each, for the UI.
    pub discovery: Vec<&'static str>,
    /// How it probes a single address in a routed sweep, if it can.
    pub sweep: Option<&'static str>,
    pub default_login: Option<DefaultLogin>,
    pub notes: Vec<&'static str>,
    pub enabled_by_default: bool,
    /// The vendor's own tool for a camera nothing else can reach, if there is
    /// one.
    pub recovery: Option<Recovery>,
}

/// A vendor's tool for getting a camera back when nothing can reach it — a
/// network reset to factory settings, say (BirdDog Neuralyzer). Meerkat cannot
/// do what such a tool does when its protocol is not published, so it points
/// at the tool, then watches for the camera to come back.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    /// The tool's name, as the vendor calls it.
    pub tool: &'static str,
    /// The vendor's own download page.
    pub url: &'static str,
    /// Which of the vendor's cameras it is for.
    pub models: &'static str,
    /// What it does, in one sentence.
    pub does: &'static str,
    pub steps: Vec<&'static str>,
    /// The camera's network settings afterwards.
    pub after: &'static str,
    /// Where the camera answers when no DHCP server gives it an address:
    /// probed while watching for it.
    pub fallback: Option<Ipv4Addr>,
}

/// One way out for a discovery broadcast: a socket bound to `bind` (and
/// pinned to interface `index`), sending to each of `dests`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leg {
    /// Interface name, or `routed` for the directed-broadcast leg.
    pub iface: String,
    pub index: Option<u32>,
    pub bind: Ipv4Addr,
    pub dests: Vec<Ipv4Addr>,
}

impl Leg {
    /// The legs for a set of interfaces: each sends to the limited broadcast
    /// and its own directed broadcast; `routed` adds a leg for the directed
    /// broadcasts of routed subnets the user listed.
    pub fn for_ifaces(ifaces: &[LocalIface], routed: &[Ipv4Addr]) -> Vec<Leg> {
        let mut legs: Vec<Leg> = ifaces
            .iter()
            .map(|i| {
                let mut dests = vec![Ipv4Addr::BROADCAST];
                if let Some(b) = i.broadcast {
                    dests.push(b);
                }
                Leg { iface: i.name.clone(), index: i.index, bind: i.ip, dests }
            })
            .collect();
        if !routed.is_empty() {
            legs.push(Leg { iface: "routed".into(), index: None, bind: Ipv4Addr::UNSPECIFIED, dests: routed.to_vec() });
        }
        legs
    }
}

pub(crate) enum Msg {
    Found(Box<Camera>),
    Log { level: Level, driver: String, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// Port overrides keyed `"<driver>.<what>"`. Real scans leave it empty; tests
/// and the simulator move every protocol off its well-known port.
pub type PortMap = HashMap<String, u16>;

/// What a driver gets for one scan.
pub struct ScanCtx {
    pub driver: &'static str,
    /// The interfaces chosen for this scan (usable ones only).
    pub ifaces: Vec<LocalIface>,
    pub legs: Vec<Leg>,
    /// How long broadcast discovery listens for answers.
    pub listen: Duration,
    /// Per-address timeout in a sweep.
    pub probe_timeout: Duration,
    /// How many sweep probes this driver may have in flight.
    pub concurrency: usize,
    pub ports: PortMap,
    pub http: Http,
    pub cancel: CancellationToken,
    pub(crate) tx: mpsc::UnboundedSender<Msg>,
}

impl ScanCtx {
    /// Report a camera. `via` is the interface or sweep range it came from.
    pub fn found(&self, mut cam: Camera, method: Method, via: &str) {
        cam.seen = vec![Sighting { driver: self.driver.to_string(), method, via: via.to_string() }];
        let _ = self.tx.send(Msg::Found(Box::new(cam)));
    }

    pub fn log(&self, level: Level, message: impl Into<String>) {
        let _ = self.tx.send(Msg::Log { level, driver: self.driver.to_string(), message: message.into() });
    }

    pub fn port(&self, what: &str, default: u16) -> u16 {
        port(&self.ports, self.driver, what, default)
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The socket to send a datagram for `ip:port` to (the sandbox's route,
    /// or the address itself).
    pub fn to(&self, ip: Ipv4Addr, port: u16) -> SocketAddrV4 {
        self.http.dest(SocketAddrV4::new(ip, port))
    }

    /// Sleep until the listen window closes or the scan is cancelled.
    pub async fn listen_window(&self) {
        tokio::select! {
            _ = tokio::time::sleep(self.listen) => {}
            _ = self.cancel.cancelled() => {}
        }
    }

    /// A ScanCtx wired to a channel, for driver tests outside the engine.
    pub fn for_test(driver: &'static str, legs: Vec<Leg>, ports: PortMap, listen: Duration) -> (ScanCtx, TestSink) {
        let (tx, rx) = mpsc::unbounded_channel();
        let ctx = ScanCtx {
            driver,
            ifaces: Vec::new(),
            legs,
            listen,
            probe_timeout: Duration::from_millis(800),
            concurrency: 32,
            ports,
            http: Http::new(Duration::from_millis(1500)),
            cancel: CancellationToken::new(),
            tx,
        };
        (ctx, TestSink(rx))
    }
}

/// The receiving end of [`ScanCtx::for_test`].
pub struct TestSink(mpsc::UnboundedReceiver<Msg>);

impl TestSink {
    /// Every camera reported so far, and the log lines.
    pub fn drain(&mut self) -> (Vec<Camera>, Vec<String>) {
        let (mut cams, mut logs) = (Vec::new(), Vec::new());
        while let Ok(m) = self.0.try_recv() {
            match m {
                Msg::Found(c) => cams.push(*c),
                Msg::Log { level, message, .. } => logs.push(format!("{level:?}: {message}")),
            }
        }
        (cams, logs)
    }
}

/// What a driver gets for a read or write on one camera.
pub struct OpCtx {
    pub creds: Option<Credentials>,
    /// Legs to use for a layer-2 write: the interfaces the camera was heard
    /// on, or every chosen interface if none is known.
    pub legs: Vec<Leg>,
    pub ifaces: Vec<LocalIface>,
    pub ports: PortMap,
    pub http: Http,
    pub timeout: Duration,
    pub driver: &'static str,
}

impl OpCtx {
    pub fn port(&self, what: &str, default: u16) -> u16 {
        port(&self.ports, self.driver, what, default)
    }

    /// See [`ScanCtx::to`].
    pub fn to(&self, ip: Ipv4Addr, port: u16) -> SocketAddrV4 {
        self.http.dest(SocketAddrV4::new(ip, port))
    }

    pub fn creds(&self) -> Result<&Credentials> {
        self.creds.as_ref().filter(|c| !c.username.is_empty()).ok_or(DriverError::NeedsAuth)
    }

    pub fn for_test(driver: &'static str, legs: Vec<Leg>, ports: PortMap, creds: Option<Credentials>) -> OpCtx {
        OpCtx { creds, legs, ifaces: Vec::new(), ports, http: Http::new(Duration::from_millis(1500)), timeout: Duration::from_millis(1500), driver }
    }
}

fn port(ports: &PortMap, driver: &str, what: &str, default: u16) -> u16 {
    ports.get(&format!("{driver}.{what}")).copied().unwrap_or(default)
}

/// What a write did, so the engine knows when and where to look for the
/// camera again.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub message: String,
    /// The camera restarts its network (or itself) to apply the change.
    pub restarts: bool,
    /// Where it should answer next; None after a switch to DHCP.
    pub expect_ip: Option<Ipv4Addr>,
    /// How long to wait before looking for it there.
    pub settle_ms: u64,
}

#[async_trait]
pub trait Driver: Send + Sync + 'static {
    fn info(&self) -> DriverInfo;

    /// Broadcast/multicast discovery over `ctx.legs`, for `ctx.listen`.
    async fn discover(&self, _ctx: &ScanCtx) -> Result<()> {
        Ok(())
    }

    /// Is `ip` one of ours? Used by routed sweeps and by "add by address".
    async fn probe(&self, _ctx: &ScanCtx, _ip: Ipv4Addr) -> Result<Option<Camera>> {
        Ok(None)
    }

    /// Whether this driver has a unicast probe; the engine skips `sweep` for
    /// drivers that don't.
    fn can_probe(&self) -> bool {
        false
    }

    /// Probe every target. The default probes them `ctx.concurrency` at a
    /// time; a driver whose probe is one UDP datagram can do better.
    async fn sweep(&self, ctx: &ScanCtx, targets: &[Ipv4Addr], via: &str) -> Result<()> {
        futures::stream::iter(targets.iter().copied())
            .for_each_concurrent(ctx.concurrency.max(1), |ip| async move {
                if ctx.cancelled() {
                    return;
                }
                match tokio::time::timeout(ctx.probe_timeout * 3, self.probe(ctx, ip)).await {
                    Ok(Ok(Some(cam))) => ctx.found(cam, Method::Unicast, via),
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(DriverError::Http(HttpError::Transport(_)))) | Ok(Err(DriverError::Timeout)) => {}
                    Ok(Err(e)) => ctx.log(Level::Debug, format!("{ip}: {e}")),
                }
            })
            .await;
        Ok(())
    }

    async fn read_network(&self, _op: &OpCtx, _cam: &Camera) -> Result<NetConfig> {
        Err(DriverError::Unsupported("read network settings"))
    }

    async fn write_network(&self, _op: &OpCtx, _cam: &Camera, _cfg: &NetConfig) -> Result<WriteOutcome> {
        Err(DriverError::Unsupported("change network settings"))
    }

    /// The built-in web UI. The default is `http://<ip>:<web_port or 80>/`.
    fn web_url(&self, cam: &Camera) -> Option<String> {
        Some(match cam.web_port {
            Some(p) if p != 80 => format!("http://{}:{p}/", cam.ip),
            _ => format!("http://{}/", cam.ip),
        })
    }
}
