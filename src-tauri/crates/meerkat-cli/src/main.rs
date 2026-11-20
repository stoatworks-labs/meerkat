//! `meerkat-cli` — the engine from a terminal.
//!
//! ```text
//! meerkat-cli ifaces
//! meerkat-cli drivers
//! meerkat-cli catalog [brand] [--json]
//! meerkat-cli scan  [scope]
//! meerkat-cli set   <camera> --ip A [--prefix N | --mask M] [--gw G] [--dns D]... [--name N] [--user U --pass P] [scope] [--yes]
//! meerkat-cli set   <camera> --dhcp [--user U --pass P] [scope] [--yes]
//! meerkat-cli web   <camera> [scope]
//!
//! scope: [--iface NAME]... [--range R]... [--driver ID]... [--secs N] [--demo] [--json] [--verbose]
//! ```
//!
//! `<camera>` is a MAC address (any usual spelling), an IP address or a row id
//! from `scan --json`. `set` scans first, so it knows how the camera was
//! reached, then shows the change and sends it only with `--yes`. `--demo`
//! runs everything against the built-in simulator instead of the network.
//! `catalog` prints the factory logins and firmware links Meerkat knows, and
//! `scan` names the rows that apply to each camera; neither touches a camera.

use std::net::Ipv4Addr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use meerkat_core::driver::Level;
use meerkat_core::net::{in_subnet, mask_to_prefix, prefix_to_mask};
use meerkat_core::{Camera, Catalog, Credentials, Engine, Event, Login, Mac, NetConfig, Reach, ScanRequest, Source, SourceKind};

const USAGE: &str = "usage:
  meerkat-cli ifaces
  meerkat-cli drivers
  meerkat-cli catalog [brand] [--json]
  meerkat-cli scan  [scope]
  meerkat-cli set   <camera> --ip A [--prefix N | --mask M] [--gw G] [--dns D]... [--name N] [--user U --pass P] [scope] [--yes]
  meerkat-cli set   <camera> --dhcp [--user U --pass P] [scope] [--yes]
  meerkat-cli web   <camera> [scope]

scope: [--iface NAME]... [--range R]... [--driver ID]... [--secs N] [--demo] [--json] [--verbose]
<camera>: a MAC address, an IP address, or an id from `scan --json`
ranges: 10.20.0.0/24, 10.20.0.10-60, 10.20.0.10-10.20.1.40 or one address";

#[derive(Default)]
struct Args {
    cmd: String,
    target: Option<String>,
    ifaces: Vec<String>,
    ranges: Vec<String>,
    drivers: Vec<String>,
    secs: Option<f64>,
    demo: bool,
    json: bool,
    verbose: bool,
    yes: bool,
    ip: Option<Ipv4Addr>,
    prefix: Option<u8>,
    gw: Option<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    name: Option<String>,
    dhcp: bool,
    user: Option<String>,
    pass: Option<String>,
}

fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.into_iter();
    a.cmd = it.next().ok_or("no command")?;
    fn ip(v: String, flag: &str) -> Result<Ipv4Addr, String> {
        v.parse().map_err(|_| format!("{flag}: {v} is not an IPv4 address"))
    }
    while let Some(arg) = it.next() {
        let mut val = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--iface" | "-i" => a.ifaces.push(val("--iface")?),
            "--range" | "-r" => a.ranges.push(val("--range")?),
            "--driver" | "-d" => a.drivers.push(val("--driver")?),
            "--secs" => a.secs = Some(val("--secs")?.parse().map_err(|_| "--secs wants a number")?),
            "--demo" => a.demo = true,
            "--json" => a.json = true,
            "--verbose" | "-v" => a.verbose = true,
            "--yes" | "-y" => a.yes = true,
            "--ip" => a.ip = Some(ip(val("--ip")?, "--ip")?),
            "--prefix" => {
                let v = val("--prefix")?;
                a.prefix = Some(v.trim_start_matches('/').parse().map_err(|_| format!("--prefix: {v} is not a prefix length"))?);
            }
            "--mask" => {
                let v = val("--mask")?;
                a.prefix = Some(mask_to_prefix(ip(v.clone(), "--mask")?).ok_or(format!("--mask: {v} is not a contiguous netmask"))?);
            }
            "--gw" | "--gateway" => a.gw = Some(ip(val("--gw")?, "--gw")?),
            "--dns" => a.dns.push(ip(val("--dns")?, "--dns")?),
            "--name" => a.name = Some(val("--name")?),
            "--dhcp" => a.dhcp = true,
            "--user" => a.user = Some(val("--user")?),
            "--pass" | "--password" => a.pass = Some(val("--pass")?),
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if a.target.is_none() => a.target = Some(s.to_string()),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(argv) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("meerkat-cli: {e}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("meerkat-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(a: Args) -> Result<(), String> {
    // The simulator lives as long as the command when --demo is given.
    let (_sim, engine) = if a.demo {
        let sim = meerkat_sim::Sim::start(meerkat_sim::demo_cameras()).await.map_err(|e| format!("could not start the simulator: {e}"))?;
        let engine = Engine::sandboxed(meerkat_drivers::all(), meerkat_drivers::catalog(), sim.sandbox());
        (Some(sim), engine)
    } else {
        (None, Engine::new(meerkat_drivers::all(), meerkat_drivers::catalog()))
    };
    let engine = Arc::new(engine);
    match a.cmd.as_str() {
        "ifaces" | "interfaces" => ifaces(&engine, a.json),
        "drivers" => drivers(&engine, a.json),
        "catalog" | "logins" | "firmware" => catalog(engine.catalog(), a.target.as_deref(), a.json),
        "scan" => {
            let cams = scan(&engine, &a).await?;
            if a.json {
                println!("{}", serde_json::to_string_pretty(&cams).unwrap());
            } else {
                print_table(&cams);
            }
            Ok(())
        }
        "set" => set(&engine, &a).await,
        "web" => web(&engine, &a).await,
        c => Err(format!("unknown command {c}\n\n{USAGE}")),
    }
}

fn ifaces(engine: &Engine, json: bool) -> Result<(), String> {
    let list = engine.interfaces()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        return Ok(());
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|i| {
            let note = if i.loopback {
                "loopback"
            } else if i.link_local {
                "link-local"
            } else if i.p2p {
                "point-to-point"
            } else {
                ""
            };
            vec![i.name.clone(), i.cidr(), i.broadcast.map(|b| b.to_string()).unwrap_or_default(), if i.usable() { "yes".into() } else { format!("no ({note})") }]
        })
        .collect();
    table(&["IFACE", "ADDRESS", "BROADCAST", "SCANNED"], &rows);
    Ok(())
}

fn drivers(engine: &Engine, json: bool) -> Result<(), String> {
    let list = engine.drivers();
    if json {
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        return Ok(());
    }
    for d in list {
        println!("{} — {} ({}){}", d.id, d.vendor, d.protocol, if d.enabled_by_default { "" } else { " [off by default]" });
        for x in &d.discovery {
            println!("    finds by: {x}");
        }
        if let Some(s) = &d.sweep {
            println!("    sweeps:   {s}");
        }
        for n in &d.notes {
            println!("    note: {n}");
        }
    }
    Ok(())
}

fn source_text(s: &Source) -> String {
    let kind = match s.kind {
        SourceKind::Vendor => "",
        SourceKind::VendorExcerpt => " [excerpt only]",
        SourceKind::ThirdParty => " [not the maker]",
    };
    format!("{}{kind}\n        {}", s.title, s.url)
}

/// The tables, whole or for one brand (any case).
fn catalog(c: Catalog, brand: Option<&str>, json: bool) -> Result<(), String> {
    let want = |b: &str| brand.is_none_or(|w| b.eq_ignore_ascii_case(w));
    let logins: Vec<_> = c.logins.iter().filter(|r| want(r.brand)).collect();
    let firmware: Vec<_> = c.firmware.iter().filter(|r| want(r.brand)).collect();
    if logins.is_empty() && firmware.is_empty() {
        let mut brands: Vec<&str> = c.logins.iter().map(|r| r.brand).chain(c.firmware.iter().map(|r| r.brand)).collect();
        brands.sort_unstable();
        brands.dedup();
        return Err(format!("nothing for {}; the brands are {}", brand.unwrap_or("?"), brands.join(", ")));
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "logins": logins, "firmware": firmware })).unwrap());
        return Ok(());
    }
    println!("FACTORY LOGINS — Meerkat never tries these on a camera.\n");
    for r in &logins {
        let login = match r.login {
            Login::Factory { username: "", password } => format!("password {}", if password.is_empty() { "(blank)" } else { password }),
            Login::Factory { username, password } => format!("{username} / {}", if password.is_empty() { "(blank)" } else { password }),
            Login::Create { username: Some(u) } => format!("none: you set {u}'s password at the first login"),
            Login::Create { username: None } => "none: you create the account at the first login".into(),
        };
        println!("{} {} — {login}", r.brand, r.family);
        if let Some(f) = r.firmware {
            println!("    firmware: {f}");
        }
        if r.change_at_first_login {
            println!("    the camera makes you change it at the first login");
        }
        if let Some(l) = r.lockout {
            println!("    lockout: {l}");
        }
        for n in r.notes {
            println!("    note: {n}");
        }
        for s in r.sources {
            println!("    source: {}", source_text(s));
        }
        println!();
    }
    println!("FIRMWARE — from the maker; Meerkat does not supply or upload it.\n");
    for r in &firmware {
        println!("{} {}\n    downloads: {}", r.brand, r.family, r.downloads);
        if let Some(n) = r.downloads_note {
            println!("        {n}");
        }
        for h in r.how {
            println!("    how: {h}");
        }
        if let Some(p) = r.web_path {
            println!("    the camera's firmware page: http://<camera>{p}");
        }
        for w in r.warnings {
            println!("    warning: {w}");
        }
        if let Some(u) = r.upload {
            println!("    published upload: {u}");
        }
        for s in r.sources {
            println!("    source: {}", source_text(s));
        }
        println!();
    }
    Ok(())
}

async fn scan(engine: &Arc<Engine>, a: &Args) -> Result<Vec<Camera>, String> {
    let req = ScanRequest {
        ifaces: a.ifaces.clone(),
        ranges: a.ranges.clone(),
        drivers: a.drivers.clone(),
        listen_ms: a.secs.map(|s| (s * 1000.0) as u64).unwrap_or(2500),
        ..Default::default()
    };
    let verbose = a.verbose;
    let quiet = a.json;
    engine
        .scan(req, move |e| match e {
            Event::Log { level, driver, message } if !quiet && (verbose || !matches!(level, Level::Debug)) => eprintln!("  {driver}: {message}"),
            Event::Camera { camera, .. } if verbose && !quiet => eprintln!("  + {} {} at {}", camera.vendor, camera.model.as_deref().unwrap_or("?"), camera.ip),
            Event::Done { summary } if !quiet => eprintln!("  {} camera(s) in {:.1} s{}", summary.cameras, summary.elapsed_ms as f64 / 1000.0, if summary.cancelled { " (stopped)" } else { "" }),
            _ => {}
        })
        .await?;
    let mut cams = engine.cameras();
    cams.sort_by_key(|c| (c.ip.octets(), c.vendor.clone()));
    Ok(cams)
}

fn reach_text(r: &Reach) -> String {
    match r {
        Reach::Local { iface } => format!("local ({iface})"),
        Reach::Routed => "routed".into(),
        Reach::Mismatch { iface } => format!("WRONG SUBNET ({iface})"),
        Reach::Unknown => "?".into(),
    }
}

fn print_table(cams: &[Camera]) {
    if cams.is_empty() {
        println!("No cameras found.");
        return;
    }
    let rows: Vec<Vec<String>> = cams
        .iter()
        .map(|c| {
            let mut via: Vec<String> = c.seen.iter().map(|s| s.driver.clone()).collect();
            via.dedup();
            vec![
                c.vendor.clone(),
                c.model.clone().unwrap_or_default(),
                c.name.clone().unwrap_or_default(),
                c.net.as_ref().map(|n| format!("{}/{}", c.ip, n.prefix)).unwrap_or_else(|| c.ip.to_string()),
                reach_text(&c.reach),
                c.mac.map(|m| m.to_string()).unwrap_or_default(),
                c.firmware.clone().unwrap_or_default(),
                if c.caps.write_network { "yes".into() } else { "web UI".into() },
                via.join("+"),
            ]
        })
        .collect();
    table(&["VENDOR", "MODEL", "NAME", "ADDRESS", "REACH", "MAC", "FIRMWARE", "SET IP", "FOUND BY"], &rows);
}

fn table(head: &[&str], rows: &[Vec<String>]) {
    let mut w: Vec<usize> = head.iter().map(|h| h.len()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let line = |cells: Vec<String>| {
        let s: Vec<String> = cells.iter().enumerate().map(|(i, c)| format!("{c:<width$}", width = w[i])).collect();
        println!("{}", s.join("  ").trim_end());
    };
    line(head.iter().map(|h| h.to_string()).collect());
    for r in rows {
        line(r.clone());
    }
}

/// The camera `target` names: a MAC, an IP or a row id.
fn find(cams: &[Camera], target: &str) -> Result<Camera, String> {
    let hit: Vec<&Camera> = if let Ok(m) = target.parse::<Mac>() {
        cams.iter().filter(|c| c.mac == Some(m)).collect()
    } else if let Ok(ip) = target.parse::<Ipv4Addr>() {
        cams.iter().filter(|c| c.ip == ip).collect()
    } else {
        cams.iter().filter(|c| c.id == target).collect()
    };
    match hit.as_slice() {
        [c] => Ok((*c).clone()),
        [] => Err(format!("no camera matching {target} was found (scan with --range for cameras behind a router)")),
        _ => Err(format!("{target} matches {} cameras; name one by MAC", hit.len())),
    }
}

fn describe(c: &Camera) -> String {
    let mut s = format!("{} {}", c.vendor, c.model.as_deref().unwrap_or(""));
    if let Some(n) = &c.name {
        s.push_str(&format!(" \"{n}\""));
    }
    if let Some(m) = c.mac {
        s.push_str(&format!(" ({m})"));
    }
    s
}

fn net_text(n: &NetConfig) -> String {
    if n.dhcp {
        return "DHCP".into();
    }
    let mut s = format!("{}/{}", n.ip, n.prefix);
    if let Some(g) = n.gateway {
        s.push_str(&format!(" via {g}"));
    }
    if !n.dns.is_empty() {
        s.push_str(&format!(", DNS {}", n.dns.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(" ")));
    }
    if let Some(h) = &n.hostname {
        s.push_str(&format!(", name {h}"));
    }
    s
}

async fn set(engine: &Arc<Engine>, a: &Args) -> Result<(), String> {
    let target = a.target.as_deref().ok_or("set: which camera? Give its MAC or IP address")?;
    if a.ip.is_none() && !a.dhcp {
        return Err("set: give --ip (and --prefix or --mask, --gw) or --dhcp".into());
    }
    let cams = scan(engine, a).await?;
    let cam = find(&cams, target)?;
    let cur = cam.net.clone();
    let prefix = a.prefix.or(cur.as_ref().map(|n| n.prefix)).unwrap_or(24);
    let cfg = if a.dhcp {
        NetConfig { dhcp: true, ip: Ipv4Addr::UNSPECIFIED, prefix, gateway: None, dns: vec![], hostname: a.name.clone() }
    } else {
        let ip = a.ip.unwrap();
        // Keep the old gateway only if it still sits inside the new subnet.
        let gateway = a.gw.or_else(|| cur.as_ref().and_then(|n| n.gateway).filter(|g| in_subnet(*g, ip, prefix)));
        if a.gw.is_none() && gateway.is_none() {
            eprintln!("  note: no gateway (the old one is outside {ip}/{prefix}); give --gw to set one");
        }
        let dns = if a.dns.is_empty() { cur.as_ref().map(|n| n.dns.clone()).unwrap_or_default() } else { a.dns.clone() };
        NetConfig { dhcp: false, ip, prefix, gateway, dns, hostname: a.name.clone() }
    };
    let from = cur.as_ref().map(net_text).unwrap_or_else(|| cam.ip.to_string());
    println!("{}", describe(&cam));
    println!("  now:  {from}  [{}]", reach_text(&cam.reach));
    println!("  new:  {}  (mask {})", net_text(&cfg), prefix_to_mask(cfg.prefix));
    if !a.yes {
        println!("Dry run — add --yes to send it.");
        return Ok(());
    }
    let creds = match (&a.user, &a.pass) {
        (Some(u), p) => Some(Credentials { username: u.clone(), password: p.clone().unwrap_or_default() }),
        (None, Some(p)) => Some(Credentials { username: "admin".into(), password: p.clone() }),
        _ => None,
    };
    let out = engine.write_network(&cam.id, cfg, creds).await?;
    println!("  {}", out.message);
    if out.restarts {
        println!("  the camera restarts its network; looking for it…");
    }
    match engine.verify(&cam.id, &out, Duration::from_millis(out.settle_ms) + Duration::from_secs(45), |_| {}).await? {
        Some(c) => println!("  back at {} [{}]", c.ip, reach_text(&c.reach)),
        None => println!("  not heard back yet — it may still be restarting, or now be on a subnet this computer cannot hear; scan again in a minute"),
    }
    Ok(())
}

async fn web(engine: &Arc<Engine>, a: &Args) -> Result<(), String> {
    let target = a.target.as_deref().ok_or("web: which camera? Give its MAC or IP address")?;
    let cams = scan(engine, a).await?;
    let cam = find(&cams, target)?;
    let url = cam.web_url.clone().ok_or("this camera has no web UI address")?;
    let open = engine.browser_url(&url);
    if let Reach::Mismatch { iface } = &cam.reach {
        eprintln!("  note: {} is outside {iface}'s subnet, so the browser cannot reach it until this computer has an address on its subnet", cam.ip);
    }
    println!("{open}");
    let cmd = if cfg!(target_os = "macos") {
        ("open", vec![open.clone()])
    } else if cfg!(windows) {
        ("cmd", vec!["/C".into(), "start".into(), String::new(), open.clone()])
    } else {
        ("xdg-open", vec![open.clone()])
    };
    std::process::Command::new(cmd.0).args(&cmd.1).status().map_err(|e| format!("could not open a browser: {e}"))?;
    if a.demo {
        // The simulator serves the page only while this process lives.
        eprintln!("  (demo: serving the simulated page for 30 s)");
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    Ok(())
}
