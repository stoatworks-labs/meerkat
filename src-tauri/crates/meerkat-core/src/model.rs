//! What Meerkat knows about a camera. These are also the JSON shapes the UI
//! gets (`src/types.ts` mirrors them), so field names are camelCase on the wire.

use std::collections::BTreeMap;
use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::catalog::CatalogMatch;

/// A 48-bit hardware address. Printed `aa:bb:cc:dd:ee:ff`; parsed from that,
/// from dashes, or from twelve bare hex digits, in either case.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Mac(pub [u8; 6]);

impl Mac {
    pub fn octets(&self) -> [u8; 6] {
        self.0
    }

    /// Upper-case, separated by `sep` — the shape several vendor protocols
    /// want back (Sony: `-`, Panasonic's tool shows `:`).
    pub fn to_upper(&self, sep: &str) -> String {
        self.0.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(sep)
    }

    /// The low three octets as hex, which several vendors put in the
    /// factory hostname (`birddog-1a2b3c`, `AW-UE150-1A2B3C`).
    pub fn tail(&self) -> String {
        format!("{:02x}{:02x}{:02x}", self.0[3], self.0[4], self.0[5])
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2], b[3], b[4], b[5])
    }
}

impl fmt::Debug for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Mac {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        let seps_ok = s.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '-' | '.' | ' '));
        if hex.len() != 12 || !seps_ok {
            return Err(format!("not a MAC address: {s:?}"));
        }
        let mut out = [0u8; 6];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
        }
        Ok(Mac(out))
    }
}

impl Serialize for Mac {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Mac {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A camera's IPv4 settings — what the network panel shows and what a write
/// sends. `prefix` rather than a mask: a mask that is not a prefix is not a
/// valid setting, and the UI converts for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetConfig {
    pub dhcp: bool,
    pub ip: Ipv4Addr,
    pub prefix: u8,
    #[serde(default)]
    pub gateway: Option<Ipv4Addr>,
    #[serde(default)]
    pub dns: Vec<Ipv4Addr>,
    /// Hostname or camera title, for drivers whose protocol carries one
    /// alongside the address (Sony's NAME, Panasonic's camera title).
    #[serde(default)]
    pub hostname: Option<String>,
}

impl NetConfig {
    pub fn mask(&self) -> Ipv4Addr {
        crate::net::prefix_to_mask(self.prefix)
    }
}

/// How a camera was heard from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    /// A vendor broadcast protocol (Panasonic Easy IP, Sony IP Setup). These
    /// work at layer 2, so they find a camera whatever its address is.
    Broadcast,
    /// Multicast discovery (ONVIF WS-Discovery).
    Multicast,
    /// mDNS / DNS-SD (NDI, `_http._tcp`).
    Mdns,
    /// A probe sent to one address — how routed subnets are swept.
    Unicast,
}

/// One sighting: which driver heard the camera, how, and on what.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sighting {
    pub driver: String,
    pub method: Method,
    /// The interface name for broadcast/multicast/mDNS, or the range a
    /// sweep was given (`10.20.0.0/24`).
    pub via: String,
}

/// Where the camera's address sits relative to this computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reach {
    /// In the subnet of one of our interfaces: talk to it directly.
    Local { iface: String },
    /// Not in any local subnet, but it answered a unicast probe, so a router
    /// gets us there.
    Routed,
    /// Heard at layer 2 on `iface`, but its address is outside that
    /// interface's subnet — the factory-default camera on the wrong network.
    /// HTTP will not reach it; a layer-2 protocol can still re-address it.
    Mismatch { iface: String },
    #[default]
    Unknown,
}

/// What the owning driver can do with this camera.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    pub read_network: bool,
    pub write_network: bool,
    /// The write goes over a layer-2 protocol keyed by MAC, so it works when
    /// the camera's current address is unreachable (Reach::Mismatch).
    pub write_by_mac: bool,
    /// Writes need a username and password.
    pub needs_auth: bool,
    pub dhcp: bool,
    pub dns: bool,
    pub hostname: bool,
    pub reboot: bool,
}

/// One camera, merged across every driver that found it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    /// Stable key: `mac:…` when the MAC is known, else `ip:…`.
    pub id: String,
    /// The driver that administers it (the highest-priority one that saw it).
    pub driver: String,
    pub vendor: String,
    pub model: Option<String>,
    /// The camera's own name (title, NDI name, hostname).
    pub name: Option<String>,
    pub mac: Option<Mac>,
    pub ip: Ipv4Addr,
    /// Network settings, when discovery carried them (Easy IP and Sony do).
    pub net: Option<NetConfig>,
    pub firmware: Option<String>,
    pub serial: Option<String>,
    /// Port of the built-in web UI (80 unless the driver knows better).
    pub web_port: Option<u16>,
    pub web_url: Option<String>,
    pub caps: Caps,
    /// Everything else the protocol said, verbatim, for the detail panel.
    pub fields: BTreeMap<String, String>,
    pub seen: Vec<Sighting>,
    pub reach: Reach,
    /// Milliseconds since the Unix epoch of the latest sighting.
    pub last_seen: u64,
    /// Which rows of the catalogue (factory logins, firmware pages) are about
    /// it. Filled in by the engine from the vendor and model.
    #[serde(default)]
    pub catalog: CatalogMatch,
}

impl Camera {
    /// A camera as a driver first reports it. The engine fills in `id`,
    /// `reach`, `web_url` and `last_seen`.
    pub fn new(driver: &str, vendor: &str, ip: Ipv4Addr) -> Self {
        Camera {
            id: String::new(),
            driver: driver.to_string(),
            vendor: vendor.to_string(),
            model: None,
            name: None,
            mac: None,
            ip,
            net: None,
            firmware: None,
            serial: None,
            web_port: None,
            web_url: None,
            caps: Caps::default(),
            fields: BTreeMap::new(),
            seen: Vec::new(),
            reach: Reach::Unknown,
            last_seen: 0,
            catalog: CatalogMatch::default(),
        }
    }

    pub fn key(&self) -> String {
        match self.mac {
            Some(m) => format!("mac:{m}"),
            None => format!("ip:{}", self.ip),
        }
    }

    pub fn field(mut self, k: &str, v: impl Into<String>) -> Self {
        let v = v.into();
        if !v.is_empty() {
            self.fields.insert(k.to_string(), v);
        }
        self
    }
}

/// A username and password for a write (or a read that needs one).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_parses_every_usual_shape() {
        let want = Mac([0x00, 0x80, 0x45, 0x1a, 0x2b, 0x3c]);
        for s in ["00:80:45:1a:2b:3c", "00-80-45-1A-2B-3C", "0080451A2B3C", "0080.451a.2b3c"] {
            assert_eq!(s.parse::<Mac>().unwrap(), want, "{s}");
        }
        assert!("00:80:45:1a:2b".parse::<Mac>().is_err());
        assert!("00:80:45:1a:2b:3g".parse::<Mac>().is_err());
        assert_eq!(want.to_string(), "00:80:45:1a:2b:3c");
        assert_eq!(want.to_upper("-"), "00-80-45-1A-2B-3C");
        assert_eq!(want.tail(), "1a2b3c");
    }

    #[test]
    fn camera_key_prefers_mac() {
        let mut c = Camera::new("x", "X", Ipv4Addr::new(10, 0, 0, 5));
        assert_eq!(c.key(), "ip:10.0.0.5");
        c.mac = Some(Mac([2, 0, 0, 0, 0, 1]));
        assert_eq!(c.key(), "mac:02:00:00:00:00:01");
    }
}
