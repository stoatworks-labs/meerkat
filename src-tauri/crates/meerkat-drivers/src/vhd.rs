//! The ValueHD ("VHD") camera platform — PTZOptics and the OEMs that share its
//! firmware — via its "Upgrade Tool" discovery on multicast 239.255.255.251:8005.
//!
//! Source: open-source code only — the device side in tt-0927/IPC_Camera's
//! multicast UDP server, the client side in Electronics/PTZControllerUI — and
//! none of it checked on hardware here. So this driver **discovers only**: two
//! different "CONFIGNET" set dialects exist in that code and neither is
//! verified, and Meerkat does not send a write it cannot vouch for. Change these
//! cameras' addresses in their web UI (or over ONVIF, if it is switched on).
//!
//! ```text
//! search  "SEARCH * UPGRADE\r\nClient ID:<32 hex>"
//! reply   "REPLY OK\r\nVERSION:..\r\nDEVICE_MODEL:..\r\nClient ID:..\r\nDevice ID:..\r\nDHCP=..\r\nIP=..\r\nMASK=..\r\nGATEWAY=..\r\nMAC=..\r\nFDNS=.."
//! ```

use std::collections::{BTreeMap, HashSet};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use async_trait::async_trait;
use meerkat_core::driver::{Level, Result};
use meerkat_core::net::{self, mask_to_prefix, udp_socket, UdpOpts};
use meerkat_core::{Caps, Camera, Driver, DriverInfo, Mac, Method, NetConfig, ScanCtx};
use tokio::time::Instant;

use crate::udp::Inbox;
use crate::util::{guess_vendor, random_bytes};

pub struct Vhd;

pub const ID: &str = "vhd";
pub const PORT: u16 = 8005;
pub const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 251);

pub fn search(client_id: &str) -> Vec<u8> {
    format!("SEARCH * UPGRADE\r\nClient ID:{client_id}").into_bytes()
}

/// `KEY` → value for every `KEY:value` or `KEY=value` line of a reply.
pub fn parse_reply(b: &[u8]) -> Option<BTreeMap<String, String>> {
    let s = String::from_utf8_lossy(b);
    let mut lines = s.lines();
    if !lines.next()?.trim().eq_ignore_ascii_case("REPLY OK") {
        return None;
    }
    let mut out = BTreeMap::new();
    for l in lines {
        let i = l.find([':', '=']);
        if let Some(i) = i {
            out.insert(l[..i].trim().to_ascii_uppercase(), l[i + 1..].trim().to_string());
        }
    }
    Some(out)
}

pub fn camera_from_reply(m: &BTreeMap<String, String>) -> Option<Camera> {
    let ip: Ipv4Addr = m.get("IP")?.parse().ok()?;
    let model = m.get("DEVICE_MODEL").cloned().filter(|s| !s.is_empty());
    let vendor = model.as_deref().and_then(guess_vendor).unwrap_or("PTZOptics / VHD");
    let mut cam = Camera::new(ID, vendor, ip);
    cam.model = model;
    cam.mac = m.get("MAC").and_then(|x| x.parse::<Mac>().ok());
    cam.firmware = m.get("VERSION").cloned();
    cam.web_port = Some(80);
    if let Some(prefix) = m.get("MASK").and_then(|x| x.parse().ok()).and_then(mask_to_prefix) {
        cam.net = Some(NetConfig {
            dhcp: m.get("DHCP").map(|d| d == "1" || d.eq_ignore_ascii_case("on")).unwrap_or(false),
            ip,
            prefix,
            gateway: m.get("GATEWAY").and_then(|g| g.parse().ok()),
            dns: m.get("FDNS").and_then(|d| d.parse().ok()).into_iter().collect(),
            hostname: None,
        });
    }
    cam.caps = Caps { read_network: true, ..Caps::default() };
    if let Some(d) = m.get("DEVICE ID") {
        cam.fields.insert("deviceId".into(), d.clone());
    }
    if let Some(u) = m.get("UPTIME") {
        cam.fields.insert("uptime".into(), u.clone());
    }
    Some(cam)
}

#[async_trait]
impl Driver for Vhd {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: ID,
            vendor: "PTZOptics / VHD",
            protocol: "VHD Upgrade Tool discovery",
            priority: 50,
            discovery: vec!["Multicast \"SEARCH * UPGRADE\" to 239.255.255.251:8005 (answers are multicast too)"],
            sweep: None,
            notes: vec![
                "Discovery only: the platform's set command is not verified on hardware, so change these cameras' addresses in their web UI, or over ONVIF.",
                "Seen on PTZOptics and other cameras built on the ValueHD platform.",
            ],
            enabled_by_default: true,
            recovery: None,
        }
    }

    async fn discover(&self, ctx: &ScanCtx) -> Result<()> {
        let port = ctx.port("udp", PORT);
        let testing = ctx.ports.contains_key("vhd.udp");
        // On a real network the answers are multicast to the group's port, so
        // the listener has to sit on it. Against the simulator they come back
        // to the sending socket, and the group's port IS the simulator's own
        // socket: macOS lets a wildcard bind share it, Linux refuses, and the
        // scan used to give up there. So the simulator gets an ephemeral port.
        let listen_port = if testing { 0 } else { port };
        let listen = match udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, listen_port), UdpOpts { reuse: true, broadcast: true, ..Default::default() }) {
            Ok(s) => s,
            Err(e) => {
                ctx.log(Level::Debug, format!("cannot listen on UDP {port}: {e}"));
                return Ok(());
            }
        };
        if !testing {
            for leg in ctx.legs.iter().filter(|l| !l.bind.is_unspecified()) {
                let _ = listen.join_multicast_v4(GROUP, leg.bind);
            }
        }
        let listen = Arc::new(listen);
        let id: String = random_bytes::<16>().iter().map(|b| format!("{b:02x}")).collect();
        let pkt = search(&id);
        let mut socks = vec![listen.clone()];
        for leg in &ctx.legs {
            let opts = UdpOpts { broadcast: true, reuse: true, iface_index: leg.index, multicast_ttl: Some(2) };
            let Ok(s) = udp_socket(SocketAddrV4::new(leg.bind, 0), opts) else { continue };
            if !leg.bind.is_unspecified() {
                let _ = socket2::SockRef::from(&s).set_multicast_if_v4(&leg.bind);
            }
            let dests: Vec<Ipv4Addr> = if testing || leg.bind.is_unspecified() { leg.dests.clone() } else { vec![GROUP] };
            for d in dests {
                let _ = s.send_to(&pkt, ctx.to(d, port)).await;
            }
            socks.push(Arc::new(s));
        }
        let mut inbox = Inbox::new(&socks);
        let deadline = Instant::now() + ctx.listen;
        let mut seen = HashSet::new();
        while let Some((b, from)) = tokio::select! { x = inbox.next_until(deadline) => x, _ = ctx.cancel.cancelled() => None } {
            let Some(m) = parse_reply(&b) else { continue };
            let Some(cam) = camera_from_reply(&m) else { continue };
            if !seen.insert((cam.mac, cam.ip)) {
                continue;
            }
            let from_ip = match from {
                std::net::SocketAddr::V4(v) => *v.ip(),
                _ => cam.ip,
            };
            let via = net::local_for(&ctx.ifaces, from_ip).map(|i| i.name.clone()).or_else(|| ctx.legs.first().map(|l| l.iface.clone())).unwrap_or_default();
            ctx.found(cam, Method::Multicast, &via);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_parses() {
        let r = b"REPLY OK\r\nVERSION:SOC v6.3.32\r\nDEVICE_MODEL:PT20X-SDI-G3\r\nClient ID:00ff\r\nDevice ID:abcdef0123\r\nUptime=1234\r\nDHCP=0\r\nIP=192.168.100.88\r\nMASK=255.255.255.0\r\nGATEWAY=192.168.100.1\r\nMAC=DC:ED:84:00:11:22\r\nFDNS=192.168.100.1";
        let m = parse_reply(r).unwrap();
        let c = camera_from_reply(&m).unwrap();
        assert_eq!(c.ip, Ipv4Addr::new(192, 168, 100, 88));
        assert_eq!(c.model.as_deref(), Some("PT20X-SDI-G3"));
        assert_eq!(c.mac.unwrap().to_string(), "dc:ed:84:00:11:22");
        assert_eq!(c.net.as_ref().unwrap().gateway, Some(Ipv4Addr::new(192, 168, 100, 1)));
        assert!(!c.caps.write_network);
        assert!(parse_reply(b"SEARCH * UPGRADE\r\nClient ID:1").is_none());
    }
}
