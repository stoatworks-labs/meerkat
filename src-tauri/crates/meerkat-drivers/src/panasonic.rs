//! Panasonic — Easy IP Setup (UDP 10669/10670), plus the AW CGI for routed
//! sweeps.
//!
//! The frame format is from packet captures published by two independent
//! reimplementations — dPro-Software's PanasonicEasyIPsetupCore (an AW-HE130,
//! 2015–2018) and AndyJae's smart-reset-browser (an AW-UE160, 2026, checked
//! against Panasonic's own tool doing a real change) — and Panasonic's AW-UE100
//! operating instructions and web-control supplements.
//!
//! ```text
//! 0-1   00 01
//! 2-3   u16 total length − 52
//! 4-5   type: 000D search · 000E search reply · 0002 set · 0003 set ack · 0004 confirm
//! 6-11  camera MAC (zeros in a search)
//! 12-17 PC MAC   18-21 PC IPv4  (echoed back in replies)
//! 22-23 00 00 search/reply · 00 01 set/ack · 00 02 confirm
//! 24-32 20 11 1E 11 23 1F 1E 19 13
//! 33-36 00 00 00 01 search · 00 00 02 02 reply · 00 00 02 00 set/confirm (ack: status in 35-36)
//! 37-47 zeros
//! 48..  TLVs: u16 id, u16 length, value
//! end   FF FF, u16 checksum = (sum of every preceding byte + 1) & 0xFFFF
//! ```
//!
//! Search goes to 255.255.255.255:10670; cameras answer by broadcast to 10669,
//! so a camera on any address is found from any interface on its wire. A set
//! is keyed by the camera's MAC and is sent twice (address-mode TLV 03, then
//! 02), each to broadcast and the camera's address on both ports; an acked set
//! is followed by a type-0004 confirm, without which a UE160 acks and never
//! applies the change. Cameras accept sets only in the first ~20 minutes after
//! power-on unless "Easy IP Setup accommodate period" is Unlimited — outside it
//! they do not answer at all.
//!
//! Cameras with user authentication on want the JSON stage on 10671/10672
//! first (Lookup → Req-Pub → Inform → Setup → Ack-Client → Ack-Server, with
//! `access_number = MD5("user:pass:random")`); Meerkat runs it when it has
//! credentials. The UE160's RSA variant of that stage is undocumented and not
//! supported.

use std::collections::{BTreeMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use md5::{Digest, Md5};
use meerkat_core::driver::{Leg, Level, Result};
use meerkat_core::net::{self, mask_to_prefix, prefix_to_mask, udp_socket, UdpOpts};
use meerkat_core::{Caps, Camera, Credentials, Driver, DriverError, DriverInfo, Mac, Method, NetConfig, OpCtx, ScanCtx, WriteOutcome};
use serde_json::{json, Value};
use tokio::net::UdpSocket;
use tokio::time::Instant;

use crate::udp::Inbox;

pub struct Panasonic;

pub const ID: &str = "panasonic";
pub const CAMERA_PORT: u16 = 10670;
pub const PC_PORT: u16 = 10669;
pub const JSON_PC_PORT: u16 = 10671;
pub const JSON_CAMERA_PORT: u16 = 10672;

const MAGIC: [u8; 9] = [0x20, 0x11, 0x1E, 0x11, 0x23, 0x1F, 0x1E, 0x19, 0x13];
/// The field list Easy IP Setup asks for (advisory: cameras send what they send).
const ASK: [u16; 19] = [0x20, 0x21, 0x22, 0x23, 0x25, 0x28, 0x40, 0x41, 0x42, 0x44, 0xA5, 0xA6, 0xA7, 0xA8, 0xAD, 0xB3, 0xB4, 0xB7, 0xB8];

pub const T_SEARCH: u16 = 0x000D;
pub const T_REPLY: u16 = 0x000E;
pub const T_SET: u16 = 0x0002;
pub const T_ACK: u16 = 0x0003;
pub const T_CONFIRM: u16 = 0x0004;

fn header(kind: u16, cam: [u8; 6], pc_mac: [u8; 6], pc_ip: Ipv4Addr, b22: [u8; 2], b33: [u8; 4]) -> Vec<u8> {
    let mut p = Vec::with_capacity(128);
    p.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    p.extend_from_slice(&kind.to_be_bytes());
    p.extend_from_slice(&cam);
    p.extend_from_slice(&pc_mac);
    p.extend_from_slice(&pc_ip.octets());
    p.extend_from_slice(&b22);
    p.extend_from_slice(&MAGIC);
    p.extend_from_slice(&b33);
    p.extend_from_slice(&[0u8; 11]);
    debug_assert_eq!(p.len(), 48);
    p
}

fn tlv(p: &mut Vec<u8>, id: u16, v: &[u8]) {
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&(v.len() as u16).to_be_bytes());
    p.extend_from_slice(v);
}

/// Append the FF FF trailer, fill in the length and the checksum.
pub fn finish(mut p: Vec<u8>) -> Vec<u8> {
    p.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
    let len = p.len();
    p[2..4].copy_from_slice(&((len - 52) as u16).to_be_bytes());
    let ck = checksum(&p[..len - 2]);
    p[len - 2..].copy_from_slice(&ck.to_be_bytes());
    p
}

pub fn checksum(b: &[u8]) -> u16 {
    (b.iter().map(|x| *x as u32).sum::<u32>().wrapping_add(1) & 0xFFFF) as u16
}

pub fn search_packet(pc_mac: [u8; 6], pc_ip: Ipv4Addr) -> Vec<u8> {
    let mut p = header(T_SEARCH, [0; 6], pc_mac, pc_ip, [0, 0], [0, 0, 0, 1]);
    let ask: Vec<u8> = ASK.iter().flat_map(|a| a.to_be_bytes()).collect();
    tlv(&mut p, 0xFFF0, &ask);
    finish(p)
}

/// What a set carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetFields {
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns: [Ipv4Addr; 2],
    pub port: u16,
}

pub fn set_packet(cam: [u8; 6], pc_mac: [u8; 6], pc_ip: Ipv4Addr, f: &SetFields, mode: u8, creds: Option<&Credentials>) -> Vec<u8> {
    let mut p = header(T_SET, cam, pc_mac, pc_ip, [0, 1], [0, 0, 2, 0]);
    let port = f.port.to_be_bytes();
    let mut dns = f.dns[0].octets().to_vec();
    dns.extend_from_slice(&f.dns[1].octets());
    tlv(&mut p, 0x0000, &[mode]);
    tlv(&mut p, 0x0001, &[0x02]);
    tlv(&mut p, 0x0020, &f.ip.octets());
    tlv(&mut p, 0x0021, &f.mask.octets());
    tlv(&mut p, 0x0022, &f.gateway.octets());
    tlv(&mut p, 0x0023, &dns);
    tlv(&mut p, 0x0025, &port);
    // The IPv6 block goes out zeroed: Panasonic's own tool sends it that way,
    // and echoing a camera's own values back made a UE160 ack without applying.
    tlv(&mut p, 0x0040, &[0u8; 16]);
    tlv(&mut p, 0x0041, &[0u8; 16]);
    tlv(&mut p, 0x0042, &[0u8; 32]);
    tlv(&mut p, 0x0044, &port);
    tlv(&mut p, 0x00A0, &f.ip.octets());
    tlv(&mut p, 0x00A1, &f.mask.octets());
    tlv(&mut p, 0x00A2, &f.gateway.octets());
    tlv(&mut p, 0x00A3, &dns);
    tlv(&mut p, 0x00A6, &[0x92]);
    if let Some(c) = creds.filter(|c| !c.username.is_empty()) {
        tlv(&mut p, 0x00B7, &cstr(&c.username));
        tlv(&mut p, 0x00B8, &cstr(&c.password));
    }
    finish(p)
}

pub fn confirm_packet(cam: [u8; 6], pc_mac: [u8; 6], pc_ip: Ipv4Addr) -> Vec<u8> {
    finish(header(T_CONFIRM, cam, pc_mac, pc_ip, [0, 2], [0, 0, 2, 0]))
}

fn cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// A parsed Easy IP datagram.
#[derive(Clone, Debug)]
pub struct Frame {
    pub kind: u16,
    pub cam_mac: [u8; 6],
    pub pc_mac: [u8; 6],
    pub pc_ip: Ipv4Addr,
    pub b22: [u8; 2],
    pub b33: [u8; 4],
    pub tlvs: BTreeMap<u16, Vec<u8>>,
    pub checksum_ok: bool,
}

impl Frame {
    pub fn parse(b: &[u8]) -> Option<Frame> {
        if b.len() < 52 || b[0..2] != [0x00, 0x01] || b[24..33] != MAGIC {
            return None;
        }
        let n = b.len();
        let mut tlvs = BTreeMap::new();
        let mut i = 48;
        while i + 4 <= n - 4 {
            let id = u16::from_be_bytes([b[i], b[i + 1]]);
            if id == 0xFFFF {
                break;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            let end = i + 4 + len;
            if end > n - 2 {
                break;
            }
            tlvs.insert(id, b[i + 4..end].to_vec());
            i = end;
        }
        Some(Frame {
            kind: u16::from_be_bytes([b[4], b[5]]),
            cam_mac: b[6..12].try_into().ok()?,
            pc_mac: b[12..18].try_into().ok()?,
            pc_ip: Ipv4Addr::new(b[18], b[19], b[20], b[21]),
            b22: [b[22], b[23]],
            b33: [b[33], b[34], b[35], b[36]],
            tlvs,
            checksum_ok: checksum(&b[..n - 2]) == u16::from_be_bytes([b[n - 2], b[n - 1]]),
        })
    }

    pub fn ip4(&self, id: u16) -> Option<Ipv4Addr> {
        let v = self.tlvs.get(&id)?;
        (v.len() >= 4).then(|| Ipv4Addr::new(v[0], v[1], v[2], v[3]))
    }

    pub fn u16(&self, id: u16) -> Option<u16> {
        let v = self.tlvs.get(&id)?;
        (v.len() >= 2).then(|| u16::from_be_bytes([v[0], v[1]]))
    }

    pub fn text(&self, id: u16) -> Option<String> {
        let v = self.tlvs.get(&id)?;
        let end = v.iter().position(|b| *b == 0).unwrap_or(v.len());
        let s = String::from_utf8_lossy(&v[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Ack status (bytes 35-36): 0x0200 and 0x0202 both mean accepted.
    pub fn status(&self) -> u16 {
        u16::from_be_bytes([self.b33[2], self.b33[3]])
    }
}

fn looks_like_model(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    ["AW-", "AK-", "AG-", "AJ-", "WV-"].iter().any(|p| u.starts_with(p))
}

/// The camera a search reply describes.
pub fn camera_from_reply(f: &Frame) -> Option<Camera> {
    let ip = f.ip4(0x0020)?;
    let mut cam = Camera::new(ID, "Panasonic", ip);
    cam.mac = Some(Mac(f.cam_mac));
    // 00A7 and 00A8 are the model and the title, and implementations disagree
    // which is which; the one that reads like a model code is the model.
    let (a7, a8) = (f.text(0x00A7), f.text(0x00A8));
    match (&a7, &a8) {
        (Some(x), y) if looks_like_model(x) => {
            cam.model = Some(x.clone());
            cam.name = y.clone();
        }
        (x, Some(y)) if looks_like_model(y) => {
            cam.model = Some(y.clone());
            cam.name = x.clone();
        }
        (x, y) => {
            cam.model = x.clone();
            cam.name = y.clone();
        }
    }
    let port = f.u16(0x0025).filter(|p| *p != 0).unwrap_or(80);
    cam.web_port = Some(port);
    let prefix = f.ip4(0x0021).and_then(mask_to_prefix).unwrap_or(24);
    let dns = f.tlvs.get(&0x0023).map(|v| v.chunks(4).filter(|c| c.len() == 4 && c != &[0, 0, 0, 0]).map(|c| Ipv4Addr::new(c[0], c[1], c[2], c[3])).collect()).unwrap_or_default();
    cam.net = Some(NetConfig { dhcp: false, ip, prefix, gateway: f.ip4(0x0022).filter(|g| !g.is_unspecified()), dns, hostname: None });
    cam.caps = Caps { read_network: true, write_network: true, write_by_mac: true, dns: true, ..Caps::default() };
    if let Some(v) = f.text(0x00A9) {
        cam.fields.insert("networkVersion".into(), v);
    }
    if let Some(m) = f.tlvs.get(&0x0000).and_then(|v| v.first()) {
        cam.fields.insert("easyIpMode".into(), format!("{m:#04x}"));
    }
    if f.ip4(0x0020) != f.ip4(0x00A0) && f.tlvs.contains_key(&0x00A0) {
        cam.fields.insert("warning".into(), "the reply's primary and duplicate address fields disagree".into());
    }
    if !f.checksum_ok {
        cam.fields.insert("checksum".into(), "bad (accepted anyway)".into());
    }
    Some(cam)
}

/// Panasonic's relaxed JSON: a trailing comma after every member of every
/// object and array, then FF FF and the same checksum as the binary frames.
pub fn json_datagram(v: &Value) -> Vec<u8> {
    fn enc(v: &Value, out: &mut String) {
        match v {
            Value::Object(m) => {
                out.push('{');
                for (k, x) in m {
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    enc(x, out);
                    out.push(',');
                }
                if m.is_empty() {
                    out.push(',');
                }
                out.push('}');
            }
            Value::Array(a) => {
                out.push('[');
                for x in a {
                    enc(x, out);
                    out.push(',');
                }
                out.push(']');
            }
            x => out.push_str(&x.to_string()),
        }
    }
    let mut s = String::new();
    enc(v, &mut s);
    let mut b = s.into_bytes();
    b.extend_from_slice(&[0xFF, 0xFF]);
    let ck = checksum(&b);
    b.extend_from_slice(&ck.to_be_bytes());
    b
}

pub fn parse_json_datagram(b: &[u8]) -> Option<Value> {
    if b.len() < 6 || b[b.len() - 4..b.len() - 2] != [0xFF, 0xFF] {
        return None;
    }
    let text = std::str::from_utf8(&b[..b.len() - 4]).ok()?;
    // Drop the trailing commas so a strict parser takes it.
    let mut strict = String::with_capacity(text.len());
    let mut in_str = false;
    let mut esc = false;
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            strict.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        if c == ',' {
            let next = chars[i + 1..].iter().find(|x| !x.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        strict.push(c);
    }
    serde_json::from_str(&strict).ok()
}

pub fn access_number(c: &Credentials, random: &str) -> String {
    let h = Md5::digest(format!("{}:{}:{}", c.username, c.password, random).as_bytes());
    h.iter().map(|b| format!("{b:02X}")).collect()
}

fn app_info(op: &str) -> Value {
    json!({ "app_version": "1.5.6", "app_operation": op })
}

/// One socket per leg, bound to that interface's address on the PC port so
/// a camera that answers the source port rather than broadcasting still
/// reaches us.
fn leg_socket(leg: &Leg, port: u16) -> std::io::Result<UdpSocket> {
    let opts = UdpOpts { broadcast: true, reuse: true, iface_index: leg.index, multicast_ttl: None };
    udp_socket(SocketAddrV4::new(leg.bind, port), opts).or_else(|_| udp_socket(SocketAddrV4::new(leg.bind, 0), opts))
}

fn macs_for(legs: &[Leg]) -> std::collections::HashMap<String, Mac> {
    let _ = legs;
    net::iface_macs()
}

#[async_trait]
impl Driver for Panasonic {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: ID,
            vendor: "Panasonic",
            protocol: "Easy IP Setup",
            priority: 70,
            discovery: vec!["UDP broadcast to 10670, answers by broadcast to 10669 — finds cameras on any address on the same wire"],
            sweep: Some("HTTP GET /cgi-bin/aw_cam?cmd=QID (model) and /cgi-bin/getinfo?FILE=1 (MAC)"),
            notes: vec![
                "Cameras accept new settings only in the first 20 minutes after power-on, unless the camera's \"Easy IP Setup accommodate period\" is Unlimited. No answer to a change usually means the window has closed: power-cycle the camera and try again.",
                "Easy IP is layer 2: it re-addresses a camera on the wrong subnet, but cannot cross a router.",
                "Cameras with user authentication on need their admin login. UE160-generation cameras with authentication on use an encrypted variant Meerkat does not speak: use the web UI or switch authentication off first.",
                "Switching a camera to DHCP is not possible over Easy IP; set it in the web UI.",
            ],
            enabled_by_default: true,
            recovery: None,
        }
    }

    async fn discover(&self, ctx: &ScanCtx) -> Result<()> {
        let pc_port = ctx.port("pc", PC_PORT);
        let cam_port = ctx.port("camera", CAMERA_PORT);
        let mut socks: Vec<Arc<UdpSocket>> = Vec::new();
        // The shared listener: replies are broadcasts to the PC port.
        match udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, pc_port), UdpOpts { broadcast: true, reuse: true, ..Default::default() }) {
            Ok(s) => socks.push(Arc::new(s)),
            Err(e) => ctx.log(Level::Warn, format!("cannot listen on UDP {pc_port} ({e}); only cameras that answer the sending port will show")),
        }
        let macs = macs_for(&ctx.legs);
        let mut sends = Vec::new();
        for leg in &ctx.legs {
            match leg_socket(leg, pc_port) {
                Ok(s) => {
                    let s = Arc::new(s);
                    let mac = macs.get(&leg.iface).map(|m| m.octets()).unwrap_or([0; 6]);
                    sends.push((s.clone(), leg.clone(), search_packet(mac, if leg.bind.is_unspecified() { Ipv4Addr::UNSPECIFIED } else { leg.bind })));
                    socks.push(s);
                }
                Err(e) => ctx.log(Level::Warn, format!("{}: cannot open a socket on {} ({e})", leg.iface, leg.bind)),
            }
        }
        let mut inbox = Inbox::new(&socks);
        // Twice, 300 ms apart: broadcasts are not retransmitted.
        for round in 0..2 {
            for (s, leg, pkt) in &sends {
                for d in &leg.dests {
                    if let Err(e) = s.send_to(pkt, ctx.to(*d, cam_port)).await {
                        if round == 0 {
                            ctx.log(Level::Debug, format!("{}: send to {d} failed: {e}", leg.iface));
                        }
                    }
                }
            }
            if round == 0 {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
        let deadline = Instant::now() + ctx.listen;
        let mut seen = HashSet::new();
        while let Some((b, from)) = tokio::select! { x = inbox.next_until(deadline) => x, _ = ctx.cancel.cancelled() => None } {
            let Some(f) = Frame::parse(&b) else { continue };
            if f.kind != T_REPLY && f.kind != 0x0001 {
                continue;
            }
            if !seen.insert(f.cam_mac) {
                continue;
            }
            let Some(cam) = camera_from_reply(&f) else { continue };
            let via = via_for(ctx, &f, from);
            ctx.found(cam, Method::Broadcast, &via);
        }
        Ok(())
    }

    fn can_probe(&self) -> bool {
        true
    }

    async fn probe(&self, ctx: &ScanCtx, ip: Ipv4Addr) -> Result<Option<Camera>> {
        let port = ctx.port("http", 80);
        let base = if port == 80 { format!("http://{ip}") } else { format!("http://{ip}:{port}") };
        let r = match ctx.http.get(&format!("{base}/cgi-bin/aw_cam?cmd=QID&res=1"), None).await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        let body = r.body.trim();
        let Some(model) = body.strip_prefix("OID:") else { return Ok(None) };
        let mut cam = Camera::new(ID, "Panasonic", ip);
        cam.model = Some(model.trim().to_string());
        cam.web_port = Some(port);
        if let Ok(info) = ctx.http.get(&format!("{base}/cgi-bin/getinfo?FILE=1"), None).await {
            for line in info.body.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim();
                    match k.trim() {
                        "MAC" => cam.mac = v.parse().ok(),
                        "SERIAL" => cam.serial = Some(v.to_string()).filter(|s| !s.is_empty()),
                        "VERSION" => cam.firmware = Some(v.to_string()).filter(|s| !s.is_empty()),
                        "NAME" if cam.model.as_deref() != Some(v) => cam.name = Some(v.to_string()).filter(|s| !s.is_empty()),
                        _ => {}
                    }
                }
            }
        }
        if cam.firmware.is_none() {
            if let Ok(r) = ctx.http.get(&format!("{base}/cgi-bin/aw_cam?cmd=QSV&res=1"), None).await {
                cam.firmware = r.body.trim().strip_prefix("OSV:").map(|s| s.trim().to_string());
            }
        }
        if let Ok(r) = ctx.http.get(&format!("{base}/cgi-bin/aw_ptz?cmd=%23O&res=1"), None).await {
            let p = match r.body.trim() {
                "p0" => "standby",
                "p1" => "on",
                "p3" => "switching on",
                _ => "",
            };
            if !p.is_empty() {
                cam.fields.insert("power".into(), p.into());
            }
        }
        // Over a router this driver can identify the camera but not re-address
        // it: Easy IP is layer 2. The engine refuses writes without a broadcast sighting.
        cam.caps = Caps { write_by_mac: true, ..Caps::default() };
        Ok(Some(cam))
    }

    async fn read_network(&self, _op: &OpCtx, cam: &Camera) -> Result<NetConfig> {
        // Easy IP's search reply carries the settings; a rescan refreshes them.
        cam.net.clone().ok_or(DriverError::Unsupported("read the settings of a camera only reached over a router"))
    }

    async fn write_network(&self, op: &OpCtx, cam: &Camera, cfg: &NetConfig) -> Result<WriteOutcome> {
        if cfg.dhcp {
            return Err(DriverError::Unsupported("switch a camera to DHCP over Easy IP — use its web UI"));
        }
        let mac = cam.mac.ok_or_else(|| DriverError::Invalid("no MAC address for this camera; rescan".into()))?.octets();
        let fields = SetFields {
            ip: cfg.ip,
            mask: prefix_to_mask(cfg.prefix),
            gateway: cfg.gateway.unwrap_or(Ipv4Addr::UNSPECIFIED),
            dns: [cfg.dns.first().copied().unwrap_or(Ipv4Addr::UNSPECIFIED), cfg.dns.get(1).copied().unwrap_or(Ipv4Addr::UNSPECIFIED)],
            port: cam.web_port.unwrap_or(80),
        };
        let pc_port = op.port("pc", PC_PORT);
        let cam_port = op.port("camera", CAMERA_PORT);
        let legs = if op.legs.is_empty() { return Err(DriverError::Unreachable("no interface to send on".into())) } else { op.legs.clone() };
        let macs = macs_for(&legs);
        let creds = op.creds.clone().filter(|c| !c.username.is_empty());

        if let Some(c) = &creds {
            match json_stage(op, &legs[0], macs.get(&legs[0].iface).copied(), mac, c, &fields).await? {
                JsonStage::Accepted | JsonStage::NotSpoken => {}
                JsonStage::Refused => return Err(DriverError::Protocol("the camera did not accept that username and password".into())),
            }
        }

        let listener = udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, pc_port), UdpOpts { broadcast: true, reuse: true, ..Default::default() }).ok().map(Arc::new);
        let unicast = net::local_for(&op.ifaces, cam.ip).is_some();
        let mut statuses = Vec::new();
        for mode in [0x03u8, 0x02] {
            let mut socks: Vec<Arc<UdpSocket>> = listener.iter().cloned().collect();
            let mut sends = Vec::new();
            for leg in &legs {
                let s = Arc::new(leg_socket(leg, pc_port)?);
                let pc_mac = macs.get(&leg.iface).map(|m| m.octets()).unwrap_or([0; 6]);
                sends.push((s.clone(), leg.clone(), pc_mac, set_packet(mac, pc_mac, leg.bind, &fields, mode, creds.as_ref())));
                socks.push(s);
            }
            let mut inbox = Inbox::new(&socks);
            for (s, leg, _, pkt) in &sends {
                let mut dests: Vec<Ipv4Addr> = leg.dests.clone();
                if unicast {
                    dests.push(cam.ip);
                }
                for d in dests {
                    for port in [pc_port, cam_port] {
                        let _ = s.send_to(pkt, op.to(d, port)).await;
                    }
                }
            }
            let deadline = Instant::now() + Duration::from_millis(2500);
            let mut acked = false;
            while let Some((b, _)) = inbox.next_until(deadline).await {
                if let Some(f) = Frame::parse(&b) {
                    if f.kind == T_ACK && f.cam_mac == mac {
                        statuses.push(f.status());
                        if matches!(f.status(), 0x0200 | 0x0202) {
                            acked = true;
                            break;
                        }
                    }
                }
            }
            if acked {
                for (s, leg, pc_mac, _) in &sends {
                    let pkt = confirm_packet(mac, *pc_mac, leg.bind);
                    for d in leg.dests.iter().copied().chain(unicast.then_some(cam.ip)) {
                        let _ = s.send_to(&pkt, op.to(d, cam_port)).await;
                    }
                }
            }
        }
        if statuses.iter().any(|s| matches!(s, 0x0200 | 0x0202)) {
            Ok(WriteOutcome {
                message: format!("The camera accepted {}/{}", cfg.ip, cfg.prefix),
                restarts: true,
                expect_ip: Some(cfg.ip),
                settle_ms: 3000,
            })
        } else if let Some(s) = statuses.first() {
            Err(DriverError::Protocol(format!("the camera answered with status {s:#06x} and did not take the change")))
        } else {
            Err(DriverError::Protocol(
                "no answer from the camera. Panasonic cameras accept Easy IP changes only in the first 20 minutes after power-on (unless the camera's \"Easy IP Setup accommodate period\" is Unlimited) — power-cycle it and try again within 20 minutes".into(),
            ))
        }
    }

    fn web_url(&self, cam: &Camera) -> Option<String> {
        Some(match cam.web_port {
            Some(p) if p != 80 => format!("http://{}:{p}/", cam.ip),
            _ => format!("http://{}/", cam.ip),
        })
    }
}

/// Which interface a reply belongs to: the leg whose address it echoes, else
/// the leg whose subnet holds the sender, else the only leg.
fn via_for(ctx: &ScanCtx, f: &Frame, from: SocketAddr) -> String {
    if let Some(l) = ctx.legs.iter().find(|l| l.bind == f.pc_ip && !l.bind.is_unspecified()) {
        return l.iface.clone();
    }
    if let SocketAddr::V4(v4) = from {
        if let Some(i) = net::local_for(&ctx.ifaces, *v4.ip()) {
            return i.name.clone();
        }
    }
    ctx.legs.first().map(|l| l.iface.clone()).unwrap_or_else(|| "?".into())
}

enum JsonStage {
    Accepted,
    Refused,
    NotSpoken,
}

/// The authenticated JSON stage. `NotSpoken` when the camera never answers
/// the lookup (older models, or authentication off): the binary set follows
/// with the credentials in TLVs 00B7/00B8 instead.
async fn json_stage(op: &OpCtx, leg: &Leg, pc_mac: Option<Mac>, cam: [u8; 6], c: &Credentials, f: &SetFields) -> Result<JsonStage> {
    let pc_port = op.port("json_pc", JSON_PC_PORT);
    let cam_port = op.port("json_camera", JSON_CAMERA_PORT);
    let sock = udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, pc_port), UdpOpts { broadcast: true, reuse: true, iface_index: leg.index, ..Default::default() })?;
    let cam_s = Mac(cam).to_upper("-");
    let pc_s = pc_mac.unwrap_or_default().to_upper("-");
    let server = json!({ "server_mac": pc_s, "server_ip": leg.bind.to_string() });
    let send = |v: Value| {
        let pkt = json_datagram(&v);
        let dests = leg.dests.clone();
        let sock = &sock;
        async move {
            for d in dests {
                let _ = sock.send_to(&pkt, op.to(d, cam_port)).await;
            }
        }
    };
    send(json!({ "app_info": app_info("Lookup"), "server_info": server })).await;
    send(json!({ "app_info": app_info("Req-Pub"), "client_info": { "client_mac": cam_s }, "server_info": server })).await;
    let Some(inform) = wait_for(&sock, &cam_s, "Inform", Instant::now() + Duration::from_secs(2)).await else { return Ok(JsonStage::NotSpoken) };
    let ci = &inform["client_info"];
    let random = ci["client_random_number"].as_str().unwrap_or("").trim().to_string();
    let update = ci["client_update_time"].as_str().unwrap_or("").trim().to_string();
    if random.is_empty() || update.is_empty() {
        return Ok(JsonStage::NotSpoken);
    }
    let s = |k: &str| ci[k].as_str().unwrap_or("").to_string();
    let setup = json!({
        "app_info": app_info("Setup"),
        "client_info": {
            "client_mac": cam_s, "client_model": s("client_model"), "client_name": s("client_name"),
            "client_firmware_ver": s("client_firmware_ver"),
            "client_status": if s("client_status").is_empty() { "PowerOn".to_string() } else { s("client_status") },
            "client_update_time": update, "client_random_number": random,
        },
        "server_info": { "server_mac": pc_s, "server_ip": leg.bind.to_string(), "access_number": access_number(c, &random) },
        "general_network": { "http_port": f.port.to_string(), "https_port": "443" },
        "ipv4_info": { "ipv4_dhcp": "off", "ipv4_addr": f.ip.to_string(), "ipv4_prefix": f.mask.to_string(), "ipv4_gateway": f.gateway.to_string() },
        "ipv6_info": { "ipv6_auto": "on", "ipv6_addr": "::", "ipv6_gateway": "::", "ipv6_dhcp": "off" },
        "dns_info": { "dns_auto": "off", "dns_primary_server": f.dns[0].to_string(), "dns_secondary_server": f.dns[1].to_string() },
    });
    send(setup).await;
    if wait_for(&sock, &cam_s, "Ack-Client", Instant::now() + Duration::from_millis(1600)).await.is_none() {
        return Ok(JsonStage::Refused);
    }
    send(json!({ "app_info": app_info("Ack-Server"), "client_info": { "client_mac": cam_s }, "server_info": server })).await;
    Ok(JsonStage::Accepted)
}

/// The next JSON-stage message from camera `cam` with operation `op`.
async fn wait_for(sock: &UdpSocket, cam: &str, op: &str, until: Instant) -> Option<Value> {
    let mut buf = vec![0u8; 4096];
    loop {
        let Ok(Ok((n, _))) = tokio::time::timeout_at(until, sock.recv_from(&mut buf)).await else { return None };
        if let Some(v) = parse_json_datagram(&buf[..n]) {
            if v["client_info"]["client_mac"].as_str() == Some(cam) && v["app_info"]["app_operation"].as_str() == Some(op) {
                return Some(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Easy IP Setup's own search request, captured 31 March 2018 by
    /// dPro-Software (PC 00:1c:42:4b:bb:f8 at 169.254.224.14).
    const CAPTURED_SEARCH: &str = "0001002A000D000000000000001C424BBBF8A9FEE00E000020111E11231F1E1913000000010000000000000000000000FFF00026002000210022002300250028004000410042004400A500A600A700A800AD00B300B400B700B8FFFF1221";

    /// An AW-HE130's reply to it (192.168.0.10/24, gateway .1), same capture set.
    const CAPTURED_REPLY: &str = "00010175000ea8137476a86b001c424bbbf8a9fee00e000020111e11231f1e19130000020200000000000000000000000000000102000100010000200004c0a8000a00210004ffffff0000220004c0a8000100230008000000000000000000250002005000400010fe80000000000000aa1374fffe76a86b004100100000000000000000000000000000000000420020000000000000000000000000000000000000000000000000000000000000000000440002005000450002004000a00004c0a8000a00a10004ffffff0000a20004c0a8000100a30008000000000000000000a400047fffffff00a600019200a7001041572d4845313330000000000000000000a8001043414d3a48453133304b00000000000000a9001030322e3030303000000000000000000000aa0010302e303000000000000000000000000000b100280000000000000000000000000000000000000000000000000000000000000000000000000000000000b200280000000000000000000000000000000000000000000000000000000000000000000000000000000000b40002000000b500010000b6000192ffff35e4";

    #[test]
    fn search_matches_easy_ip_setup_byte_for_byte() {
        let p = search_packet([0x00, 0x1c, 0x42, 0x4b, 0xbb, 0xf8], Ipv4Addr::new(169, 254, 224, 14));
        assert_eq!(p, unhex(CAPTURED_SEARCH));
        assert_eq!(p.len(), 94);
    }

    #[test]
    fn parses_a_real_he130_reply() {
        let f = Frame::parse(&unhex(CAPTURED_REPLY)).unwrap();
        assert!(f.checksum_ok);
        assert_eq!(f.kind, T_REPLY);
        assert_eq!(f.pc_ip, Ipv4Addr::new(169, 254, 224, 14));
        let c = camera_from_reply(&f).unwrap();
        assert_eq!(c.mac.unwrap().to_string(), "a8:13:74:76:a8:6b");
        assert_eq!(c.ip, Ipv4Addr::new(192, 168, 0, 10));
        assert_eq!(c.model.as_deref(), Some("AW-HE130"));
        assert_eq!(c.name.as_deref(), Some("CAM:HE130K"));
        assert_eq!(c.web_port, Some(80));
        let n = c.net.unwrap();
        assert_eq!((n.prefix, n.gateway), (24, Some(Ipv4Addr::new(192, 168, 0, 1))));
        assert!(n.dns.is_empty());
        assert_eq!(c.fields["networkVersion"], "02.0000");
        assert!(c.caps.write_by_mac);
    }

    #[test]
    fn set_and_confirm_have_the_ue160_shape() {
        let f = SetFields { ip: Ipv4Addr::new(10, 0, 0, 21), mask: Ipv4Addr::new(255, 255, 255, 0), gateway: Ipv4Addr::new(10, 0, 0, 1), dns: [Ipv4Addr::UNSPECIFIED; 2], port: 80 };
        let p = set_packet([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2], Ipv4Addr::new(10, 0, 0, 5), &f, 3, None);
        let fr = Frame::parse(&p).unwrap();
        assert!(fr.checksum_ok);
        assert_eq!(fr.kind, T_SET);
        assert_eq!(&p[22..24], &[0, 1]);
        assert_eq!(&p[33..37], &[0, 0, 2, 0]);
        // The fixed 58 bytes smart-reset-browser builds: TLV 0000 = mode, 0001 = 02.
        assert_eq!(&p[48..58], &[0, 0, 0, 1, 3, 0, 1, 0, 1, 2]);
        assert_eq!(fr.ip4(0x0020), Some(f.ip));
        assert_eq!(fr.ip4(0x00A0), Some(f.ip));
        assert_eq!(fr.u16(0x0044), Some(80));
        assert_eq!(u16::from_be_bytes([p[2], p[3]]) as usize, p.len() - 52);
        let c = confirm_packet([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2], Ipv4Addr::new(10, 0, 0, 5));
        assert_eq!(c.len(), 52);
        assert_eq!(&c[2..6], &[0, 0, 0, 4]);
        assert_eq!(&c[22..24], &[0, 2]);
        let creds = Credentials { username: "admin".into(), password: "12345".into() };
        let pa = Frame::parse(&set_packet([2, 0, 0, 0, 0, 1], [0; 6], Ipv4Addr::UNSPECIFIED, &f, 2, Some(&creds))).unwrap();
        assert_eq!(pa.text(0x00B7).as_deref(), Some("admin"));
    }

    #[test]
    fn relaxed_json_round_trips() {
        let v = json!({ "app_info": { "app_operation": "Inform" }, "client_info": { "client_mac": "AA-BB", "list": [1, 2] } });
        let d = json_datagram(&v);
        let s = std::str::from_utf8(&d[..d.len() - 4]).unwrap();
        assert!(s.contains(r#""app_operation":"Inform",}"#), "{s}");
        assert!(s.contains("[1,2,]"));
        assert_eq!(parse_json_datagram(&d).unwrap(), v);
        let c = Credentials { username: "admin".into(), password: "pass".into() };
        assert_eq!(access_number(&c, "1234"), access_number(&c, "1234").to_uppercase());
        assert_eq!(access_number(&c, "1234").len(), 32);
    }
}
