//! BirdDog — the RESTful API on port 8080.
//!
//! Sources: BirdDog RESTful API 2.0 (https://birddog.tv/AV/API/), the X-series
//! API 2.1 Postman collection (https://documenter.getpostman.com/view/29602224/2sAYHxn45i),
//! and Bitfocus Companion's birddog-ptz module.
//!
//! - **Discovery.** BirdDog has no broadcast protocol of its own; its cameras
//!   announce themselves as NDI sources over mDNS. The NDI driver hears them and
//!   the engine hands each address to this driver's probe, which confirms it
//!   over the API. Routed subnets are found by sweeping.
//! - **Identify:** `GET /version` (plain text `BirdDog X1`) first — cheap, and
//!   safe on a KBD controller, which drops the connection on `/about` — then
//!   `GET /about` (JSON; `SerialNumber` is the MAC).
//! - **Network settings:** `GET`/`POST /EthSetup` exists from API 2.1 (X-series
//!   and MAX on 6.x firmware). P-series and the converters stop at 5.5.x, whose
//!   API has no network endpoint: they show read-only here and their own web UI
//!   (port 80, password `birddog`) is where the address changes.
//! - **No auth** on the API in either version.

use std::net::Ipv4Addr;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::driver::{Recovery, Result};
use meerkat_core::net::{mask_to_prefix, prefix_to_mask};
use meerkat_core::{Caps, Camera, Driver, DriverError, DriverInfo, Mac, NetConfig, OpCtx, ScanCtx, WriteOutcome};
use serde_json::{json, Value};

pub struct BirdDog;

pub const ID: &str = "birddog";
const API_PORT: u16 = 8080;

impl BirdDog {
    fn base(ports: impl Fn(&str, u16) -> u16, ip: Ipv4Addr) -> String {
        format!("http://{ip}:{}", ports("api", API_PORT))
    }
}

/// The camera an `/about` reply describes, or None if it is not a BirdDog.
pub fn from_about(ip: Ipv4Addr, version: Option<&str>, about: &Value) -> Option<Camera> {
    let s = |k: &str| about.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let hw = s("HardwareVersion");
    let fw = s("FirmwareVersion");
    let host = s("HostName").or_else(|| s("MyHostName"));
    let is_bd = [hw.as_deref(), fw.as_deref(), host.as_deref(), version]
        .iter()
        .flatten()
        .any(|v| v.to_ascii_lowercase().contains("birddog"));
    if !is_bd {
        return None;
    }
    let mut cam = Camera::new(ID, "BirdDog", ip);
    let model = hw.clone().or_else(|| version.map(str::to_string)).map(|m| m.trim_start_matches("BirdDog").trim().to_string());
    cam.model = model.clone().filter(|m| !m.is_empty());
    // FirmwareVersion is "BirdDog X1 Ultra 5.6.057": keep the number.
    cam.firmware = fw.as_deref().and_then(|f| f.split_whitespace().last()).map(str::to_string);
    cam.mac = s("SerialNumber").and_then(|m| m.parse::<Mac>().ok());
    cam.serial = s("SerialNumber");
    cam.name = host.clone();
    cam.web_port = Some(80);
    let addr = s("IPAddress").and_then(|a| a.parse::<Ipv4Addr>().ok()).unwrap_or(ip);
    let prefix = s("NetworkMask").and_then(|m| m.parse::<Ipv4Addr>().ok()).and_then(mask_to_prefix);
    if let Some(prefix) = prefix {
        cam.net = Some(NetConfig {
            dhcp: s("NetworkConfigMethod").map(|m| m.eq_ignore_ascii_case("dhcp")).unwrap_or(false),
            ip: addr,
            prefix,
            gateway: s("GateWay").and_then(|g| g.parse().ok()),
            dns: s("Dns").or_else(|| s("DNS")).and_then(|d| d.parse().ok()).into_iter().collect(),
            hostname: host.clone(),
        });
    }
    cam.caps = Caps { read_network: true, ..Caps::default() };
    for (k, label) in [("Format", "ndiName"), ("Status", "status"), ("FallbackIP", "fallbackIp"), ("MCUVersion", "mcuVersion"), ("HostName", "hostname")] {
        if let Some(v) = s(k) {
            cam.fields.insert(label.into(), v);
        }
    }
    if let Some(fw) = &fw {
        cam.fields.insert("firmwareFull".into(), fw.clone());
    }
    Some(cam)
}

fn eth_to_net(v: &Value) -> Option<NetConfig> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty());
    let ip: Ipv4Addr = s("IpAddr")?.parse().ok()?;
    let prefix = mask_to_prefix(s("NetMask")?.parse().ok()?)?;
    Some(NetConfig {
        dhcp: s("IpMode").map(|m| m.eq_ignore_ascii_case("dhcp")).unwrap_or(false),
        ip,
        prefix,
        gateway: s("GateWayIp").and_then(|g| g.parse().ok()).filter(|g: &Ipv4Addr| !g.is_unspecified()),
        dns: [s("DnsSvrIpFirst"), s("DnsSvrIpSecond")].into_iter().flatten().filter_map(|d| d.parse().ok()).collect(),
        hostname: None,
    })
}

#[async_trait]
impl Driver for BirdDog {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: ID,
            vendor: "BirdDog",
            protocol: "BirdDog RESTful API",
            priority: 60,
            discovery: vec!["Found through NDI (mDNS) and confirmed with GET :8080/about"],
            sweep: Some("HTTP GET :8080/version, then /about"),
            notes: vec![
                "Network settings can be changed over the API on X-series and MAX (firmware 6.x, /EthSetup). P-series and converters (5.5.x) are read-only here: change them in the web UI's Network page.",
                "Out of the box a BirdDog uses DHCP and falls back to 192.168.100.100/24 after about 30 seconds without a lease.",
                "The camera reboots to apply a new address; Meerkat waits for it to come back.",
            ],
            enabled_by_default: true,
            recovery: Some(Recovery {
                tool: "BirdDog Neuralyzer",
                url: "https://birddog.tv/neuralyzer",
                models: "P-series",
                does: "Resets a camera's network settings to the factory defaults, for a camera that no longer answers at the address it was given.",
                steps: vec![
                    "Download BirdDog Neuralyzer from BirdDog and run it on a computer on the same network switch as the camera. BirdDog says the camera needs current firmware for it to work.",
                    "Use it to reset the camera's network settings.",
                    "Come back here and press Watch for it: Meerkat scans until the camera turns up again.",
                ],
                after: "Out of the box a BirdDog uses DHCP, and falls back to 192.168.100.100/24 when no DHCP server answers.",
                fallback: Some(std::net::Ipv4Addr::new(192, 168, 100, 100)),
            }),
        }
    }

    fn can_probe(&self) -> bool {
        true
    }

    async fn probe(&self, ctx: &ScanCtx, ip: Ipv4Addr) -> Result<Option<Camera>> {
        let base = Self::base(|w, d| ctx.port(w, d), ip);
        let version = match ctx.http.get(&format!("{base}/version"), None).await {
            Ok(r) => r.body.trim().to_string(),
            Err(_) => return Ok(None),
        };
        if !version.to_ascii_lowercase().contains("birddog") {
            return Ok(None);
        }
        if version.to_ascii_uppercase().contains("KBD") {
            let mut c = Camera::new(ID, "BirdDog", ip);
            c.model = Some(version.trim_start_matches("BirdDog").trim().to_string());
            c.web_port = Some(80);
            return Ok(Some(c.field("note", "PTZ keyboard controller")));
        }
        let about: Value = match ctx.http.get(&format!("{base}/about"), None).await {
            Ok(r) => serde_json::from_str(&r.body).unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };
        let mut cam = match from_about(ip, Some(&version), &about) {
            Some(c) => c,
            None => {
                let mut c = Camera::new(ID, "BirdDog", ip);
                c.model = Some(version.trim_start_matches("BirdDog").trim().to_string());
                c.web_port = Some(80);
                c.field("api", "legacy (upgrade the firmware for the API)")
            }
        };
        // API 2.1 has /EthSetup; its presence is what makes the address writable.
        if let Ok(r) = ctx.http.get(&format!("{base}/EthSetup"), None).await {
            if let Ok(v) = serde_json::from_str::<Value>(&r.body) {
                if let Some(net) = eth_to_net(&v) {
                    cam.net = Some(NetConfig { hostname: cam.net.as_ref().and_then(|n| n.hostname.clone()), ..net });
                    cam.caps = Caps { read_network: true, write_network: true, dhcp: true, dns: true, reboot: true, ..Caps::default() };
                    cam.fields.insert("api".into(), "2.1 (network settings over the API)".into());
                }
            }
        }
        cam.fields.entry("api".into()).or_insert_with(|| "2.0 (no network endpoint: use the web UI)".into());
        Ok(Some(cam))
    }

    async fn read_network(&self, op: &OpCtx, cam: &Camera) -> Result<NetConfig> {
        let base = Self::base(|w, d| op.port(w, d), cam.ip);
        if cam.caps.write_network {
            let r = op.http.get(&format!("{base}/EthSetup"), None).await?;
            let v: Value = serde_json::from_str(&r.body).map_err(|e| DriverError::Protocol(format!("/EthSetup: {e}")))?;
            return eth_to_net(&v).ok_or_else(|| DriverError::Protocol("/EthSetup reply has no address".into()));
        }
        let r = op.http.get(&format!("{base}/about"), None).await?;
        let v: Value = serde_json::from_str(&r.body).map_err(|e| DriverError::Protocol(format!("/about: {e}")))?;
        from_about(cam.ip, None, &v).and_then(|c| c.net).ok_or_else(|| DriverError::Protocol("/about reply has no address".into()))
    }

    async fn write_network(&self, op: &OpCtx, cam: &Camera, cfg: &NetConfig) -> Result<WriteOutcome> {
        if !cam.caps.write_network {
            return Err(DriverError::Unsupported("change this model's address over the API — use its web UI"));
        }
        let base = Self::base(|w, d| op.port(w, d), cam.ip);
        let mut body = json!({ "IpMode": if cfg.dhcp { "dhcp" } else { "static" } });
        if !cfg.dhcp {
            body["IpAddr"] = json!(cfg.ip.to_string());
            body["NetMask"] = json!(prefix_to_mask(cfg.prefix).to_string());
            body["GateWayIp"] = json!(cfg.gateway.map(|g| g.to_string()).unwrap_or_default());
            body["DnsSvrIpFirst"] = json!(cfg.dns.first().map(|d| d.to_string()).unwrap_or_default());
            body["DnsSvrIpSecond"] = json!(cfg.dns.get(1).map(|d| d.to_string()).unwrap_or_default());
        }
        op.http.post(&format!("{base}/EthSetup"), "application/json", body.to_string().into_bytes(), None).await?;
        // Read it back while the old address still answers: proof it took.
        let back = op.http.get(&format!("{base}/EthSetup"), None).await.ok().and_then(|r| serde_json::from_str::<Value>(&r.body).ok()).and_then(|v| eth_to_net(&v));
        if let Some(b) = &back {
            if !cfg.dhcp && (b.ip != cfg.ip || b.prefix != cfg.prefix) {
                return Err(DriverError::Protocol(format!("the camera accepted the request but still reports {}/{}", b.ip, b.prefix)));
            }
        }
        // The API documents no live apply; a reboot brings the new address up.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let rebooted = op.http.post(&format!("{base}/reboot"), "application/json", b"{}".to_vec(), None).await.is_ok()
            || op.http.get(&format!("{base}/reboot"), None).await.is_ok();
        Ok(WriteOutcome {
            message: if rebooted { "Saved; the camera is rebooting onto its new address".into() } else { "Saved; reboot the camera to apply it".into() },
            restarts: rebooted,
            expect_ip: if cfg.dhcp { None } else { Some(cfg.ip) },
            settle_ms: if rebooted { 20_000 } else { 2_000 },
        })
    }

    fn web_url(&self, cam: &Camera) -> Option<String> {
        Some(format!("http://{}/", cam.ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_from_the_x1_ultra_example() {
        // BirdDog's published X-series API example, verbatim.
        let v: Value = serde_json::from_str(r#"{"FallbackIP":"192.168.100.100","FirmwareVersion":"BirdDog X1 Ultra 5.6.057","Format":"CAM HX","GateWay":"192.168.23.1","HardwareVersion":"BirdDog X1 Ultra","HostName":"birddog-608F9","IPAddress":"192.168.23.75","MCUVersion":"5","NetworkConfigMethod":"static","NetworkMask":"255.255.255.0","SerialNumber":"8027b62608f9","Status":"Active","WifiConfigMethod":"static","WifiIPAddress":"192.168.100.101","WifiMask":"255.255.255.0","Dns":""}"#).unwrap();
        let c = from_about("192.168.23.75".parse().unwrap(), Some("BirdDog X1"), &v).unwrap();
        assert_eq!(c.model.as_deref(), Some("X1 Ultra"));
        assert_eq!(c.firmware.as_deref(), Some("5.6.057"));
        assert_eq!(c.mac.unwrap().to_string(), "80:27:b6:26:08:f9");
        assert_eq!(c.name.as_deref(), Some("birddog-608F9"));
        let n = c.net.unwrap();
        assert!(!n.dhcp);
        assert_eq!((n.prefix, n.gateway), (24, Some("192.168.23.1".parse().unwrap())));
        assert!(n.dns.is_empty());
        assert_eq!(c.fields["ndiName"], "CAM HX");
    }

    #[test]
    fn not_a_birddog() {
        let v: Value = serde_json::from_str(r#"{"HostName":"printer","FirmwareVersion":"1.0"}"#).unwrap();
        assert!(from_about("10.0.0.9".parse().unwrap(), Some("HP LaserJet"), &v).is_none());
    }

    #[test]
    fn ethsetup_from_the_published_example() {
        let v: Value = serde_json::from_str(r#"{"IpMode":"dhcp","IpAddr":"192.168.20.46","NetMask":"255.255.255.0","GateWayIp":"192.168.20.20","DnsSvrIpFirst":"9.9.9.9","DnsSvrIpSecond":"1.1.1.1","DhcpTimeout":"30","DhcpFallbackIPAddr":"192.168.100.100","DhcpFallbackNetMask":"255.255.255.0","DhcpFallbackGateWayIp":"192.168.1.1"}"#).unwrap();
        let n = eth_to_net(&v).unwrap();
        assert!(n.dhcp);
        assert_eq!(n.dns.len(), 2);
        assert_eq!(n.ip, "192.168.20.46".parse::<Ipv4Addr>().unwrap());
    }
}
