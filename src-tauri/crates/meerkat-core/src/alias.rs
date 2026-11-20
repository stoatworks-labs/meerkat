//! Temporary addresses: a second IPv4 address on one of this computer's
//! interfaces, in the subnet of a camera that is on the wrong one.
//!
//! The layer-2 protocols (Panasonic Easy IP, Sony IP Setup) re-address a camera
//! on the wrong subnet by MAC. Everything that speaks HTTP (ONVIF, BirdDog's
//! API, every camera's own web page) cannot: a camera at 192.168.100.100 is
//! heard on a laptop at 10.0.0.5, but packets to it go to the default gateway.
//! A temporary address on that interface in the camera's subnet puts the two
//! on one wire, so HTTP reaches it and its web page opens.
//!
//! The address is added the operating system's own way, with the user's admin
//! password (macOS asks through its own dialog, Linux through polkit, Windows
//! through UAC), and none of the three ways survives a restart:
//!
//! - macOS: `ifconfig <if> alias <ip> netmask <mask>` / `-alias`
//! - Linux: `ip addr add <ip>/<prefix> dev <if>` / `del`
//! - Windows: `New-NetIPAddress -PolicyStore ActiveStore -SkipAsSource` /
//!   `Remove-NetIPAddress`
//!
//! Meerkat removes the ones it added when it quits.
//!
//! The subnet is never wider than a /24 around the camera, so the route it adds
//! cannot swallow traffic to anything else, and it is refused if it overlaps a
//! network this computer is already on.

use std::net::Ipv4Addr;
use std::process::Stdio;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::model::{Camera, Reach};
use crate::net::{self, LocalIface};

/// The narrowest subnet a temporary address may claim: wider and its route
/// starts to capture traffic meant for other networks.
pub const MIN_PREFIX: u8 = 24;
pub const MAX_PREFIX: u8 = 30;

/// A temporary address on interface `iface`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    pub iface: String,
    /// The interface's index, which Windows wants instead of its name.
    #[serde(default)]
    pub index: Option<u32>,
    pub ip: Ipv4Addr,
    pub prefix: u8,
}

impl Alias {
    pub fn cidr(&self) -> String {
        format!("{}/{}", self.ip, self.prefix)
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        net::in_subnet(ip, self.ip, self.prefix)
    }

    /// How the interface list shows it once it is up.
    pub fn as_iface(&self) -> LocalIface {
        LocalIface {
            name: self.iface.clone(),
            index: self.index,
            ip: self.ip,
            prefix: self.prefix,
            broadcast: Some(net::broadcast(self.ip, self.prefix)),
            loopback: false,
            p2p: false,
            link_local: false,
        }
    }

    pub fn is(&self, i: &LocalIface) -> bool {
        i.name == self.iface && i.ip == self.ip
    }
}

/// The address to add so this computer can reach `cam`: on the interface it
/// was heard on, in its subnet (at most a /24), on a host number nothing in
/// `taken` uses. Only a camera heard at layer 2 on the wrong subnet needs one.
pub fn plan(cam: &Camera, all: &[LocalIface], taken: &[Ipv4Addr]) -> Result<Alias, String> {
    let Reach::Mismatch { iface } = &cam.reach else {
        return Err(match &cam.reach {
            Reach::Local { iface } => format!("{} is already on {iface}'s subnet", cam.ip),
            Reach::Routed => format!("{} is reached through a router; a temporary address would not help", cam.ip),
            _ => format!("{} was not heard on any interface", cam.ip),
        });
    };
    let on = all.iter().find(|i| &i.name == iface && i.usable()).ok_or(format!("{iface} is no longer up"))?;
    let prefix = cam.net.as_ref().map(|n| n.prefix).unwrap_or(MIN_PREFIX).clamp(MIN_PREFIX, MAX_PREFIX);
    let lo = u32::from(net::network(cam.ip, prefix));
    let hi = u32::from(net::broadcast(cam.ip, prefix));
    let gateway = cam.net.as_ref().and_then(|n| n.gateway);
    // From the top of the subnet down, skipping .254 and .1 (where gateways
    // usually are), the camera, its gateway and every camera in the table.
    let pick = (lo + 1..hi)
        .rev()
        .map(Ipv4Addr::from)
        .filter(|ip| {
            let last = ip.octets()[3];
            prefix < 24 || (last != 254 && last != 1)
        })
        .find(|ip| *ip != cam.ip && Some(*ip) != gateway && !taken.contains(ip))
        .ok_or(format!("no free address left in {}/{prefix}", net::network(cam.ip, prefix)))?;
    let a = Alias { iface: on.name.clone(), index: on.index, ip: pick, prefix };
    check(&a, all)?;
    Ok(a)
}

/// Everything that must hold before an address is added.
pub fn check(a: &Alias, all: &[LocalIface]) -> Result<(), String> {
    if !(MIN_PREFIX..=MAX_PREFIX).contains(&a.prefix) {
        return Err(format!("a temporary address must be a /{MIN_PREFIX} to a /{MAX_PREFIX}, not a /{}", a.prefix));
    }
    if !safe_name(&a.iface) {
        return Err(format!("{:?} is not an interface name Meerkat will pass to the system", a.iface));
    }
    let on = all.iter().find(|i| i.name == a.iface).ok_or(format!("there is no interface called {}", a.iface))?;
    if !on.usable() {
        return Err(format!("{} is a loopback or tunnel interface", a.iface));
    }
    let ip = a.ip;
    if ip.is_loopback() || ip.is_multicast() || ip.is_broadcast() || ip.is_unspecified() || ip.is_link_local() {
        return Err(format!("{ip} cannot be a temporary address"));
    }
    if ip == net::network(ip, a.prefix) || ip == net::broadcast(ip, a.prefix) {
        return Err(format!("{ip} is the network or broadcast address of its /{}", a.prefix));
    }
    if let Some(i) = all.iter().find(|i| !i.loopback && overlaps(i.ip, i.prefix, ip, a.prefix)) {
        return Err(format!("{} overlaps {} on {}, a network this computer is already on", a.cidr(), i.cidr(), i.name));
    }
    Ok(())
}

fn overlaps(a: Ipv4Addr, ap: u8, b: Ipv4Addr, bp: u8) -> bool {
    let p = ap.min(bp);
    net::network(a, p) == net::network(b, p)
}

/// Letters, digits, `.`, `_`, `-`, and on Windows spaces and brackets
/// ("Ethernet 2", "Wi-Fi (USB)"). Nothing a shell would read.
fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || "._- ()".contains(c)) && !s.starts_with('-')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    pub fn this() -> Option<Os> {
        match std::env::consts::OS {
            "macos" => Some(Os::Mac),
            "linux" => Some(Os::Linux),
            "windows" => Some(Os::Windows),
            _ => None,
        }
    }
}

/// The program and arguments that add (or, with `add` false, remove) every
/// address in `list` behind one admin prompt.
pub fn command(os: Os, add: bool, list: &[Alias]) -> Result<(String, Vec<String>), String> {
    for a in list {
        if !safe_name(&a.iface) {
            return Err(format!("{:?} is not an interface name Meerkat will pass to the system", a.iface));
        }
    }
    Ok(match os {
        Os::Mac => {
            let sh: Vec<String> = list
                .iter()
                .map(|a| {
                    if add {
                        format!("/sbin/ifconfig {} alias {} netmask {}", a.iface, a.ip, net::prefix_to_mask(a.prefix))
                    } else {
                        format!("/sbin/ifconfig {} -alias {}", a.iface, a.ip)
                    }
                })
                .collect();
            let script = format!("do shell script \"{}\" with administrator privileges", sh.join(" ; "));
            ("/usr/bin/osascript".into(), vec!["-e".into(), script])
        }
        Os::Linux => {
            let sh: Vec<String> = list.iter().map(|a| format!("ip addr {} {} dev {}", if add { "add" } else { "del" }, a.cidr(), a.iface)).collect();
            ("pkexec".into(), vec!["/bin/sh".into(), "-c".into(), sh.join(" ; ")])
        }
        Os::Windows => {
            let mut ps = Vec::new();
            for a in list {
                let index = a.index.ok_or(format!("{} has no interface index", a.iface))?;
                ps.push(if add {
                    format!("New-NetIPAddress -InterfaceIndex {index} -IPAddress {} -PrefixLength {} -SkipAsSource $true -PolicyStore ActiveStore -ErrorAction Stop | Out-Null", a.ip, a.prefix)
                } else {
                    format!("Remove-NetIPAddress -InterfaceIndex {index} -IPAddress {} -Confirm:$false -ErrorAction Stop", a.ip)
                });
            }
            // The elevated half runs from an encoded command, so nothing in it
            // passes through a second round of quoting.
            let inner = base64_utf16le(&ps.join("; "));
            let outer = format!(
                "$p = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{inner}'; exit $p.ExitCode"
            );
            ("powershell.exe".into(), vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), outer])
        }
    })
}

/// What PowerShell's `-EncodedCommand` takes: base64 of the UTF-16LE text.
fn base64_utf16le(s: &str) -> String {
    let bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    crate::http::base64(&bytes)
}

/// Who adds and removes the addresses.
#[async_trait]
pub trait AliasBackend: Send + Sync {
    async fn add(&self, list: &[Alias]) -> Result<(), String>;
    async fn remove(&self, list: &[Alias]) -> Result<(), String>;
}

/// The operating system, behind its admin prompt.
pub struct SystemAliases;

#[async_trait]
impl AliasBackend for SystemAliases {
    async fn add(&self, list: &[Alias]) -> Result<(), String> {
        run(true, list).await
    }

    async fn remove(&self, list: &[Alias]) -> Result<(), String> {
        run(false, list).await
    }
}

async fn run(add: bool, list: &[Alias]) -> Result<(), String> {
    if list.is_empty() {
        return Ok(());
    }
    let os = Os::this().ok_or("temporary addresses are not supported on this operating system")?;
    let (prog, args) = command(os, add, list)?;
    let out = tokio::process::Command::new(&prog)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("could not run {prog}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    // osascript: "User canceled. (-128)"; pkexec exits 126 when dismissed.
    if err.contains("-128") || out.status.code() == Some(126) {
        return Err("cancelled: the admin password was not given".into());
    }
    let verb = if add { "add" } else { "remove" };
    Err(if err.is_empty() { format!("could not {verb} the address ({})", out.status) } else { format!("could not {verb} the address: {err}") })
}

/// The simulator's stand-in: addresses exist only in the engine's own list.
pub struct NoAliases;

#[async_trait]
impl AliasBackend for NoAliases {
    async fn add(&self, _: &[Alias]) -> Result<(), String> {
        Ok(())
    }

    async fn remove(&self, _: &[Alias]) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Method, NetConfig, Sighting};

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    fn en0() -> LocalIface {
        LocalIface { name: "en0".into(), index: Some(7), ip: ip("10.40.7.80"), prefix: 24, broadcast: Some(ip("10.40.7.255")), loopback: false, p2p: false, link_local: false }
    }

    fn lost(at: &str, prefix: Option<u8>) -> Camera {
        let mut c = Camera::new("ndi", "BirdDog", ip(at));
        c.seen.push(Sighting { driver: "ndi".into(), method: Method::Mdns, via: "en0".into() });
        c.reach = Reach::Mismatch { iface: "en0".into() };
        c.net = prefix.map(|p| NetConfig { dhcp: false, ip: ip(at), prefix: p, gateway: Some(ip("192.168.100.1")), dns: vec![], hostname: None });
        c
    }

    #[test]
    fn a_plan_sits_in_the_cameras_slash_24_on_the_interface_that_heard_it() {
        let a = plan(&lost("192.168.100.100", None), &[en0()], &[]).unwrap();
        assert_eq!(a, Alias { iface: "en0".into(), index: Some(7), ip: ip("192.168.100.253"), prefix: 24 });
        assert!(a.contains(ip("192.168.100.100")));
    }

    #[test]
    fn a_plan_skips_taken_addresses_and_never_widens_past_a_slash_24() {
        let a = plan(&lost("172.16.5.10", Some(16)), &[en0()], &[ip("172.16.5.253"), ip("172.16.5.252")]).unwrap();
        assert_eq!((a.ip, a.prefix), (ip("172.16.5.251"), 24));
        let narrow = plan(&lost("192.168.100.100", Some(28)), &[en0()], &[]).unwrap();
        assert_eq!((narrow.ip, narrow.prefix), (ip("192.168.100.110"), 28), "inside 192.168.100.96/28");
    }

    #[test]
    fn only_a_camera_on_the_wrong_subnet_gets_one() {
        let mut c = lost("10.40.7.9", None);
        c.reach = Reach::Local { iface: "en0".into() };
        assert!(plan(&c, &[en0()], &[]).unwrap_err().contains("already on"));
        c.reach = Reach::Routed;
        assert!(plan(&c, &[en0()], &[]).unwrap_err().contains("router"));
    }

    #[test]
    fn checks_refuse_overlaps_wide_subnets_and_odd_names() {
        let all = [en0()];
        let ok = Alias { iface: "en0".into(), index: Some(7), ip: ip("192.168.100.253"), prefix: 24 };
        assert_eq!(check(&ok, &all), Ok(()));
        assert!(check(&Alias { prefix: 16, ..ok.clone() }, &all).unwrap_err().contains("/24 to a /30"));
        assert!(check(&Alias { ip: ip("10.40.7.200"), ..ok.clone() }, &all).unwrap_err().contains("overlaps"));
        assert!(check(&Alias { ip: ip("192.168.100.255"), ..ok.clone() }, &all).unwrap_err().contains("broadcast"));
        assert!(check(&Alias { iface: "en0; rm -rf ~".into(), ..ok.clone() }, &all).unwrap_err().contains("not an interface name"));
        assert!(check(&Alias { iface: "en9".into(), ..ok }, &all).unwrap_err().contains("no interface"));
    }

    #[test]
    fn the_commands_each_system_runs() {
        let a = [Alias { iface: "en0".into(), index: Some(7), ip: ip("192.168.100.253"), prefix: 24 }];
        let (p, args) = command(Os::Mac, true, &a).unwrap();
        assert_eq!(p, "/usr/bin/osascript");
        assert_eq!(args[1], r#"do shell script "/sbin/ifconfig en0 alias 192.168.100.253 netmask 255.255.255.0" with administrator privileges"#);
        let two = [a[0].clone(), Alias { ip: ip("192.168.0.253"), ..a[0].clone() }];
        assert_eq!(command(Os::Mac, false, &two).unwrap().1[1], r#"do shell script "/sbin/ifconfig en0 -alias 192.168.100.253 ; /sbin/ifconfig en0 -alias 192.168.0.253" with administrator privileges"#);
        assert_eq!(command(Os::Linux, true, &a).unwrap(), ("pkexec".into(), vec!["/bin/sh".into(), "-c".into(), "ip addr add 192.168.100.253/24 dev en0".into()]));
        let (p, args) = command(Os::Windows, true, &a).unwrap();
        assert_eq!(p, "powershell.exe");
        let enc = args[3].split('\'').nth(7).unwrap();
        assert_eq!(enc, base64_utf16le("New-NetIPAddress -InterfaceIndex 7 -IPAddress 192.168.100.253 -PrefixLength 24 -SkipAsSource $true -PolicyStore ActiveStore -ErrorAction Stop | Out-Null"));
    }

    #[test]
    fn base64_matches_powershells_encoding() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("dir"))
        assert_eq!(base64_utf16le("dir"), "ZABpAHIA");
        assert_eq!(base64_utf16le("a"), "YQA=");
    }
}
