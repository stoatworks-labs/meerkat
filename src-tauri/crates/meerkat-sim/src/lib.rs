//! Simulated PTZ cameras.
//!
//! Every protocol Meerkat speaks, answered from one process on loopback with
//! ephemeral ports: Panasonic Easy IP (search, set, ack, and a change applied
//! only after the confirm — like a UE160), Panasonic's AW CGI, Sony IP Setup
//! (with the WRITE lock), BirdDog's REST API (/EthSetup applied on /reboot),
//! ONVIF (WS-Discovery and a device service that checks the UsernameToken
//! digest) and VHD discovery.
//!
//! [`Sim::sandbox`] is what the engine needs to scan it: a fake `sim0`
//! interface (192.168.10.20/24), sockets bound to loopback, the simulator's
//! ports, and a route that sends each camera's address to that camera's own
//! listener on 127.0.0.1 and every other address to a closed port. So every
//! camera has an address of its own, as on a real network, and nothing a
//! scan or a write sends leaves the machine. The end-to-end tests drive it,
//! and the app's demo mode (`MEERKAT_DEMO=1`) runs it.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};

use meerkat_core::driver::PortMap;
use meerkat_core::http::Route;
use meerkat_core::net::{mask_to_prefix, prefix_to_mask};
use meerkat_core::{Credentials, LocalIface, Mac, Sandbox};
use meerkat_drivers::panasonic::{self, Frame};
use meerkat_drivers::util::{xml_blocks, xml_text};
use meerkat_drivers::{sony, vhd};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Panasonic,
    Sony,
    BirdDog,
    Onvif,
    Vhd,
}

/// One simulated camera's state.
#[derive(Clone, Debug)]
pub struct SimCam {
    pub kind: Kind,
    pub mac: Mac,
    pub ip: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Ipv4Addr,
    pub dns: Vec<Ipv4Addr>,
    pub dhcp: bool,
    pub model: String,
    pub name: String,
    pub firmware: String,
    /// Panasonic: inside the 20-minute window. Sony: WRITE:on.
    pub writable: bool,
    /// ONVIF login.
    pub login: Option<Credentials>,
    /// A BirdDog that has /EthSetup (X-series) or not (P-series).
    pub eth_setup: bool,
    /// A change accepted but not yet applied (Panasonic before confirm,
    /// BirdDog before reboot).
    pub pending: Option<(Ipv4Addr, u8, Ipv4Addr)>,
    /// How many times the camera was told to reboot.
    pub reboots: u32,
}

impl SimCam {
    pub fn new(kind: Kind, mac: [u8; 6], ip: [u8; 4], model: &str, name: &str) -> SimCam {
        let ip = Ipv4Addr::from(ip);
        let o = ip.octets();
        SimCam {
            kind,
            mac: Mac(mac),
            ip,
            prefix: 24,
            gateway: Ipv4Addr::new(o[0], o[1], o[2], 1),
            dns: vec![],
            dhcp: false,
            model: model.into(),
            name: name.into(),
            firmware: "1.00".into(),
            writable: true,
            login: None,
            eth_setup: true,
            pending: None,
            reboots: 0,
        }
    }
}

/// The demo rig, seen from `sim0` (192.168.10.20/24). On the right subnet:
/// a Panasonic past its 20-minute window, a write-locked Sony, an X-series
/// BirdDog (found over NDI, then its API) and an ONVIF-only AVer. On the
/// wrong subnet, found anyway: a Panasonic and a Sony that broadcast
/// discovery can still re-address, a PTZOptics, and a P-series BirdDog
/// sitting on its 192.168.100.100 fallback.
pub fn demo_cameras() -> Vec<SimCam> {
    let mut v = vec![
        SimCam::new(Kind::Panasonic, [0x00, 0x80, 0x45, 0x1a, 0x2b, 0x01], [192, 168, 0, 10], "AW-UE150", "CAM 1 WIDE"),
        SimCam::new(Kind::Panasonic, [0x00, 0x80, 0x45, 0x1a, 0x2b, 0x02], [192, 168, 10, 12], "AW-UE100", "CAM 2 TIGHT"),
        SimCam::new(Kind::Sony, [0x08, 0x00, 0x46, 0xaa, 0x10, 0x01], [192, 168, 0, 100], "SRG-X400", "STAGE L"),
        SimCam::new(Kind::Sony, [0x08, 0x00, 0x46, 0xaa, 0x10, 0x02], [192, 168, 10, 31], "BRC-AM7", "FOH"),
        SimCam::new(Kind::BirdDog, [0x80, 0x27, 0xb6, 0x26, 0x08, 0xf9], [192, 168, 10, 40], "X1 Ultra", "birddog-608f9"),
        SimCam::new(Kind::BirdDog, [0x80, 0x27, 0xb6, 0x30, 0x10, 0x02], [192, 168, 100, 100], "P400", "birddog-31002"),
        SimCam::new(Kind::Onvif, [0x00, 0x18, 0x1a, 0x00, 0x33, 0x01], [192, 168, 10, 30], "PTZ330UV", "AVer Lectern"),
        SimCam::new(Kind::Vhd, [0xdc, 0xed, 0x84, 0x00, 0x11, 0x22], [192, 168, 100, 88], "PT20X-SDI-G3", "PTZOptics"),
    ];
    v[1].writable = false;
    v[1].firmware = "4.10".into();
    v[2].firmware = "2.10.00".into();
    v[3].writable = false;
    v[4].firmware = "6.1.059".into();
    v[4].dhcp = true;
    v[5].firmware = "5.5.110".into();
    v[5].eth_setup = false;
    v[6].login = Some(Credentials { username: "admin".into(), password: "admin".into() });
    v
}

/// The ports a camera of this kind answers HTTP on.
fn http_ports(kind: Kind) -> &'static [u16] {
    match kind {
        Kind::Panasonic | Kind::Onvif => &[80],
        Kind::BirdDog => &[80, 8080],
        _ => &[],
    }
}

/// The ports the simulator listens on (camera side) and the ones the
/// drivers should listen on (PC side).
#[derive(Clone, Debug, Default)]
pub struct Ports {
    pub panasonic_camera: u16,
    pub panasonic_pc: u16,
    pub panasonic_json_pc: u16,
    pub panasonic_json_camera: u16,
    pub sony_setup: u16,
    pub sony_reply: u16,
    pub onvif_wsd: u16,
    pub vhd: u16,
    pub ndi: u16,
    /// Bound once and closed: where the route sends every address that is
    /// not a simulated camera, so it is refused at once.
    pub dead: u16,
}

pub struct Sim {
    pub cams: Arc<Mutex<Vec<SimCam>>>,
    pub ports: Ports,
    /// Each HTTP camera's own listener on 127.0.0.1.
    http: Arc<Vec<(Mac, u16)>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Sim {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

const LO: Ipv4Addr = Ipv4Addr::LOCALHOST;

async fn udp() -> std::io::Result<UdpSocket> {
    UdpSocket::bind(SocketAddrV4::new(LO, 0)).await
}

async fn free_port() -> std::io::Result<u16> {
    Ok(udp().await?.local_addr()?.port())
}

impl Sim {
    /// Start every responder on 127.0.0.1.
    pub async fn start(cams: Vec<SimCam>) -> std::io::Result<Sim> {
        let mut http = Vec::new();
        let mut listeners = Vec::new();
        for c in cams.iter().filter(|c| !http_ports(c.kind).is_empty()) {
            let l = TcpListener::bind(SocketAddrV4::new(LO, 0)).await?;
            http.push((c.mac, l.local_addr()?.port()));
            listeners.push((c.mac, l));
        }
        let cams = Arc::new(Mutex::new(cams));
        let pana = Arc::new(udp().await?);
        let sony_s = Arc::new(udp().await?);
        let wsd = Arc::new(udp().await?);
        let vhd_s = Arc::new(udp().await?);
        let ndi_s = Arc::new(udp().await?);
        let ports = Ports {
            panasonic_camera: pana.local_addr()?.port(),
            panasonic_pc: free_port().await?,
            panasonic_json_pc: free_port().await?,
            panasonic_json_camera: free_port().await?,
            sony_setup: sony_s.local_addr()?.port(),
            sony_reply: free_port().await?,
            onvif_wsd: wsd.local_addr()?.port(),
            vhd: vhd_s.local_addr()?.port(),
            ndi: ndi_s.local_addr()?.port(),
            dead: free_port().await?,
        };
        let mut tasks = vec![
            tokio::spawn(panasonic_udp(pana, cams.clone(), ports.panasonic_pc)),
            tokio::spawn(sony_udp(sony_s, cams.clone(), ports.sony_reply)),
            tokio::spawn(onvif_wsd(wsd, cams.clone())),
            tokio::spawn(vhd_udp(vhd_s, cams.clone())),
            tokio::spawn(ndi_udp(ndi_s, cams.clone())),
        ];
        for (mac, l) in listeners {
            tasks.push(tokio::spawn(http_server(l, cams.clone(), mac)));
        }
        Ok(Sim { cams, ports, http: Arc::new(http), tasks })
    }

    /// The engine's port overrides for this simulator.
    pub fn port_map(&self) -> PortMap {
        let p = &self.ports;
        HashMap::from([
            ("panasonic.camera".into(), p.panasonic_camera),
            ("panasonic.pc".into(), p.panasonic_pc),
            ("panasonic.json_pc".into(), p.panasonic_json_pc),
            ("panasonic.json_camera".into(), p.panasonic_json_camera),
            ("sony.setup".into(), p.sony_setup),
            ("sony.reply".into(), p.sony_reply),
            ("onvif.wsd".into(), p.onvif_wsd),
            ("vhd.udp".into(), p.vhd),
            ("ndi.sim".into(), p.ndi),
        ])
    }

    /// Where traffic for each address goes. Loopback stays loopback (the
    /// discovery responders live there); a broadcast reaches them too; a
    /// camera's address and HTTP port reach that camera's listener, wherever
    /// it has been re-addressed to since; anything else is refused.
    pub fn route(&self) -> Route {
        let cams = self.cams.clone();
        let http = self.http.clone();
        let dead = SocketAddrV4::new(LO, self.ports.dead);
        let bcast = Sim::iface().broadcast.unwrap_or(Ipv4Addr::BROADCAST);
        Arc::new(move |a: SocketAddrV4| {
            let ip = *a.ip();
            if ip.is_loopback() {
                return a;
            }
            if ip == Ipv4Addr::BROADCAST || ip == bcast || ip.is_multicast() {
                return SocketAddrV4::new(LO, a.port());
            }
            let cams = cams.lock().unwrap();
            cams.iter()
                .find(|c| c.ip == ip && http_ports(c.kind).contains(&a.port()))
                .and_then(|c| http.iter().find(|(m, _)| *m == c.mac))
                .map(|(_, p)| SocketAddrV4::new(LO, *p))
                .unwrap_or(dead)
        })
    }

    /// Everything an [`meerkat_core::Engine`] needs to scan this simulator
    /// and nothing else.
    pub fn sandbox(&self) -> Sandbox {
        Sandbox { ifaces: vec![Sim::iface()], bind: LO, ports: self.port_map(), route: self.route() }
    }

    /// The fake interface a scan of the simulator broadcasts on.
    pub fn iface() -> LocalIface {
        LocalIface {
            name: "sim0".into(),
            index: None,
            ip: Ipv4Addr::new(192, 168, 10, 20),
            prefix: 24,
            broadcast: Some(Ipv4Addr::new(192, 168, 10, 255)),
            loopback: false,
            p2p: false,
            link_local: false,
        }
    }

    pub fn cam(&self, mac: Mac) -> Option<SimCam> {
        self.cams.lock().unwrap().iter().find(|c| c.mac == mac).cloned()
    }

    pub fn snapshot(&self) -> Vec<SimCam> {
        self.cams.lock().unwrap().clone()
    }
}

fn mask(c: &SimCam) -> Ipv4Addr {
    prefix_to_mask(c.prefix)
}

// ---------------------------------------------------------------- Panasonic

fn pana_tlv(p: &mut Vec<u8>, id: u16, v: &[u8]) {
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&(v.len() as u16).to_be_bytes());
    p.extend_from_slice(v);
}

fn pana_head(kind: u16, cam: &SimCam, req: &Frame, b22: [u8; 2], b33: [u8; 4]) -> Vec<u8> {
    let mut p = vec![0x00, 0x01, 0x00, 0x00];
    p.extend_from_slice(&kind.to_be_bytes());
    p.extend_from_slice(&cam.mac.octets());
    p.extend_from_slice(&req.pc_mac);
    p.extend_from_slice(&req.pc_ip.octets());
    p.extend_from_slice(&b22);
    p.extend_from_slice(&[0x20, 0x11, 0x1E, 0x11, 0x23, 0x1F, 0x1E, 0x19, 0x13]);
    p.extend_from_slice(&b33);
    p.extend_from_slice(&[0u8; 11]);
    p
}

fn pana_reply(cam: &SimCam, req: &Frame) -> Vec<u8> {
    let mut p = pana_head(panasonic::T_REPLY, cam, req, [0, 0], [0, 0, 2, 2]);
    let mut dns = [0u8; 8];
    for (i, d) in cam.dns.iter().take(2).enumerate() {
        dns[i * 4..i * 4 + 4].copy_from_slice(&d.octets());
    }
    let pad16 = |s: &str| {
        let mut v = s.as_bytes().to_vec();
        v.resize(16, 0);
        v
    };
    pana_tlv(&mut p, 0x0000, &[0x02]);
    pana_tlv(&mut p, 0x0001, &[0x00]);
    pana_tlv(&mut p, 0x0020, &cam.ip.octets());
    pana_tlv(&mut p, 0x0021, &mask(cam).octets());
    pana_tlv(&mut p, 0x0022, &cam.gateway.octets());
    pana_tlv(&mut p, 0x0023, &dns);
    pana_tlv(&mut p, 0x0025, &80u16.to_be_bytes());
    pana_tlv(&mut p, 0x00A0, &cam.ip.octets());
    pana_tlv(&mut p, 0x00A1, &mask(cam).octets());
    pana_tlv(&mut p, 0x00A2, &cam.gateway.octets());
    pana_tlv(&mut p, 0x00A3, &dns);
    pana_tlv(&mut p, 0x00A7, &pad16(&cam.model));
    pana_tlv(&mut p, 0x00A8, &pad16(&cam.name));
    pana_tlv(&mut p, 0x00A9, &pad16(&cam.firmware));
    panasonic::finish(p)
}

async fn panasonic_udp(s: Arc<UdpSocket>, cams: Arc<Mutex<Vec<SimCam>>>, pc_port: u16) {
    let mut buf = vec![0u8; 4096];
    loop {
        let Ok((n, from)) = s.recv_from(&mut buf).await else { continue };
        let Some(f) = Frame::parse(&buf[..n]) else { continue };
        let to = SocketAddr::new(from.ip(), pc_port);
        let mut out = Vec::new();
        {
            let mut cams = cams.lock().unwrap();
            match f.kind {
                panasonic::T_SEARCH => {
                    for c in cams.iter().filter(|c| c.kind == Kind::Panasonic) {
                        out.push(pana_reply(c, &f));
                    }
                }
                panasonic::T_SET => {
                    if let Some(c) = cams.iter_mut().find(|c| c.kind == Kind::Panasonic && c.mac.octets() == f.cam_mac) {
                        // Outside the window a real camera says nothing at all.
                        if c.writable {
                            if let (Some(ip), Some(m), Some(gw)) = (f.ip4(0x0020), f.ip4(0x0021), f.ip4(0x0022)) {
                                // A set arrives once per broadcast address it was sent to;
                                // one that matches what is already applied changes nothing.
                                let want = (ip, mask_to_prefix(m).unwrap_or(24), gw);
                                if want != (c.ip, c.prefix, c.gateway) {
                                    c.pending = Some(want);
                                }
                                out.push(panasonic::finish(pana_head(panasonic::T_ACK, c, &f, [0, 1], [0, 0, 2, 2])));
                            }
                        }
                    }
                }
                panasonic::T_CONFIRM => {
                    if let Some(c) = cams.iter_mut().find(|c| c.kind == Kind::Panasonic && c.mac.octets() == f.cam_mac) {
                        if let Some((ip, prefix, gw)) = c.pending.take() {
                            c.ip = ip;
                            c.prefix = prefix;
                            c.gateway = gw;
                        }
                    }
                }
                _ => {}
            }
        }
        for p in out {
            let _ = s.send_to(&p, to).await;
        }
    }
}

// ---------------------------------------------------------------- Sony

async fn sony_udp(s: Arc<UdpSocket>, cams: Arc<Mutex<Vec<SimCam>>>, reply_port: u16) {
    let mut buf = vec![0u8; 4096];
    loop {
        let Ok((n, from)) = s.recv_from(&mut buf).await else { continue };
        let Some(fields) = sony::parse(&buf[..n]) else { continue };
        let to = SocketAddr::new(from.ip(), reply_port);
        let get = |k: &str| fields.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        let mut out = Vec::new();
        {
            let mut cams = cams.lock().unwrap();
            if get("ENQ").as_deref() == Some("network") {
                for c in cams.iter().filter(|c| c.kind == Kind::Sony) {
                    out.push(sony::frame(&[
                        ("MAC", &c.mac.to_upper("-")),
                        ("INFO", "network"),
                        ("MODEL", "IPCARD"),
                        ("SOFTVERSION", &c.firmware),
                        ("IPADR", &c.ip.to_string()),
                        ("MASK", &mask(c).to_string()),
                        ("GATEWAY", &c.gateway.to_string()),
                        ("NAME", &c.name),
                        ("WRITE", if c.writable { "on" } else { "off" }),
                    ]));
                }
            } else if let Some(mac) = get("MAC").and_then(|m| m.parse::<Mac>().ok()) {
                if let Some(c) = cams.iter_mut().find(|c| c.kind == Kind::Sony && c.mac == mac) {
                    let ip = get("IPADR").and_then(|v| v.parse().ok());
                    let m = get("MASK").and_then(|v| v.parse().ok()).and_then(mask_to_prefix);
                    if let (true, Some(ip), Some(m)) = (c.writable, ip, m) {
                        c.ip = ip;
                        c.prefix = m;
                        if let Some(g) = get("GATEWAY").and_then(|v| v.parse().ok()) {
                            c.gateway = g;
                        }
                        if let Some(n) = get("NAME") {
                            c.name = n;
                        }
                        out.push(sony::frame(&[("ACK", &c.mac.to_upper("-"))]));
                    } else {
                        out.push(sony::frame(&[("NAK", &c.mac.to_upper("-")), ("WRITE", "off")]));
                    }
                }
            }
        }
        for p in out {
            let _ = s.send_to(&p, to).await;
        }
    }
}

// ---------------------------------------------------------------- ONVIF

async fn onvif_wsd(s: Arc<UdpSocket>, cams: Arc<Mutex<Vec<SimCam>>>) {
    let mut buf = vec![0u8; 8192];
    loop {
        let Ok((n, from)) = s.recv_from(&mut buf).await else { continue };
        let text = String::from_utf8_lossy(&buf[..n]).into_owned();
        if !text.contains("Probe") {
            continue;
        }
        let id = xml_text(&text, "MessageID").unwrap_or_default();
        let list: Vec<SimCam> = cams.lock().unwrap().iter().filter(|c| c.kind == Kind::Onvif).cloned().collect();
        for c in list {
            let body = format!(
                r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery"><s:Header><a:RelatesTo>{id}</a:RelatesTo></s:Header><s:Body><d:ProbeMatches><d:ProbeMatch><a:EndpointReference><a:Address>urn:uuid:sim-{mac}</a:Address></a:EndpointReference><d:Scopes>onvif://www.onvif.org/type/ptz onvif://www.onvif.org/name/{name} onvif://www.onvif.org/hardware/{model} onvif://www.onvif.org/Profile/Streaming</d:Scopes><d:XAddrs>http://{ip}/onvif/device_service</d:XAddrs></d:ProbeMatch></d:ProbeMatches></s:Body></s:Envelope>"#,
                mac = c.mac.to_upper(""),
                ip = c.ip,
                name = c.name.replace(' ', "%20"),
                model = c.model,
            );
            let _ = s.send_to(body.as_bytes(), from).await;
        }
    }
}

fn onvif_auth_ok(body: &str, login: &Credentials) -> bool {
    let (Some(user), Some(pass), Some(nonce), Some(created)) = (xml_text(body, "Username"), xml_text(body, "Password"), xml_text(body, "Nonce"), xml_text(body, "Created")) else { return false };
    let Some(nonce) = b64decode(&nonce) else { return false };
    let mut h = Sha1::new();
    h.update(&nonce);
    h.update(created.as_bytes());
    h.update(login.password.as_bytes());
    user == login.username && meerkat_core::http::base64(&h.finalize()) == pass
}

fn b64decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=' && !c.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

fn soap(body: &str) -> String {
    format!(r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:tds="http://www.onvif.org/ver10/device/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema"><s:Body>{body}</s:Body></s:Envelope>"#)
}

fn onvif_handle(req: &str, c: &mut SimCam) -> (u16, String) {
    let body = xml_blocks(req, "Body").into_iter().next().unwrap_or("").to_string();
    if body.contains("GetSystemDateAndTime") {
        return (200, soap("<tds:GetSystemDateAndTimeResponse><tds:SystemDateAndTime><tt:UTCDateTime><tt:Time><tt:Hour>0</tt:Hour><tt:Minute>0</tt:Minute><tt:Second>0</tt:Second></tt:Time><tt:Date><tt:Year>2026</tt:Year><tt:Month>11</tt:Month><tt:Day>20</tt:Day></tt:Date></tt:UTCDateTime></tds:SystemDateAndTime></tds:GetSystemDateAndTimeResponse>"));
    }
    if let Some(login) = &c.login {
        if !onvif_auth_ok(req, login) {
            return (400, soap(r#"<s:Fault><s:Code><s:Value>s:Sender</s:Value><s:Subcode><s:Value>ter:NotAuthorized</s:Value></s:Subcode></s:Code><s:Reason><s:Text xml:lang="en">Sender not Authorized</s:Text></s:Reason></s:Fault>"#));
        }
    }
    let r = if body.contains("GetNetworkInterfaces") {
        format!(
            r#"<tds:GetNetworkInterfacesResponse><tds:NetworkInterfaces token="eth0"><tt:Enabled>true</tt:Enabled><tt:Info><tt:HwAddress>{}</tt:HwAddress></tt:Info><tt:IPv4><tt:Enabled>true</tt:Enabled><tt:Config><tt:Manual><tt:Address>{}</tt:Address><tt:PrefixLength>{}</tt:PrefixLength></tt:Manual><tt:DHCP>{}</tt:DHCP></tt:Config></tt:IPv4></tds:NetworkInterfaces></tds:GetNetworkInterfacesResponse>"#,
            c.mac, c.ip, c.prefix, c.dhcp
        )
    } else if body.contains("GetNetworkDefaultGateway") {
        format!("<tds:GetNetworkDefaultGatewayResponse><tds:NetworkGateway><tt:IPv4Address>{}</tt:IPv4Address></tds:NetworkGateway></tds:GetNetworkDefaultGatewayResponse>", c.gateway)
    } else if body.contains("GetDNS") {
        let d: String = c.dns.iter().map(|d| format!("<tt:DNSManual><tt:Type>IPv4</tt:Type><tt:IPv4Address>{d}</tt:IPv4Address></tt:DNSManual>")).collect();
        format!("<tds:GetDNSResponse><tds:DNSInformation><tt:FromDHCP>false</tt:FromDHCP>{d}</tds:DNSInformation></tds:GetDNSResponse>")
    } else if body.contains("GetHostname") {
        format!("<tds:GetHostnameResponse><tds:HostnameInformation><tt:FromDHCP>false</tt:FromDHCP><tt:Name>{}</tt:Name></tds:HostnameInformation></tds:GetHostnameResponse>", c.name.replace(' ', "-"))
    } else if body.contains("GetDeviceInformation") {
        format!("<tds:GetDeviceInformationResponse><tds:Manufacturer>AVer</tds:Manufacturer><tds:Model>{}</tds:Model><tds:FirmwareVersion>{}</tds:FirmwareVersion><tds:SerialNumber>SIM1</tds:SerialNumber><tds:HardwareId>1</tds:HardwareId></tds:GetDeviceInformationResponse>", c.model, c.firmware)
    } else if body.contains("SetNetworkDefaultGateway") {
        if let Some(g) = xml_text(&body, "IPv4Address").and_then(|g| g.parse().ok()) {
            c.gateway = g;
        }
        "<tds:SetNetworkDefaultGatewayResponse/>".into()
    } else if body.contains("SetDNS") {
        c.dns = xml_blocks(&body, "DNSManual").into_iter().filter_map(|b| xml_text(b, "IPv4Address")?.parse().ok()).collect();
        "<tds:SetDNSResponse/>".into()
    } else if body.contains("SetHostname") {
        if let Some(n) = xml_text(&body, "Name") {
            c.name = n;
        }
        "<tds:SetHostnameResponse/>".into()
    } else if body.contains("SetNetworkInterfaces") {
        if xml_text(&body, "DHCP").as_deref() == Some("true") {
            c.dhcp = true;
        } else if let (Some(ip), Some(p)) = (xml_text(&body, "Address").and_then(|a| a.parse().ok()), xml_text(&body, "PrefixLength").and_then(|p| p.parse().ok())) {
            c.pending = Some((ip, p, c.gateway));
            c.dhcp = false;
        }
        "<tds:SetNetworkInterfacesResponse><tds:RebootNeeded>true</tds:RebootNeeded></tds:SetNetworkInterfacesResponse>".into()
    } else if body.contains("SystemReboot") {
        c.reboots += 1;
        if let Some((ip, p, _)) = c.pending.take() {
            c.ip = ip;
            c.prefix = p;
        }
        "<tds:SystemRebootResponse><tds:Message>Rebooting</tds:Message></tds:SystemRebootResponse>".into()
    } else {
        return (400, soap("<s:Fault><s:Reason><s:Text>unsupported</s:Text></s:Reason></s:Fault>"));
    };
    (200, soap(&r))
}

// ---------------------------------------------------------------- VHD

async fn vhd_udp(s: Arc<UdpSocket>, cams: Arc<Mutex<Vec<SimCam>>>) {
    let mut buf = vec![0u8; 2048];
    loop {
        let Ok((n, from)) = s.recv_from(&mut buf).await else { continue };
        let t = String::from_utf8_lossy(&buf[..n]).into_owned();
        if !t.starts_with("SEARCH * UPGRADE") {
            continue;
        }
        let client = t.lines().find_map(|l| l.strip_prefix("Client ID:")).unwrap_or("").to_string();
        let list: Vec<SimCam> = cams.lock().unwrap().iter().filter(|c| c.kind == Kind::Vhd).cloned().collect();
        for c in list {
            let r = format!(
                "REPLY OK\r\nVERSION:SOC v6.3.32\r\nDEVICE_MODEL:{}\r\nClient ID:{client}\r\nDevice ID:sim{}\r\nUptime=1234\r\nDHCP=0\r\nIP={}\r\nMASK={}\r\nGATEWAY={}\r\nMAC={}\r\nFDNS={}",
                c.model,
                c.mac.to_upper(""),
                c.ip,
                mask(&c),
                c.gateway,
                c.mac.to_upper(":"),
                c.gateway
            );
            let _ = s.send_to(r.as_bytes(), from).await;
        }
    }
    #[allow(unreachable_code)]
    {
        let _ = vhd::PORT;
    }
}

// ---------------------------------------------------------------- NDI

/// Pretend mDNS: every BirdDog announces itself as an NDI source at its own
/// address — on the wrong subnet too, as mDNS does.
async fn ndi_udp(s: Arc<UdpSocket>, cams: Arc<Mutex<Vec<SimCam>>>) {
    let mut buf = vec![0u8; 256];
    loop {
        let Ok((_, from)) = s.recv_from(&mut buf).await else { continue };
        let list: Vec<serde_json::Value> = cams
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.kind == Kind::BirdDog)
            .map(|c| serde_json::json!({ "name": format!("{} (CAM)._ndi._tcp.local.", c.name.to_uppercase()), "host": format!("{}.local.", c.name), "port": 5961, "ip": c.ip.to_string() }))
            .collect();
        let _ = s.send_to(serde_json::to_string(&list).unwrap().as_bytes(), from).await;
    }
}

// ---------------------------------------------------------------- HTTP

async fn http_server(l: TcpListener, cams: Arc<Mutex<Vec<SimCam>>>, mac: Mac) {
    loop {
        let Ok((sock, _)) = l.accept().await else { continue };
        let cams = cams.clone();
        tokio::spawn(async move {
            let _ = serve(sock, cams, mac).await;
        });
    }
}

async fn serve(mut sock: TcpStream, cams: Arc<Mutex<Vec<SimCam>>>, mac: Mac) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
    while buf.len() < head_end + len {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("/").to_string();
    let (status, ctype, out) = {
        let mut cams = cams.lock().unwrap();
        match cams.iter_mut().find(|c| c.mac == mac) {
            None => (404, "text/plain", String::new()),
            Some(c) if method == "GET" && path == "/" => (200, "text/html; charset=utf-8", web_ui(c)),
            Some(c) => match c.kind {
                Kind::Panasonic => panasonic_http(&path, c),
                Kind::BirdDog => birddog_http(&method, &path, &body, c),
                Kind::Onvif => {
                    let (s, b) = onvif_handle(&body, c);
                    (s, "application/soap+xml", b)
                }
                _ => (404, "text/plain", String::new()),
            },
        }
    };
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let resp = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
    sock.write_all(resp.as_bytes()).await?;
    sock.shutdown().await
}

/// What "Open web UI" shows in the demo, in place of the camera's own pages.
fn web_ui(c: &SimCam) -> String {
    let vendor = match c.kind {
        Kind::Panasonic => "Panasonic",
        Kind::Sony => "Sony",
        Kind::BirdDog => "BirdDog",
        Kind::Onvif => "AVer",
        Kind::Vhd => "PTZOptics",
    };
    format!(
        r#"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{vendor} {model}</title>
<style>body{{font:15px/1.5 system-ui,sans-serif;margin:0;background:#f4f1ea;color:#1d2433}}main{{max-width:34rem;margin:12vh auto;padding:0 16px}}h1{{font-size:1.4rem;margin:0 0 .25rem}}p{{color:#556}}dl{{display:grid;grid-template-columns:auto 1fr;gap:.25rem 1rem}}dt{{color:#778}}code{{font-family:ui-monospace,monospace}}</style>
<main><h1>{vendor} {model}</h1><p>A simulated camera in Meerkat's demo. On a real network this opens the camera's own web interface.</p>
<dl><dt>Name</dt><dd>{name}</dd><dt>Address</dt><dd><code>{ip}/{prefix}</code></dd><dt>Gateway</dt><dd><code>{gw}</code></dd><dt>MAC</dt><dd><code>{mac}</code></dd><dt>Firmware</dt><dd>{fw}</dd></dl></main>"#,
        model = c.model,
        name = c.name,
        ip = c.ip,
        prefix = c.prefix,
        gw = c.gateway,
        mac = c.mac,
        fw = c.firmware,
    )
}

fn panasonic_http(path: &str, c: &SimCam) -> (u16, &'static str, String) {
    if path.starts_with("/cgi-bin/aw_cam?cmd=QID") {
        (200, "text/plain", format!("OID:{}", c.model))
    } else if path.starts_with("/cgi-bin/aw_cam?cmd=QSV") {
        (200, "text/plain", format!("OSV:{}", c.firmware))
    } else if path.starts_with("/cgi-bin/getinfo") {
        (200, "text/plain", format!("MAC={}\r\nSERIAL=SIM0001\r\nVERSION={}\r\nNAME={}\r\n", c.mac.to_upper("-"), c.firmware, c.model))
    } else if path.starts_with("/cgi-bin/aw_ptz?cmd=%23O") {
        (200, "text/plain", "p1".into())
    } else {
        (404, "text/plain", String::new())
    }
}

fn birddog_http(method: &str, path: &str, body: &str, c: &mut SimCam) -> (u16, &'static str, String) {
    let j = "application/json";
    match (method, path) {
        ("GET", "/version") => (200, "text/plain", format!("BirdDog {}", c.model)),
        ("GET", "/about") => (
            200,
            j,
            serde_json::json!({
                "FallbackIP": "192.168.100.100", "FirmwareVersion": format!("BirdDog {} {}", c.model, c.firmware), "Format": "CAM",
                "GateWay": c.gateway.to_string(), "HardwareVersion": format!("BirdDog {}", c.model), "HostName": c.name,
                "IPAddress": c.ip.to_string(), "MCUVersion": "5", "NetworkConfigMethod": if c.dhcp { "dhcp" } else { "static" },
                "NetworkMask": mask(c).to_string(), "SerialNumber": c.mac.to_upper("").to_lowercase(), "Status": "Active", "Dns": ""
            })
            .to_string(),
        ),
        ("GET", "/EthSetup") if c.eth_setup => (200, j, eth_json(c)),
        ("POST", "/EthSetup") if c.eth_setup => {
            let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
            if let Some(m) = s("IpMode") {
                c.dhcp = m == "dhcp";
            }
            let ip = s("IpAddr").and_then(|x| x.parse().ok()).unwrap_or(c.ip);
            let p = s("NetMask").and_then(|x| x.parse().ok()).and_then(mask_to_prefix).unwrap_or(c.prefix);
            let gw = s("GateWayIp").and_then(|x| x.parse().ok()).unwrap_or(c.gateway);
            c.pending = Some((ip, p, gw));
            let mut out: serde_json::Value = serde_json::from_str(&eth_json(c)).unwrap();
            out["IpAddr"] = ip.to_string().into();
            out["NetMask"] = prefix_to_mask(p).to_string().into();
            (200, j, out.to_string())
        }
        ("POST", "/reboot") | ("GET", "/reboot") => {
            c.reboots += 1;
            if let Some((ip, p, gw)) = c.pending.take() {
                c.ip = ip;
                c.prefix = p;
                c.gateway = gw;
            }
            (200, j, r#"{"status":"success"}"#.into())
        }
        _ => (404, "text/plain", String::new()),
    }
}

fn eth_json(c: &SimCam) -> String {
    let (ip, p, gw) = c.pending.unwrap_or((c.ip, c.prefix, c.gateway));
    serde_json::json!({
        "IpMode": if c.dhcp { "dhcp" } else { "static" }, "IpAddr": ip.to_string(), "NetMask": prefix_to_mask(p).to_string(),
        "GateWayIp": gw.to_string(), "DnsSvrIpFirst": "", "DnsSvrIpSecond": "", "DhcpTimeout": "30",
        "DhcpFallbackIPAddr": "192.168.100.100", "DhcpFallbackNetMask": "255.255.255.0", "DhcpFallbackGateWayIp": "192.168.100.1"
    })
    .to_string()
}
