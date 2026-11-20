//! NDI — sources announced over mDNS (`_ndi._tcp.local.`).
//!
//! NDI names are `MACHINE (Source)`; BirdDog, PTZOptics, Panasonic NDI-HX,
//! OBSBOT and most NDI PTZ cameras announce one. This driver cannot change
//! anything — it labels what it hears by brand and leaves the camera to its
//! vendor's driver, which the engine asks to probe each address it finds
//! (that is how BirdDog cameras are found without a sweep).
//!
//! mDNS is link-local multicast and answers are multicast too, so a camera on
//! the wrong subnet still shows up here, flagged as unreachable.

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::Duration;

use async_trait::async_trait;
use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent};
use meerkat_core::driver::{Level, Result};
use meerkat_core::net;
use meerkat_core::{Camera, Driver, DriverInfo, Method, ScanCtx};
use tokio::time::Instant;

use crate::util::guess_vendor;

pub struct Ndi;

pub const ID: &str = "ndi";
pub const SERVICE: &str = "_ndi._tcp.local.";

/// `("BIRDDOG-608F9", Some("CAM"))` from `BIRDDOG-608F9 (CAM)._ndi._tcp.local.`.
pub fn split_name(fullname: &str) -> (String, Option<String>) {
    // Only the one dot before the service type: an instance name can itself
    // end in an (escaped) dot.
    let inst = fullname.strip_suffix(SERVICE).map(|s| s.strip_suffix('.').unwrap_or(s)).unwrap_or(fullname);
    let inst = unescape(inst);
    match (inst.rfind(" ("), inst.ends_with(')')) {
        (Some(i), true) => (inst[..i].trim().to_string(), Some(inst[i + 2..inst.len() - 1].trim().to_string())),
        _ => (inst.trim().to_string(), None),
    }
}

/// Undo DNS-SD label escaping: `\DDD` is a decimal byte (`\032` a space) and
/// `\` before anything else is that character. mdns-sd 0.22 keeps `\.` and
/// `\\` in the fullnames it reports, so `Studio.A (CAM)` arrives as
/// `Studio\.A (CAM)`.
fn unescape(label: &str) -> String {
    let b = label.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
        } else if let Some(n) = b
            .get(i + 1..i + 4)
            .filter(|d| d.iter().all(u8::is_ascii_digit))
            .and_then(|d| std::str::from_utf8(d).ok()?.parse::<u8>().ok())
        {
            out.push(n);
            i += 4;
        } else {
            out.push(b[i + 1]);
            i += 2;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn camera_from_service(fullname: &str, host: &str, port: u16, ip: Ipv4Addr) -> Camera {
    let (machine, source) = split_name(fullname);
    let label = format!("{machine} {}", source.as_deref().unwrap_or(""));
    let vendor = guess_vendor(&label).or_else(|| guess_vendor(host)).unwrap_or("NDI");
    let mut cam = Camera::new(ID, vendor, ip);
    cam.name = Some(match &source {
        Some(s) => format!("{machine} ({s})"),
        None => machine.clone(),
    });
    cam.web_port = Some(80);
    cam.fields.insert("ndiName".into(), cam.name.clone().unwrap_or_default());
    cam.fields.insert("ndiPort".into(), port.to_string());
    if !host.is_empty() {
        cam.fields.insert("mdnsHost".into(), host.trim_end_matches('.').to_string());
    }
    cam
}

#[async_trait]
impl Driver for Ndi {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: ID,
            vendor: "NDI",
            protocol: "NDI discovery (mDNS)",
            priority: 10,
            discovery: vec!["mDNS browse for _ndi._tcp on each interface — also sees cameras on the wrong subnet"],
            sweep: None,
            notes: vec!["Finds NDI cameras of any brand. Changing settings is left to the brand's own driver; an NDI source no driver recognises opens its web UI."],
            firmware: None,
            enabled_by_default: true,
            recovery: None,
        }
    }

    async fn discover(&self, ctx: &ScanCtx) -> Result<()> {
        // Tests and the demo move discovery off the wire: "ndi.off" skips it,
        // "ndi.sim" asks the simulator for its pretend announcements instead.
        if let Some(port) = ctx.ports.get("ndi.sim").copied() {
            return simulated(ctx, port).await;
        }
        if ctx.ports.contains_key("ndi.off") {
            return Ok(());
        }
        let ifaces: Vec<String> = ctx.legs.iter().filter(|l| !l.bind.is_unspecified()).map(|l| l.iface.clone()).collect();
        if ifaces.is_empty() {
            return Ok(());
        }
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                ctx.log(Level::Warn, format!("mDNS unavailable: {e}"));
                return Ok(());
            }
        };
        let _ = daemon.disable_interface(IfKind::All);
        for i in &ifaces {
            let _ = daemon.enable_interface(IfKind::Name(i.clone()));
        }
        let rx = match daemon.browse(SERVICE) {
            Ok(rx) => rx,
            Err(e) => {
                ctx.log(Level::Warn, format!("mDNS browse failed: {e}"));
                let _ = daemon.shutdown();
                return Ok(());
            }
        };
        let deadline = Instant::now() + ctx.listen.max(Duration::from_millis(1500));
        let mut seen = HashSet::new();
        // This computer's own NDI outputs (macOS AV Output, OBS, vMix…)
        // announce themselves on every interface; they are not cameras.
        let own: HashSet<Ipv4Addr> = net::interfaces().map(|v| v.into_iter().map(|i| i.ip).collect()).unwrap_or_default();
        loop {
            let ev = tokio::select! {
                r = tokio::time::timeout_at(deadline, rx.recv_async()) => match r { Ok(Ok(e)) => e, _ => break },
                _ = ctx.cancel.cancelled() => break,
            };
            if let ServiceEvent::ServiceResolved(info) = ev {
                for ip in info.get_addresses_v4() {
                    if own.contains(&ip) || !seen.insert((info.get_fullname().to_string(), ip)) {
                        continue;
                    }
                    let cam = camera_from_service(info.get_fullname(), info.get_hostname(), info.get_port(), ip);
                    let via = net::local_for(&ctx.ifaces, ip).map(|i| i.name.clone()).or_else(|| ifaces.first().cloned()).unwrap_or_default();
                    ctx.found(cam, Method::Mdns, &via);
                }
            }
        }
        let _ = daemon.stop_browse(SERVICE);
        let _ = daemon.shutdown();
        Ok(())
    }
}

/// The simulator's stand-in for mDNS: one UDP question, a JSON list back of
/// `{name, host, port, ip}` records.
async fn simulated(ctx: &ScanCtx, port: u16) -> Result<()> {
    let s = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    s.send_to(b"NDI?", (Ipv4Addr::LOCALHOST, port)).await?;
    let mut buf = vec![0u8; 8192];
    let Ok(Ok((n, _))) = tokio::time::timeout(ctx.listen, s.recv_from(&mut buf)).await else { return Ok(()) };
    let list: Vec<serde_json::Value> = serde_json::from_slice(&buf[..n]).unwrap_or_default();
    for r in list {
        let (Some(name), Some(ip)) = (r["name"].as_str(), r["ip"].as_str().and_then(|i| i.parse::<Ipv4Addr>().ok())) else { continue };
        let cam = camera_from_service(name, r["host"].as_str().unwrap_or(""), r["port"].as_u64().unwrap_or(5961) as u16, ip);
        let via = ctx.legs.first().map(|l| l.iface.clone()).unwrap_or_default();
        ctx.found(cam, Method::Mdns, &via);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndi_names() {
        assert_eq!(split_name("BIRDDOG-608F9 (CAM)._ndi._tcp.local."), ("BIRDDOG-608F9".into(), Some("CAM".into())));
        assert_eq!(split_name("TAIL 2_BC3CBC (OBSBOT)._ndi._tcp.local."), ("TAIL 2_BC3CBC".into(), Some("OBSBOT".into())));
        assert_eq!(split_name("STUDIO._ndi._tcp.local."), ("STUDIO".into(), None));
        // Escapes as mdns-sd 0.22 reports them, and the older \DDD form.
        assert_eq!(split_name("Studio\\.A (CAM)._ndi._tcp.local."), ("Studio.A".into(), Some("CAM".into())));
        assert_eq!(split_name("PTZ\\032ONE\\032\\(CAM\\)._ndi._tcp.local."), ("PTZ ONE".into(), Some("CAM".into())));
        assert_eq!(split_name("v2\\.._ndi._tcp.local."), ("v2.".into(), None));
        assert_eq!(split_name("A\\\\B._ndi._tcp.local."), ("A\\B".into(), None));
        let c = camera_from_service("BIRDDOG-608F9 (CAM)._ndi._tcp.local.", "birddog-608f9.local.", 5961, Ipv4Addr::new(10, 0, 0, 7));
        assert_eq!(c.vendor, "BirdDog");
        assert_eq!(c.name.as_deref(), Some("BIRDDOG-608F9 (CAM)"));
        assert_eq!(c.fields["mdnsHost"], "birddog-608f9.local");
        let o = camera_from_service("TAIL 2_BC3CBC (OBSBOT)._ndi._tcp.local.", "", 5961, Ipv4Addr::new(10, 0, 0, 8));
        assert_eq!(o.vendor, "OBSBOT");
    }
}
