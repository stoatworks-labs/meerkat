//! ONVIF — WS-Discovery to find, the device service to read and change the
//! address. The generic driver: most PTZ brands (Panasonic, Sony, AVer,
//! Lumens, Marshall, PTZOptics, Avonic…) speak it, so it covers whatever has
//! no vendor driver.
//!
//! Sources: ONVIF Core Specification (§5.9 auth, §7.3 discovery, §8.2 network),
//! WS-Discovery 2005/04, OASIS UsernameToken Profile 1.0.
//!
//! - **Discovery:** a SOAP Probe to 239.255.255.250:3702 out of each interface
//!   (and to `<ip>:3702` in a sweep). ProbeMatches come back unicast, so unlike
//!   the Panasonic and Sony protocols this does *not* see a camera on the wrong
//!   subnet.
//! - **Auth:** HTTP Digest when the camera answers 401, and a WS-Security
//!   UsernameToken in every request (`Base64(SHA1(nonce ‖ created ‖ password))`,
//!   `created` from the camera's own clock, read first with the unauthenticated
//!   GetSystemDateAndTime).
//! - **Write order:** gateway and DNS first, then SetNetworkInterfaces. If the
//!   camera says RebootNeeded it stays on the old address until SystemReboot,
//!   which Meerkat sends; otherwise it moves at once.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::driver::{Level, Result};
use meerkat_core::http::{base64, Http};
use meerkat_core::net::{self, udp_socket, UdpOpts};
use meerkat_core::{Caps, Camera, Credentials, Driver, DriverError, DriverInfo, Mac, Method, NetConfig, OpCtx, ScanCtx, WriteOutcome};
use sha1::{Digest, Sha1};
use tokio::net::UdpSocket;
use tokio::time::Instant;

use crate::udp::Inbox;
use crate::util::{escape, guess_vendor, iso8601, now_unix, random_bytes, unix_from_civil, uuid4, xml_attr, xml_blocks, xml_text, xml_texts};

pub struct Onvif;

pub const ID: &str = "onvif";
pub const WSD_PORT: u16 = 3702;
pub const WSD_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);

pub fn probe_xml(message_id: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" "#,
            r#"xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery" xmlns:dn="http://www.onvif.org/ver10/network/wsdl">"#,
            r#"<s:Header><a:Action s:mustUnderstand="1">http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</a:Action>"#,
            r#"<a:MessageID>urn:uuid:{}</a:MessageID><a:ReplyTo><a:Address>http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address></a:ReplyTo>"#,
            r#"<a:To s:mustUnderstand="1">urn:schemas-xmlsoap-org:ws:2005:04:discovery</a:To></s:Header>"#,
            r#"<s:Body><d:Probe><d:Types>dn:NetworkVideoTransmitter</d:Types></d:Probe></s:Body></s:Envelope>"#
        ),
        message_id
    )
}

/// What a ProbeMatch says about one device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeMatch {
    pub xaddrs: Vec<String>,
    pub scopes: Vec<String>,
    pub endpoint: Option<String>,
    pub relates_to: Option<String>,
}

pub fn parse_probe_matches(xml: &str) -> Vec<ProbeMatch> {
    let relates = xml_text(xml, "RelatesTo");
    xml_blocks(xml, "ProbeMatch")
        .into_iter()
        .map(|m| ProbeMatch {
            xaddrs: xml_text(m, "XAddrs").map(|x| x.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
            scopes: xml_text(m, "Scopes").map(|x| x.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
            endpoint: xml_text(m, "Address"),
            relates_to: relates.clone(),
        })
        .collect()
}

/// `onvif://www.onvif.org/<kind>/<value>` scopes, URL-decoded.
pub fn scope(scopes: &[String], kind: &str) -> Option<String> {
    let prefix = format!("onvif://www.onvif.org/{kind}/");
    scopes.iter().find_map(|s| {
        let rest = s.strip_prefix(&prefix).or_else(|| s.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()).then(|| &s[prefix.len()..]))?;
        Some(url_decode(rest))
    })
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The device-service URL to use: the first XAddr on the address the reply
/// came from, else the first IPv4 one.
fn pick_xaddr(m: &ProbeMatch, from: Ipv4Addr) -> Option<String> {
    m.xaddrs.iter().find(|x| x.contains(&format!("//{from}"))).or_else(|| m.xaddrs.iter().find(|x| !x.contains('['))).cloned()
}

fn host_of(url: &str) -> Option<Ipv4Addr> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', ':']).next()?;
    host.parse().ok()
}

pub fn camera_from_match(m: &ProbeMatch, from: Ipv4Addr) -> Option<Camera> {
    let xaddr = pick_xaddr(m, from)?;
    let ip = host_of(&xaddr).unwrap_or(from);
    let name = scope(&m.scopes, "name");
    let hardware = scope(&m.scopes, "hardware");
    let vendor = [hardware.as_deref(), name.as_deref()].into_iter().flatten().find_map(guess_vendor).unwrap_or("ONVIF");
    let mut cam = Camera::new(ID, vendor, ip);
    cam.model = hardware;
    cam.name = name;
    cam.mac = scope(&m.scopes, "MacAddress").or_else(|| scope(&m.scopes, "macaddress")).and_then(|s| s.parse::<Mac>().ok());
    cam.serial = scope(&m.scopes, "SerialNumber");
    cam.web_port = xaddr.split_once("://").and_then(|(_, r)| r.split('/').next()).and_then(|h| h.rsplit_once(':')).and_then(|(_, p)| p.parse().ok()).or(Some(80));
    cam.caps = Caps { read_network: true, write_network: true, needs_auth: true, dhcp: true, dns: true, hostname: true, reboot: true, ..Caps::default() };
    cam.fields.insert("onvifXaddr".into(), xaddr);
    if let Some(loc) = scope(&m.scopes, "location") {
        cam.fields.insert("onvifLocation".into(), loc);
    }
    let profiles: Vec<String> = m.scopes.iter().filter_map(|s| s.strip_prefix("onvif://www.onvif.org/Profile/").map(str::to_string)).collect();
    if !profiles.is_empty() {
        cam.fields.insert("onvifProfiles".into(), profiles.join(", "));
    }
    if m.scopes.iter().any(|s| s.to_ascii_lowercase().contains("/type/ptz")) {
        cam.fields.insert("onvifType".into(), "PTZ".into());
    }
    Some(cam)
}

/// WS-Security UsernameToken with PasswordDigest.
pub fn security_header(c: &Credentials, nonce: &[u8], created: &str) -> String {
    let mut h = Sha1::new();
    h.update(nonce);
    h.update(created.as_bytes());
    h.update(c.password.as_bytes());
    let digest = base64(&h.finalize());
    format!(
        concat!(
            r#"<wsse:Security s:mustUnderstand="1" xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd" "#,
            r#"xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd"><wsse:UsernameToken>"#,
            r#"<wsse:Username>{}</wsse:Username>"#,
            r#"<wsse:Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest">{}</wsse:Password>"#,
            r#"<wsse:Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary">{}</wsse:Nonce>"#,
            r#"<wsu:Created>{}</wsu:Created></wsse:UsernameToken></wsse:Security>"#
        ),
        escape(&c.username),
        digest,
        base64(nonce),
        created
    )
}

fn envelope(header: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:tds="http://www.onvif.org/ver10/device/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema"><s:Header>{header}</s:Header><s:Body>{body}</s:Body></s:Envelope>"#
    )
}

/// One device-service session: the URL, the login and the camera's clock offset.
struct Session<'a> {
    http: &'a Http,
    url: String,
    creds: Option<Credentials>,
    offset: i64,
}

impl<'a> Session<'a> {
    async fn open(http: &'a Http, url: String, creds: Option<Credentials>) -> Session<'a> {
        let mut s = Session { http, url, creds, offset: 0 };
        if let Ok(x) = s.call("<tds:GetSystemDateAndTime/>", false).await {
            if let Some(utc) = xml_blocks(&x, "UTCDateTime").first() {
                let n = |k: &str| xml_text(utc, k).and_then(|v| v.parse::<i64>().ok());
                if let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(se)) = (n("Year"), n("Month"), n("Day"), n("Hour"), n("Minute"), n("Second")) {
                    s.offset = unix_from_civil(y, mo as u32, d as u32, h as u32, mi as u32, se as u32) - now_unix();
                }
            }
        }
        s
    }

    async fn call(&self, body: &str, auth: bool) -> Result<String> {
        let header = match (&self.creds, auth) {
            (Some(c), true) if !c.username.is_empty() => {
                let nonce: [u8; 16] = random_bytes();
                security_header(c, &nonce, &iso8601(now_unix() + self.offset))
            }
            _ => String::new(),
        };
        let xml = envelope(&header, body);
        let r = self
            .http
            .post(&self.url, "application/soap+xml; charset=utf-8", xml.into_bytes(), if auth { self.creds.as_ref() } else { None })
            .await
            .map_err(|e| match e {
                meerkat_core::http::HttpError::Status { status: 400 | 500, body } if body.contains("NotAuthorized") || body.contains("Sender") && body.contains("Auth") => DriverError::Protocol("the camera refused that username and password".into()),
                e => DriverError::Http(e),
            })?;
        if r.body.contains(":Fault>") || r.body.contains("<Fault>") {
            let reason = xml_text(&r.body, "Text").or_else(|| xml_text(&r.body, "Reason")).unwrap_or_else(|| "SOAP fault".into());
            return Err(DriverError::Protocol(reason));
        }
        Ok(r.body)
    }
}

/// The settings GetNetworkInterfaces + GetNetworkDefaultGateway + GetDNS +
/// GetHostname describe, and the interface token to write back to.
pub fn parse_network(ifaces: &str, gateway: Option<&str>, dns: Option<&str>, hostname: Option<&str>) -> Option<(String, NetConfig, Option<Mac>)> {
    let block = xml_blocks(ifaces, "NetworkInterfaces").into_iter().next()?;
    let token = xml_attr(ifaces, "NetworkInterfaces", "token").unwrap_or_else(|| "eth0".into());
    let ipv4 = xml_blocks(block, "IPv4").into_iter().next()?;
    let dhcp = xml_text(ipv4, "DHCP").map(|v| v == "true").unwrap_or(false);
    let pick = |section: &str| xml_blocks(ipv4, section).into_iter().next().and_then(|b| Some((xml_text(b, "Address")?.parse::<Ipv4Addr>().ok()?, xml_text(b, "PrefixLength")?.parse::<u8>().ok()?)));
    let (ip, prefix) = if dhcp { pick("FromDHCP").or_else(|| pick("Manual"))? } else { pick("Manual").or_else(|| pick("FromDHCP"))? };
    let mac = xml_text(block, "HwAddress").and_then(|m| m.parse().ok());
    let gateway = gateway.and_then(|g| xml_text(g, "IPv4Address")).and_then(|g| g.parse().ok());
    let dns = dns
        .map(|d| {
            let src = if xml_text(d, "FromDHCP").as_deref() == Some("true") { xml_blocks(d, "DNSFromDHCP") } else { xml_blocks(d, "DNSManual") };
            src.into_iter().filter_map(|b| xml_text(b, "IPv4Address")?.parse().ok()).collect()
        })
        .unwrap_or_default();
    let hostname = hostname.and_then(|h| xml_text(h, "Name")).filter(|n| !n.is_empty());
    Some((token, NetConfig { dhcp, ip, prefix, gateway, dns, hostname }, mac))
}

fn xaddr_of(cam: &Camera, ports: &meerkat_core::driver::PortMap) -> String {
    if let Some(x) = cam.fields.get("onvifXaddr") {
        // A camera re-addressed since discovery is rescanned before it is written
        // to, so the XAddr host is trusted as it stands.
        return x.clone();
    }
    let port = ports.get("onvif.http").copied().unwrap_or(80);
    format!("http://{}:{port}/onvif/device_service", cam.ip)
}

fn wsd_socket(bind: Ipv4Addr, index: Option<u32>) -> std::io::Result<UdpSocket> {
    let s = udp_socket(SocketAddrV4::new(bind, 0), UdpOpts { broadcast: true, reuse: false, iface_index: index, multicast_ttl: Some(2) })?;
    if !bind.is_unspecified() {
        let _ = s.set_multicast_loop_v4(true);
        let raw = socket2::SockRef::from(&s);
        let _ = raw.set_multicast_if_v4(&bind);
    }
    Ok(s)
}

#[async_trait]
impl Driver for Onvif {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: ID,
            vendor: "ONVIF",
            protocol: "ONVIF (WS-Discovery + device service)",
            priority: 30,
            discovery: vec!["WS-Discovery Probe to 239.255.255.250:3702 on each interface"],
            sweep: Some("WS-Discovery Probe to <ip>:3702"),
            notes: vec![
                "Covers brands without their own driver (AVer, Lumens, Marshall, PTZOptics, Avonic and others), when ONVIF is switched on in the camera.",
                "Needs the camera's login to read or change settings.",
                "Answers come back unicast, so a camera on the wrong subnet is not seen this way.",
            ],
            enabled_by_default: true,
            recovery: None,
        }
    }

    async fn discover(&self, ctx: &ScanCtx) -> Result<()> {
        let port = ctx.port("wsd", WSD_PORT);
        let group = if ctx.ports.contains_key("onvif.wsd") { None } else { Some(WSD_GROUP) };
        let id = uuid4();
        let xml = probe_xml(&id);
        let mut socks = Vec::new();
        for leg in &ctx.legs {
            let s = match wsd_socket(leg.bind, leg.index) {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    ctx.log(Level::Warn, format!("{}: {e}", leg.iface));
                    continue;
                }
            };
            // Interfaces get the multicast group; the routed leg (and tests) the leg's own dests.
            let dests: Vec<Ipv4Addr> = match group {
                Some(g) if !leg.bind.is_unspecified() => vec![g],
                _ => leg.dests.clone(),
            };
            for round in 0..2 {
                for d in &dests {
                    let _ = s.send_to(xml.as_bytes(), ctx.to(*d, port)).await;
                }
                if round == 0 {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            }
            socks.push((s, leg.iface.clone()));
        }
        let all: Vec<Arc<UdpSocket>> = socks.iter().map(|(s, _)| s.clone()).collect();
        let mut inbox = Inbox::new(&all);
        let deadline = Instant::now() + ctx.listen;
        let mut seen = HashSet::new();
        while let Some((b, from)) = tokio::select! { x = inbox.next_until(deadline) => x, _ = ctx.cancel.cancelled() => None } {
            let SocketAddr::V4(from) = from else { continue };
            let text = String::from_utf8_lossy(&b);
            for m in parse_probe_matches(&text) {
                if m.relates_to.as_deref().is_some_and(|r| !r.contains(&id)) {
                    continue;
                }
                let key = m.endpoint.clone().unwrap_or_else(|| from.ip().to_string());
                if !seen.insert(key) {
                    continue;
                }
                if let Some(cam) = camera_from_match(&m, *from.ip()) {
                    let via = net::local_for(&ctx.ifaces, *from.ip()).map(|i| i.name.clone()).or_else(|| ctx.legs.first().map(|l| l.iface.clone())).unwrap_or_default();
                    ctx.found(cam, Method::Multicast, &via);
                }
            }
        }
        Ok(())
    }

    fn can_probe(&self) -> bool {
        true
    }

    async fn probe(&self, ctx: &ScanCtx, ip: Ipv4Addr) -> Result<Option<Camera>> {
        Ok(self.unicast(ctx, &[ip]).await?.pop())
    }

    async fn sweep(&self, ctx: &ScanCtx, targets: &[Ipv4Addr], via: &str) -> Result<()> {
        for c in self.unicast(ctx, targets).await? {
            ctx.found(c, Method::Unicast, via);
        }
        Ok(())
    }

    async fn read_network(&self, op: &OpCtx, cam: &Camera) -> Result<NetConfig> {
        let s = Session::open(&op.http, xaddr_of(cam, &op.ports), Some(op.creds()?.clone())).await;
        let ifaces = s.call("<tds:GetNetworkInterfaces/>", true).await?;
        let gw = s.call("<tds:GetNetworkDefaultGateway/>", true).await.ok();
        let dns = s.call("<tds:GetDNS/>", true).await.ok();
        let host = s.call("<tds:GetHostname/>", true).await.ok();
        parse_network(&ifaces, gw.as_deref(), dns.as_deref(), host.as_deref()).map(|(_, n, _)| n).ok_or_else(|| DriverError::Protocol("the camera's GetNetworkInterfaces reply has no IPv4 settings".into()))
    }

    async fn write_network(&self, op: &OpCtx, cam: &Camera, cfg: &NetConfig) -> Result<WriteOutcome> {
        let creds = op.creds()?.clone();
        let s = Session::open(&op.http, xaddr_of(cam, &op.ports), Some(creds)).await;
        let ifaces = s.call("<tds:GetNetworkInterfaces/>", true).await?;
        let current = parse_network(&ifaces, None, None, None);
        let token = current.as_ref().map(|(t, _, _)| t.clone()).unwrap_or_else(|| "eth0".into());
        let mut notes = Vec::new();

        if let Some(h) = cfg.hostname.as_deref().filter(|h| !h.is_empty()) {
            if s.call(&format!("<tds:SetHostname><tds:Name>{}</tds:Name></tds:SetHostname>", escape(h)), true).await.is_err() {
                notes.push("the hostname was not accepted");
            }
        }
        if !cfg.dhcp {
            let gw = cfg.gateway.map(|g| format!("<tds:IPv4Address>{g}</tds:IPv4Address>")).unwrap_or_default();
            if s.call(&format!("<tds:SetNetworkDefaultGateway>{gw}</tds:SetNetworkDefaultGateway>"), true).await.is_err() {
                notes.push("the gateway was not accepted");
            }
            let dns: String = cfg.dns.iter().map(|d| format!("<tds:DNSManual><tt:Type>IPv4</tt:Type><tt:IPv4Address>{d}</tt:IPv4Address></tds:DNSManual>")).collect();
            if s.call(&format!("<tds:SetDNS><tds:FromDHCP>false</tds:FromDHCP>{dns}</tds:SetDNS>"), true).await.is_err() {
                notes.push("DNS was not accepted");
            }
        }
        let ipv4 = if cfg.dhcp {
            "<tt:IPv4><tt:Enabled>true</tt:Enabled><tt:DHCP>true</tt:DHCP></tt:IPv4>".to_string()
        } else {
            format!("<tt:IPv4><tt:Enabled>true</tt:Enabled><tt:Manual><tt:Address>{}</tt:Address><tt:PrefixLength>{}</tt:PrefixLength></tt:Manual><tt:DHCP>false</tt:DHCP></tt:IPv4>", cfg.ip, cfg.prefix)
        };
        let body = format!("<tds:SetNetworkInterfaces><tds:InterfaceToken>{}</tds:InterfaceToken><tds:NetworkInterface><tt:Enabled>true</tt:Enabled>{ipv4}</tds:NetworkInterface></tds:SetNetworkInterfaces>", escape(&token));
        let r = s.call(&body, true).await;
        let reboot_needed = match &r {
            Ok(x) => xml_text(x, "RebootNeeded").map(|v| v == "true").unwrap_or(false),
            // A camera that moves at once may drop the connection before it answers.
            Err(DriverError::Http(meerkat_core::http::HttpError::Transport(_))) => false,
            Err(e) => return Err(DriverError::Protocol(e.to_string())),
        };
        if reboot_needed {
            s.call("<tds:SystemReboot/>", true).await?;
        }
        let mut message = if reboot_needed { "Saved; the camera is rebooting onto its new address".to_string() } else { "Saved; the camera has moved to its new address".to_string() };
        if !notes.is_empty() {
            message += &format!(" ({})", notes.join(", "));
        }
        Ok(WriteOutcome { message, restarts: reboot_needed, expect_ip: if cfg.dhcp { None } else { Some(cfg.ip) }, settle_ms: if reboot_needed { 25_000 } else { 2_000 } })
    }

    fn web_url(&self, cam: &Camera) -> Option<String> {
        // The device service often lives on its own port; the web UI is on 80.
        Some(format!("http://{}/", cam.ip))
    }
}

impl Onvif {
    async fn unicast(&self, ctx: &ScanCtx, targets: &[Ipv4Addr]) -> Result<Vec<Camera>> {
        let port = ctx.port("wsd", WSD_PORT);
        let s = Arc::new(udp_socket(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0), UdpOpts::default())?);
        let mut inbox = Inbox::new(std::slice::from_ref(&s));
        let id = uuid4();
        let xml = probe_xml(&id);
        for (n, ip) in targets.iter().enumerate() {
            if ctx.cancelled() {
                break;
            }
            let _ = s.send_to(xml.as_bytes(), ctx.to(*ip, port)).await;
            if n % 64 == 63 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        let deadline = Instant::now() + ctx.probe_timeout + Duration::from_millis(400);
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        while let Some((b, from)) = inbox.next_until(deadline).await {
            let SocketAddr::V4(from) = from else { continue };
            for m in parse_probe_matches(&String::from_utf8_lossy(&b)) {
                if seen.insert(from.ip().to_string() + m.endpoint.as_deref().unwrap_or("")) {
                    out.extend(camera_from_match(&m, *from.ip()));
                }
            }
        }
        let _ = xml_texts; // (re-exported helper kept for drivers that need lists)
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATCH: &str = r#"<?xml version="1.0" encoding="UTF-8"?><SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:wsa="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery"><SOAP-ENV:Header><wsa:RelatesTo>urn:uuid:abc</wsa:RelatesTo></SOAP-ENV:Header><SOAP-ENV:Body><d:ProbeMatches><d:ProbeMatch><wsa:EndpointReference><wsa:Address>urn:uuid:11111111-2222-3333-4444-555555555555</wsa:Address></wsa:EndpointReference><d:Types>dn:NetworkVideoTransmitter</d:Types><d:Scopes>onvif://www.onvif.org/type/video_encoder onvif://www.onvif.org/type/ptz onvif://www.onvif.org/name/CAM%201 onvif://www.onvif.org/hardware/AW-UE150 onvif://www.onvif.org/location/stage%20left onvif://www.onvif.org/Profile/Streaming onvif://www.onvif.org/MacAddress/00-80-45-11-22-33</d:Scopes><d:XAddrs>http://10.0.0.21:8080/onvif/device_service http://[fe80::1]/onvif/device_service</d:XAddrs><d:MetadataVersion>1</d:MetadataVersion></d:ProbeMatch></d:ProbeMatches></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

    #[test]
    fn probe_match_to_camera() {
        let ms = parse_probe_matches(MATCH);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].relates_to.as_deref(), Some("urn:uuid:abc"));
        let c = camera_from_match(&ms[0], Ipv4Addr::new(10, 0, 0, 21)).unwrap();
        assert_eq!(c.vendor, "Panasonic");
        assert_eq!(c.model.as_deref(), Some("AW-UE150"));
        assert_eq!(c.name.as_deref(), Some("CAM 1"));
        assert_eq!(c.mac.unwrap().to_string(), "00:80:45:11:22:33");
        assert_eq!(c.web_port, Some(8080));
        assert_eq!(c.fields["onvifXaddr"], "http://10.0.0.21:8080/onvif/device_service");
        assert_eq!(c.fields["onvifLocation"], "stage left");
        assert_eq!(c.fields["onvifType"], "PTZ");
    }

    #[test]
    fn password_digest_matches_the_oasis_formula() {
        // Known vector from the ONVIF Application Programmer's Guide §6.1.1.3:
        // nonce LKqI6G/AikKCQrN0zqZFlg==, created 2010-09-16T07:50:45Z, password userpassword.
        let nonce = [0x2c, 0xaa, 0x88, 0xe8, 0x6f, 0xc0, 0x8a, 0x42, 0x82, 0x42, 0xb3, 0x74, 0xce, 0xa6, 0x45, 0x96];
        let h = security_header(&Credentials { username: "user".into(), password: "userpassword".into() }, &nonce, "2010-09-16T07:50:45Z");
        assert!(h.contains(">tuOSpGlFlIXsozq4HFNeeGeFLEI=<"), "{h}");
        assert!(h.contains(">LKqI6G/AikKCQrN0zqZFlg==<"));
    }

    #[test]
    fn network_interfaces_reply() {
        let ifs = r#"<tds:GetNetworkInterfacesResponse><tds:NetworkInterfaces token="eth0"><tt:Enabled>true</tt:Enabled><tt:Info><tt:Name>eth0</tt:Name><tt:HwAddress>00:80:45:11:22:33</tt:HwAddress><tt:MTU>1500</tt:MTU></tt:Info><tt:IPv4><tt:Enabled>true</tt:Enabled><tt:Config><tt:Manual><tt:Address>10.0.0.21</tt:Address><tt:PrefixLength>24</tt:PrefixLength></tt:Manual><tt:DHCP>false</tt:DHCP></tt:Config></tt:IPv4></tds:NetworkInterfaces></tds:GetNetworkInterfacesResponse>"#;
        let gw = r#"<tds:NetworkGateway><tt:IPv4Address>10.0.0.1</tt:IPv4Address></tds:NetworkGateway>"#;
        let dns = r#"<tds:DNSInformation><tt:FromDHCP>false</tt:FromDHCP><tt:DNSManual><tt:Type>IPv4</tt:Type><tt:IPv4Address>1.1.1.1</tt:IPv4Address></tt:DNSManual></tds:DNSInformation>"#;
        let host = r#"<tds:HostnameInformation><tt:FromDHCP>false</tt:FromDHCP><tt:Name>cam-1</tt:Name></tds:HostnameInformation>"#;
        let (token, n, mac) = parse_network(ifs, Some(gw), Some(dns), Some(host)).unwrap();
        assert_eq!(token, "eth0");
        assert_eq!((n.ip, n.prefix, n.dhcp), (Ipv4Addr::new(10, 0, 0, 21), 24, false));
        assert_eq!(n.gateway, Some(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(n.dns, vec![Ipv4Addr::new(1, 1, 1, 1)]);
        assert_eq!(n.hostname.as_deref(), Some("cam-1"));
        assert_eq!(mac.unwrap().to_string(), "00:80:45:11:22:33");
    }
}
