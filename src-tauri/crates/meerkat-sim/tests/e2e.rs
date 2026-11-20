//! The whole stack against the simulator: engine → drivers → wire → fake
//! cameras → back, on loopback with ephemeral ports.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat_core::{Camera, Credentials, Engine, Event, Mac, NetConfig, Reach, ScanRequest};
use meerkat_sim::{demo_cameras, Sim};

async fn rig() -> (Sim, Engine) {
    let sim = Sim::start(demo_cameras()).await.expect("simulator");
    let engine = Engine::sandboxed(meerkat_drivers::all(), meerkat_drivers::catalog(), sim.sandbox());
    (sim, engine)
}

async fn scan(engine: &Engine) -> (Vec<Camera>, Vec<String>) {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let l = logs.clone();
    let req = ScanRequest { listen_ms: 700, probe_timeout_ms: 400, ..Default::default() };
    engine
        .scan(req, move |e| {
            if let Event::Log { driver, message, .. } = e {
                l.lock().unwrap().push(format!("{driver}: {message}"));
            }
        })
        .await
        .expect("scan");
    let logs = logs.lock().unwrap().clone();
    (engine.cameras(), logs)
}

fn by_mac(cams: &[Camera], mac: &str) -> Camera {
    let m: Mac = mac.parse().unwrap();
    cams.iter().find(|c| c.mac == Some(m)).unwrap_or_else(|| panic!("no camera {mac} in {:#?}", cams.iter().map(|c| (&c.vendor, &c.mac, c.ip)).collect::<Vec<_>>())).clone()
}

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn net(addr: &str, gw: &str) -> NetConfig {
    NetConfig { dhcp: false, ip: ip(addr), prefix: 24, gateway: Some(ip(gw)), dns: vec![], hostname: None }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_finds_every_simulated_camera() {
    let (_sim, engine) = rig().await;
    let (cams, logs) = scan(&engine).await;
    assert_eq!(cams.len(), 8, "{cams:#?}\n{logs:#?}");

    let p = by_mac(&cams, "00:80:45:1a:2b:01");
    assert_eq!((p.driver.as_str(), p.model.as_deref(), p.name.as_deref()), ("panasonic", Some("AW-UE150"), Some("CAM 1 WIDE")));
    assert_eq!(p.ip, ip("192.168.0.10"));
    // Heard at layer 2 on sim0 (192.168.10.0/24) with a 192.168.0 address.
    assert_eq!(p.reach, Reach::Mismatch { iface: "sim0".into() });
    assert!(p.caps.write_by_mac);
    assert_eq!(by_mac(&cams, "00:80:45:1a:2b:02").reach, Reach::Local { iface: "sim0".into() });

    let s = by_mac(&cams, "08:00:46:aa:10:01");
    assert_eq!((s.driver.as_str(), s.name.as_deref(), s.firmware.as_deref()), ("sony", Some("STAGE L"), Some("2.10.00")));
    assert!(s.caps.write_network);
    assert!(!by_mac(&cams, "08:00:46:aa:10:02").caps.write_network, "WRITE:off locks it");

    // NDI heard the BirdDog, then the BirdDog driver confirmed it over its API.
    let b = by_mac(&cams, "80:27:b6:26:08:f9");
    assert_eq!(b.driver, "birddog", "{b:#?}");
    assert_eq!(b.model.as_deref(), Some("X1 Ultra"));
    assert!(b.caps.write_network, "X-series has /EthSetup");
    assert!(b.seen.iter().any(|s| s.driver == "ndi"));
    assert_eq!(b.reach, Reach::Local { iface: "sim0".into() });

    // The P-series on its fallback address: NDI hears it across the subnet
    // line, nothing can reach it, so it stays an NDI row with a web UI link.
    let q = cams.iter().find(|c| c.ip == ip("192.168.100.100")).unwrap();
    assert_eq!((q.driver.as_str(), q.vendor.as_str()), ("ndi", "BirdDog"));
    assert_eq!(q.reach, Reach::Mismatch { iface: "sim0".into() });
    assert!(!q.caps.write_network);

    let v = cams.iter().find(|c| c.driver == "vhd").unwrap();
    assert_eq!(v.model.as_deref(), Some("PT20X-SDI-G3"));
    assert!(!v.caps.write_network);

    let o = cams.iter().find(|c| c.driver == "onvif").unwrap();
    assert_eq!(o.model.as_deref(), Some("PTZ330UV"));
    assert_eq!(o.ip, ip("192.168.10.30"));
    assert!(o.caps.needs_auth);
    assert_eq!(o.web_url.as_deref(), Some("http://192.168.10.30/"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_web_ui_opens_the_simulators_page_and_nothing_else_leaves() {
    let (sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let b = by_mac(&cams, "80:27:b6:26:08:f9");
    let url = engine.browser_url(b.web_url.as_deref().unwrap());
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    let page = meerkat_core::http::Http::new(Duration::from_secs(2)).get(&url, None).await.unwrap();
    assert!(page.body.contains("X1 Ultra"), "{}", page.body);
    // Any address that is not a simulated camera goes to a closed port.
    let route = sim.route();
    let dead = route(std::net::SocketAddrV4::new(ip("10.40.7.1"), 80));
    assert_eq!((*dead.ip(), dead.port()), (Ipv4Addr::LOCALHOST, sim.ports.dead));
    assert_eq!(engine.browser_url("http://8.8.8.8/"), format!("http://127.0.0.1:{}/", sim.ports.dead));
}

#[tokio::test(flavor = "multi_thread")]
async fn panasonic_readdress_lands_after_the_confirm() {
    let (sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let cam = by_mac(&cams, "00:80:45:1a:2b:01");
    // The wrong-subnet camera, moved onto this computer's subnet.
    let out = engine.write_network(&cam.id, net("192.168.10.21", "192.168.10.1"), None).await.expect("write");
    assert_eq!(out.expect_ip, Some(ip("192.168.10.21")));
    let now = sim.cam(cam.mac.unwrap()).unwrap();
    assert_eq!((now.ip, now.gateway, now.pending), (ip("192.168.10.21"), ip("192.168.10.1"), None), "the confirm applies it");
    let back = engine.verify(&cam.id, &out, Duration::from_secs(5), |_| {}).await.unwrap().expect("found again");
    assert_eq!(back.ip, ip("192.168.10.21"));
    assert_eq!(back.id, cam.id, "same row: keyed by MAC");
    assert_eq!(back.reach, Reach::Local { iface: "sim0".into() });
}

#[tokio::test(flavor = "multi_thread")]
async fn panasonic_outside_its_window_says_so() {
    let (_sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let cam = by_mac(&cams, "00:80:45:1a:2b:02");
    let e = engine.write_network(&cam.id, net("192.168.10.22", "192.168.10.1"), None).await.unwrap_err();
    assert!(e.contains("20 minutes"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn sony_readdress_and_the_write_lock() {
    let (sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let cam = by_mac(&cams, "08:00:46:aa:10:01");
    let mut cfg = net("192.168.10.32", "192.168.10.1");
    cfg.hostname = Some("STAGE-LEFT".into());
    engine.write_network(&cam.id, cfg, None).await.expect("write");
    let now = sim.cam(cam.mac.unwrap()).unwrap();
    assert_eq!(now.ip, ip("192.168.10.32"));
    assert_eq!(now.name, "STAGELEF", "8 letters, digits or spaces");

    let locked = by_mac(&cams, "08:00:46:aa:10:02");
    let e = engine.write_network(&locked.id, net("192.168.10.33", "192.168.10.1"), None).await.unwrap_err();
    assert!(e.contains("cannot change"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn birddog_ethsetup_applies_on_reboot() {
    let (sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let cam = by_mac(&cams, "80:27:b6:26:08:f9");
    let read = engine.read_network(&cam.id, None).await.expect("read");
    assert!(read.dhcp);
    let out = engine.write_network(&cam.id, net("192.168.10.50", "192.168.10.1"), None).await.expect("write");
    assert!(out.restarts);
    let now = sim.cam(cam.mac.unwrap()).unwrap();
    assert_eq!((now.ip, now.reboots, now.dhcp), (ip("192.168.10.50"), 1, false));
}

#[tokio::test(flavor = "multi_thread")]
async fn onvif_needs_the_right_login() {
    let (sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let cam = cams.iter().find(|c| c.driver == "onvif").unwrap().clone();
    let e = engine.write_network(&cam.id, net("192.168.10.41", "192.168.10.1"), None).await.unwrap_err();
    assert!(e.contains("username and password"), "{e}");
    let wrong = Credentials { username: "admin".into(), password: "nope".into() };
    let e = engine.write_network(&cam.id, net("192.168.10.41", "192.168.10.1"), Some(wrong)).await.unwrap_err();
    assert!(e.to_lowercase().contains("authori") || e.contains("refused"), "{e}");
    let right = Credentials { username: "admin".into(), password: "admin".into() };
    let got = engine.read_network(&cam.id, Some(right.clone())).await.expect("read");
    assert_eq!(got.ip, ip("192.168.10.30"));
    let out = engine.write_network(&cam.id, net("192.168.10.41", "192.168.10.254"), Some(right)).await.expect("write");
    assert!(out.restarts, "RebootNeeded → SystemReboot");
    let now = sim.snapshot().into_iter().find(|c| c.kind == meerkat_sim::Kind::Onvif).unwrap();
    assert_eq!((now.ip, now.gateway, now.reboots), (ip("192.168.10.41"), ip("192.168.10.254"), 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_are_checked_before_they_leave() {
    let (_sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let cam = by_mac(&cams, "00:80:45:1a:2b:01");
    let e = engine.write_network(&cam.id, net("192.168.10.31", "192.168.10.1"), None).await.unwrap_err();
    assert!(e.contains("already in use") && e.contains("FOH"), "{e}");
    let e = engine.write_network(&cam.id, net("10.20.0.21", "10.20.1.1"), None).await.unwrap_err();
    assert!(e.contains("outside"), "{e}");
    let mut dhcp = net("0.0.0.0", "0.0.0.0");
    dhcp.dhcp = true;
    let e = engine.write_network(&cam.id, dhcp, None).await.unwrap_err();
    assert!(e.contains("DHCP"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn panasonic_http_probe_identifies_a_routed_camera() {
    use meerkat_core::driver::{Leg, ScanCtx};
    use meerkat_core::Driver;
    let sim = Sim::start(demo_cameras()).await.unwrap();
    let (mut ctx, _sink) = ScanCtx::for_test("panasonic", Vec::<Leg>::new(), sim.port_map(), Duration::from_millis(500));
    ctx.http = meerkat_core::http::Http::new(Duration::from_secs(1)).with_route(Some(sim.route()));
    let c = meerkat_drivers::panasonic::Panasonic.probe(&ctx, ip("192.168.0.10")).await.unwrap().expect("a Panasonic");
    assert_eq!(c.model.as_deref(), Some("AW-UE150"));
    assert_eq!(c.mac.unwrap().to_string(), "00:80:45:1a:2b:01");
    assert_eq!(c.fields["power"], "on");
    assert!(!c.caps.write_network, "over a router Easy IP cannot re-address it");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_temporary_address_reaches_a_p_series_on_its_fallback_address() {
    let (_sim, engine) = rig().await;
    let (cams, _) = scan(&engine).await;
    let q = cams.iter().find(|c| c.ip == ip("192.168.100.100")).unwrap().clone();
    assert_eq!(q.reach, Reach::Mismatch { iface: "sim0".into() });

    let a = engine.plan_alias(&q.id).expect("plan");
    assert_eq!((a.iface.as_str(), a.ip, a.prefix), ("sim0", ip("192.168.100.253"), 24));
    let now = engine.add_alias(a.clone()).await.expect("add");
    assert_eq!(now.iter().find(|c| c.id == q.id).unwrap().reach, Reach::Local { iface: "sim0".into() });
    assert!(engine.interfaces().unwrap().iter().any(|i| i.ip == a.ip));
    assert!(engine.add_alias(a.clone()).await.unwrap_err().contains("overlaps"), "the same subnet twice");

    // The next scan has BirdDog's API confirm it: a P-series, so its web UI
    // is the way in, and that now opens.
    let (cams, logs) = scan(&engine).await;
    assert!(!logs.iter().any(|l| l.contains("192.168.100.253/24")), "no broadcasts out of the temporary address: {logs:#?}");
    let b = cams.iter().find(|c| c.ip == ip("192.168.100.100")).unwrap().clone();
    assert_eq!((b.driver.as_str(), b.model.as_deref()), ("birddog", Some("P400")), "{b:#?}");
    assert!(!b.caps.write_network);
    let page = meerkat_core::http::Http::new(Duration::from_secs(2)).get(&engine.browser_url(b.web_url.as_deref().unwrap()), None).await.unwrap();
    assert!(page.body.contains("P400"), "{}", page.body);

    // Gone again: back on the wrong subnet, not "routed".
    let after = engine.remove_aliases(None).await.expect("remove");
    assert!(engine.aliases().is_empty());
    assert_eq!(after.iter().find(|c| c.id == b.id).unwrap().reach, Reach::Mismatch { iface: "sim0".into() });
}

#[tokio::test(flavor = "multi_thread")]
async fn a_temporary_address_lets_an_http_driver_readdress_a_wrong_subnet_camera() {
    let mut cams = demo_cameras();
    let x1 = cams.iter_mut().find(|c| c.mac == "80:27:b6:26:08:f9".parse::<Mac>().unwrap()).unwrap();
    x1.ip = ip("192.168.50.40");
    x1.gateway = ip("192.168.50.1");
    let sim = Sim::start(cams).await.expect("simulator");
    let engine = Engine::sandboxed(meerkat_drivers::all(), meerkat_drivers::catalog(), sim.sandbox());

    // Heard over NDI, out of reach: no driver can change it from here.
    let (found, _) = scan(&engine).await;
    let row = found.iter().find(|c| c.ip == ip("192.168.50.40")).unwrap().clone();
    assert_eq!((row.driver.as_str(), row.reach.clone()), ("ndi", Reach::Mismatch { iface: "sim0".into() }));
    assert!(!row.caps.write_network);

    let a = engine.plan_alias(&row.id).expect("plan");
    assert_eq!(a.ip, ip("192.168.50.253"));
    engine.add_alias(a).await.expect("add");
    let (found, _) = scan(&engine).await;
    let b = by_mac(&found, "80:27:b6:26:08:f9");
    assert_eq!((b.driver.as_str(), b.reach.clone()), ("birddog", Reach::Local { iface: "sim0".into() }));
    assert!(b.caps.write_network, "X-series: /EthSetup, now that it is reachable");

    // Onto this computer's own subnet, then the temporary address can go.
    let out = engine.write_network(&b.id, net("192.168.10.50", "192.168.10.1"), None).await.expect("write");
    let out = meerkat_core::WriteOutcome { settle_ms: 0, ..out };
    let back = engine.verify(&b.id, &out, Duration::from_secs(5), |_| {}).await.unwrap().expect("found again");
    assert_eq!((back.id.as_str(), back.ip), (b.id.as_str(), ip("192.168.10.50")));
    engine.remove_aliases(None).await.expect("remove");
    assert_eq!(engine.camera(&b.id).unwrap().reach, Reach::Local { iface: "sim0".into() });
}
