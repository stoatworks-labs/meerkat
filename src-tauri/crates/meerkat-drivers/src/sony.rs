//! Sony — the "Camera IP Setting Command" on UDP 52380 (what Sony's IP Setup
//! Software and RM-IP controllers use), plus a VISCA-over-IP version inquiry
//! for routed sweeps.
//!
//! Source: Sony VISCA Command List E-042-100-13 v2.10 (BRC-X400/X401,
//! SRG-X400/X402/201M2, SRG-X120/HD1M2) p.41; the BRC-AM7 and ILME-FR7 lists
//! carry the same section. Every message is `0x02`, ASCII `KEY:value` fields
//! each followed by `0xFF`, then `0x03`, and every one goes to the limited
//! broadcast on 52380 — so it reaches a camera on any address on the wire.
//!
//! ```text
//! inquiry   02 "ENQ:network" FF 03
//! reply     02 MAC:..FF [INFO:network FF] MODEL:IPCARD FF SOFTVERSION:..FF IPADR:..FF MASK:..FF GATEWAY:..FF NAME:..FF WRITE:on FF 03
//! set       02 MAC:<target> FF IPADR:..FF MASK:..FF GATEWAY:..FF NAME:..FF 03
//! answer    02 ACK:<mac> FF .. 03   (or NAK:<mac>)
//! ```
//!
//! Sets are keyed by MAC and need no password, but a camera refuses them
//! while its reply says `WRITE:off` — on AM7 and FR7 that happens by itself
//! 20 minutes after power-on. `NAME` is at most 8 letters, digits or spaces.
//! DHCP cannot be switched on this way.

use std::collections::{BTreeMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::driver::{Leg, Level, Result};
use meerkat_core::net::{self, mask_to_prefix, prefix_to_mask, udp_socket, UdpOpts};
use meerkat_core::{Caps, Camera, Driver, DriverError, DriverInfo, Mac, Method, NetConfig, OpCtx, ScanCtx, WriteOutcome};
use tokio::net::UdpSocket;
use tokio::time::Instant;

use crate::udp::Inbox;

pub struct Sony;

pub const ID: &str = "sony";
pub const SETUP_PORT: u16 = 52380;
pub const VISCA_PORT: u16 = 52381;

/// Frame `KEY:value` fields the Sony way.
pub fn frame(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut p = vec![0x02];
    for (k, v) in fields {
        p.extend_from_slice(k.as_bytes());
        p.push(b':');
        p.extend_from_slice(v.as_bytes());
        p.push(0xFF);
    }
    p.push(0x03);
    p
}

pub fn inquiry() -> Vec<u8> {
    frame(&[("ENQ", "network")])
}

/// The fields of a Sony datagram in order (keys upper-cased). None if it is
/// not one.
pub fn parse(b: &[u8]) -> Option<Vec<(String, String)>> {
    if b.len() < 3 || b[0] != 0x02 {
        return None;
    }
    let end = b.iter().rposition(|x| *x == 0x03)?;
    let mut out = Vec::new();
    for part in b[1..end].split(|x| *x == 0xFF) {
        if part.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(part);
        match s.split_once(':') {
            Some((k, v)) => out.push((k.trim().to_ascii_uppercase(), v.trim().to_string())),
            None => out.push((s.trim().to_ascii_uppercase(), String::new())),
        }
    }
    (!out.is_empty()).then_some(out)
}

pub fn camera_from_reply(fields: &[(String, String)]) -> Option<Camera> {
    let m: BTreeMap<&str, &str> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    if m.contains_key("ENQ") || m.contains_key("ACK") || m.contains_key("NAK") {
        return None;
    }
    let mac: Mac = m.get("MAC")?.parse().ok()?;
    let ip: Ipv4Addr = m.get("IPADR")?.parse().ok()?;
    let mut cam = Camera::new(ID, "Sony", ip);
    cam.mac = Some(mac);
    cam.name = m.get("NAME").map(|s| s.to_string()).filter(|s| !s.is_empty());
    cam.firmware = m.get("SOFTVERSION").map(|s| s.to_string());
    cam.web_port = Some(80);
    let prefix = m.get("MASK").and_then(|x| x.parse().ok()).and_then(mask_to_prefix).unwrap_or(24);
    let gateway = m.get("GATEWAY").and_then(|g| g.parse::<Ipv4Addr>().ok()).filter(|g| !g.is_unspecified());
    cam.net = Some(NetConfig { dhcp: false, ip, prefix, gateway, dns: vec![], hostname: cam.name.clone() });
    let writable = m.get("WRITE").map(|w| !w.eq_ignore_ascii_case("off")).unwrap_or(true);
    cam.caps = Caps { read_network: true, write_network: writable, write_by_mac: true, hostname: true, ..Caps::default() };
    // MODEL is the network board ("IPCARD"), not the camera; keep it as a field.
    for (k, label) in [("MODEL", "networkBoard"), ("WRITE", "write"), ("INFO", "info")] {
        if let Some(v) = m.get(k) {
            cam.fields.insert(label.into(), v.to_string());
        }
    }
    if !m.contains_key("GATEWAY") {
        cam.fields.insert("gatewayField".into(), "absent (older model: the gateway is not sent in a set)".into());
    }
    if !writable {
        cam.fields.insert("writeNote".into(), "IP setup is locked (WRITE:off). It unlocks for 20 minutes after power-on, unless disabled on the camera".into());
    }
    Some(cam)
}

/// Keep a name to what Sony accepts: 8 characters, letters, digits, spaces.
pub fn clean_name(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').take(8).collect()
}

/// VISCA-over-IP CAM_VersionInq, sequence `seq`.
pub fn visca_version_inquiry(seq: u32) -> Vec<u8> {
    let mut p = vec![0x01, 0x10, 0x00, 0x05];
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(&[0x81, 0x09, 0x00, 0x02, 0xFF]);
    p
}

/// `(vendor, model, rom)` from a CAM_VersionInq reply (`90 50 vv vv mm mm rr rr ..`).
pub fn parse_visca_version(b: &[u8]) -> Option<(u16, u16, u16)> {
    let p = if b.len() >= 8 && b[0] == 0x01 && b[1] == 0x11 { &b[8..] } else { b };
    let i = p.windows(2).position(|w| w[0] == 0x90 && w[1] == 0x50)?;
    let r = &p[i + 2..];
    (r.len() >= 6).then(|| (u16::from_be_bytes([r[0], r[1]]), u16::from_be_bytes([r[2], r[3]]), u16::from_be_bytes([r[4], r[5]])))
}

fn leg_socket(leg: &Leg) -> std::io::Result<UdpSocket> {
    udp_socket(SocketAddrV4::new(leg.bind, 0), UdpOpts { broadcast: true, reuse: true, iface_index: leg.index, multicast_ttl: None })
}

fn listener(port: u16) -> Option<Arc<UdpSocket>> {
    udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port), UdpOpts { broadcast: true, reuse: true, ..Default::default() }).ok().map(Arc::new)
}

#[async_trait]
impl Driver for Sony {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: ID,
            vendor: "Sony",
            protocol: "Sony IP Setup (VISCA over IP)",
            priority: 70,
            discovery: vec!["UDP broadcast \"ENQ:network\" to 52380 — finds cameras on any address on the same wire"],
            sweep: Some("UDP \"ENQ:network\" to <ip>:52380 and a VISCA version inquiry to <ip>:52381"),
            notes: vec![
                "No password is needed to change the address, but the camera must allow it: on BRC-AM7 and ILME-FR7 IP setup locks itself 20 minutes after power-on.",
                "The camera name is at most 8 letters, digits or spaces.",
                "Switching a camera to DHCP is not possible this way; use its web UI.",
                "Layer 2 only: it re-addresses a camera on the wrong subnet, but cannot cross a router.",
            ],
            enabled_by_default: true,
            recovery: None,
        }
    }

    async fn discover(&self, ctx: &ScanCtx) -> Result<()> {
        let port = ctx.port("setup", SETUP_PORT);
        // Cameras answer on the same port they listen on; tests split the two.
        let reply = ctx.port("reply", SETUP_PORT);
        let mut socks: Vec<Arc<UdpSocket>> = Vec::new();
        match listener(reply) {
            Some(s) => socks.push(s),
            None => ctx.log(Level::Warn, format!("cannot listen on UDP {reply}; Sony cameras will not show (is Sony's IP Setup Software running?)")),
        }
        let mut sends = Vec::new();
        for leg in &ctx.legs {
            match leg_socket(leg) {
                Ok(s) => {
                    let s = Arc::new(s);
                    socks.push(s.clone());
                    sends.push((s, leg.clone()));
                }
                Err(e) => ctx.log(Level::Warn, format!("{}: {e}", leg.iface)),
            }
        }
        let mut inbox = Inbox::new(&socks);
        let pkt = inquiry();
        for round in 0..2 {
            for (s, leg) in &sends {
                for d in &leg.dests {
                    let _ = s.send_to(&pkt, ctx.to(*d, port)).await;
                }
            }
            if round == 0 {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
        let deadline = Instant::now() + ctx.listen;
        let mut seen = HashSet::new();
        while let Some((b, from)) = tokio::select! { x = inbox.next_until(deadline) => x, _ = ctx.cancel.cancelled() => None } {
            let Some(fields) = parse(&b) else { continue };
            let Some(cam) = camera_from_reply(&fields) else { continue };
            if !seen.insert(cam.mac) {
                continue;
            }
            let via = match from {
                SocketAddr::V4(v) => net::local_for(&ctx.ifaces, *v.ip()).map(|i| i.name.clone()),
                _ => None,
            }
            .or_else(|| ctx.legs.first().map(|l| l.iface.clone()))
            .unwrap_or_default();
            ctx.found(cam, Method::Broadcast, &via);
        }
        Ok(())
    }

    fn can_probe(&self) -> bool {
        true
    }

    async fn probe(&self, ctx: &ScanCtx, ip: Ipv4Addr) -> Result<Option<Camera>> {
        self.sweep_inner(ctx, &[ip], None).await.map(|mut v| v.pop())
    }

    /// One socket, every target, both probes: an ENQ to 52380 (some cameras
    /// answer it unicast) and a VISCA version inquiry to 52381.
    async fn sweep(&self, ctx: &ScanCtx, targets: &[Ipv4Addr], via: &str) -> Result<()> {
        for cam in self.sweep_inner(ctx, targets, Some(via)).await? {
            ctx.found(cam, Method::Unicast, via);
        }
        Ok(())
    }

    async fn read_network(&self, _op: &OpCtx, cam: &Camera) -> Result<NetConfig> {
        cam.net.clone().ok_or(DriverError::Unsupported("read the settings of a camera only reached over a router"))
    }

    async fn write_network(&self, op: &OpCtx, cam: &Camera, cfg: &NetConfig) -> Result<WriteOutcome> {
        if cfg.dhcp {
            return Err(DriverError::Unsupported("switch a camera to DHCP over Sony IP Setup — use its web UI"));
        }
        if cam.fields.get("write").is_some_and(|w| w.eq_ignore_ascii_case("off")) {
            return Err(DriverError::Protocol("the camera has IP setup locked (WRITE:off). Power-cycle it and change the address within 20 minutes".into()));
        }
        let mac = cam.mac.ok_or_else(|| DriverError::Invalid("no MAC address for this camera; rescan".into()))?;
        let port = op.port("setup", SETUP_PORT);
        let name = clean_name(cfg.hostname.as_deref().or(cam.name.as_deref()).unwrap_or(""));
        let (ip, mask) = (cfg.ip.to_string(), prefix_to_mask(cfg.prefix).to_string());
        let gw = cfg.gateway.unwrap_or(Ipv4Addr::UNSPECIFIED).to_string();
        let mac_s = mac.to_upper("-");
        let mut fields: Vec<(&str, &str)> = vec![("MAC", &mac_s), ("IPADR", &ip), ("MASK", &mask)];
        // Older models' set has no GATEWAY; send it only where the reply had one.
        if !cam.fields.contains_key("gatewayField") {
            fields.push(("GATEWAY", &gw));
        }
        fields.push(("NAME", &name));
        let pkt = frame(&fields);

        let mut socks: Vec<Arc<UdpSocket>> = listener(op.port("reply", SETUP_PORT)).into_iter().collect();
        let mut sends = Vec::new();
        for leg in &op.legs {
            let s = Arc::new(leg_socket(leg)?);
            socks.push(s.clone());
            sends.push((s, leg.clone()));
        }
        if sends.is_empty() {
            return Err(DriverError::Unreachable("no interface to send on".into()));
        }
        let mut inbox = Inbox::new(&socks);
        for (s, leg) in &sends {
            for d in &leg.dests {
                let _ = s.send_to(&pkt, op.to(*d, port)).await;
            }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Some((b, _)) = inbox.next_until(deadline).await {
            let Some(f) = parse(&b) else { continue };
            let Some((k, v)) = f.first() else { continue };
            let for_us = v.parse::<Mac>().ok() == Some(mac);
            if k == "ACK" && for_us {
                return Ok(WriteOutcome {
                    message: format!("The camera accepted {}/{}", cfg.ip, cfg.prefix),
                    restarts: true,
                    expect_ip: Some(cfg.ip),
                    settle_ms: 4000,
                });
            }
            if k == "NAK" && for_us {
                let why = f.get(1).map(|(k, v)| if v.is_empty() { k.clone() } else { format!("{k}:{v}") }).unwrap_or_default();
                return Err(DriverError::Protocol(format!("the camera refused the change{}", if why.is_empty() { String::new() } else { format!(" ({why})") })));
            }
        }
        Err(DriverError::Protocol("no answer from the camera. Check it allows IP setup (it locks 20 minutes after power-on on some models) and that this computer is on the same network segment".into()))
    }
}

impl Sony {
    async fn sweep_inner(&self, ctx: &ScanCtx, targets: &[Ipv4Addr], _via: Option<&str>) -> Result<Vec<Camera>> {
        let setup = ctx.port("setup", SETUP_PORT);
        let visca = ctx.port("visca", VISCA_PORT);
        let s = Arc::new(udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0), UdpOpts::default())?);
        let mut inbox = Inbox::new(std::slice::from_ref(&s));
        let enq = inquiry();
        for (n, ip) in targets.iter().enumerate() {
            if ctx.cancelled() {
                break;
            }
            let _ = s.send_to(&enq, ctx.to(*ip, setup)).await;
            let _ = s.send_to(&visca_version_inquiry(n as u32), ctx.to(*ip, visca)).await;
            // Pace a big sweep so a switch's broadcast-storm guard stays calm.
            if n % 64 == 63 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        let deadline = Instant::now() + ctx.probe_timeout + Duration::from_millis(400);
        let mut by_ip: BTreeMap<Ipv4Addr, Camera> = BTreeMap::new();
        while let Some((b, from)) = inbox.next_until(deadline).await {
            let SocketAddr::V4(from) = from else { continue };
            let ip = *from.ip();
            if !targets.contains(&ip) {
                continue;
            }
            if from.port() == setup {
                if let Some(c) = parse(&b).and_then(|f| camera_from_reply(&f)) {
                    by_ip.insert(ip, c);
                }
            } else if let Some((vendor, model, rom)) = parse_visca_version(&b) {
                if vendor == 0x0001 && !by_ip.contains_key(&ip) {
                    let mut c = Camera::new(ID, "Sony", ip);
                    c.web_port = Some(80);
                    c.caps = Caps { write_by_mac: true, ..Caps::default() };
                    by_ip.insert(ip, c.field("viscaModel", format!("{model:#06x}")).field("viscaRom", format!("{rom:#06x}")));
                }
            }
        }
        Ok(by_ip.into_values().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inquiry_bytes() {
        assert_eq!(inquiry(), b"\x02ENQ:network\xFF\x03".to_vec());
    }

    #[test]
    fn reply_with_info_field_parses_by_key() {
        let r = frame(&[("MAC", "08-00-46-AA-BB-CC"), ("INFO", "network"), ("MODEL", "IPCARD"), ("SOFTVERSION", "2.10.00"), ("IPADR", "192.168.0.100"), ("MASK", "255.255.255.0"), ("GATEWAY", "192.168.0.1"), ("NAME", "CAM1"), ("WRITE", "on")]);
        let c = camera_from_reply(&parse(&r).unwrap()).unwrap();
        assert_eq!(c.mac.unwrap().to_string(), "08:00:46:aa:bb:cc");
        assert_eq!(c.ip, Ipv4Addr::new(192, 168, 0, 100));
        assert_eq!(c.name.as_deref(), Some("CAM1"));
        assert_eq!(c.firmware.as_deref(), Some("2.10.00"));
        assert!(c.caps.write_network);
        assert_eq!(c.fields["networkBoard"], "IPCARD");
        let locked = frame(&[("MAC", "08-00-46-AA-BB-CC"), ("IPADR", "192.168.0.100"), ("MASK", "255.255.255.0"), ("WRITE", "off")]);
        let c = camera_from_reply(&parse(&locked).unwrap()).unwrap();
        assert!(!c.caps.write_network);
        assert!(c.fields.contains_key("gatewayField"));
    }

    #[test]
    fn our_own_echo_is_not_a_camera() {
        assert!(camera_from_reply(&parse(&inquiry()).unwrap()).is_none());
    }

    #[test]
    fn names_are_cut_to_what_sony_takes() {
        assert_eq!(clean_name("Cam-1 Left!"), "Cam1 Lef");
    }

    #[test]
    fn visca_version_reply() {
        let mut r = vec![0x01, 0x11, 0x00, 0x0A, 0, 0, 0, 7];
        r.extend_from_slice(&[0x90, 0x50, 0x00, 0x01, 0x05, 0x19, 0x02, 0x10, 0x02, 0xFF]);
        assert_eq!(parse_visca_version(&r), Some((1, 0x0519, 0x0210)));
        assert_eq!(&visca_version_inquiry(7)[..8], &[0x01, 0x10, 0x00, 0x05, 0, 0, 0, 7]);
    }
}
