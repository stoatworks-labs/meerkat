//! Local interfaces, subnet maths, address ranges, UDP sockets and settings
//! validation.
//!
//! **How Meerkat reaches past its own subnet.** Three ways, and a scan uses all
//! that apply:
//!
//! 1. *Layer-2 broadcast, per interface.* A vendor's broadcast protocol is sent
//!    out of every chosen interface to 255.255.255.255 (and that interface's
//!    directed broadcast). Every camera on the wire hears it whatever its
//!    address, and the Panasonic and Sony protocols answer by broadcast too, so
//!    a camera on 192.168.0.10 shows up on a laptop on 10.0.0.0/24. Each send
//!    socket is pinned to its interface (`IP_BOUND_IF` on macOS) because macOS
//!    otherwise sends a limited broadcast out of the primary interface only.
//! 2. *Directed broadcast to a routed subnet* (`10.20.30.255`), for networks
//!    whose routers forward it. Most don't; it costs nothing to try.
//! 3. *Unicast sweep of routed ranges.* Every address in a range the user
//!    gives gets each driver's unicast probe. This is the one that always
//!    crosses routers.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

use crate::model::NetConfig;

/// An IPv4 address on one of this computer's interfaces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalIface {
    pub name: String,
    pub index: Option<u32>,
    pub ip: Ipv4Addr,
    pub prefix: u8,
    pub broadcast: Option<Ipv4Addr>,
    pub loopback: bool,
    /// Point-to-point or tunnel (VPN, Tailscale). Not offered for broadcast
    /// discovery by default: there is no wire behind it.
    pub p2p: bool,
    /// 169.254/16: often how a camera with no DHCP server ends up.
    pub link_local: bool,
}

impl LocalIface {
    pub fn network(&self) -> Ipv4Addr {
        network(self.ip, self.prefix)
    }

    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        in_subnet(ip, self.ip, self.prefix)
    }

    /// Broadcast-capable, up, not loopback or a tunnel.
    pub fn usable(&self) -> bool {
        !self.loopback && !self.p2p
    }

    pub fn cidr(&self) -> String {
        format!("{}/{}", self.ip, self.prefix)
    }
}

/// Every IPv4 address on every up interface, loopback and tunnels included
/// (flagged), sorted by name.
pub fn interfaces() -> io::Result<Vec<LocalIface>> {
    let mut out = Vec::new();
    for i in if_addrs::get_if_addrs()? {
        if let if_addrs::IfAddr::V4(v4) = &i.addr {
            out.push(LocalIface {
                name: i.name.clone(),
                index: i.index,
                ip: v4.ip,
                prefix: v4.prefixlen,
                broadcast: v4.broadcast.or_else(|| Some(broadcast(v4.ip, v4.prefixlen))).filter(|_| v4.prefixlen < 31),
                loopback: v4.ip.is_loopback(),
                p2p: i.is_p2p() || v4.prefixlen >= 31 || i.name.starts_with("utun") || i.name.starts_with("tun"),
                link_local: v4.ip.is_link_local(),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.ip.cmp(&b.ip)));
    Ok(out)
}

pub fn prefix_to_mask(prefix: u8) -> Ipv4Addr {
    let p = prefix.min(32) as u32;
    let bits: u32 = if p == 0 { 0 } else { u32::MAX << (32 - p) };
    Ipv4Addr::from(bits)
}

/// The prefix length of a contiguous mask, or None for a mask like
/// 255.0.255.0 that no camera should be given.
pub fn mask_to_prefix(mask: Ipv4Addr) -> Option<u8> {
    let m = u32::from(mask);
    let ones = m.leading_ones();
    if m.checked_shl(ones).unwrap_or(0) == 0 {
        Some(ones as u8)
    } else {
        None
    }
}

pub fn network(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) & u32::from(prefix_to_mask(prefix)))
}

pub fn broadcast(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(prefix_to_mask(prefix)))
}

pub fn in_subnet(ip: Ipv4Addr, net_ip: Ipv4Addr, prefix: u8) -> bool {
    network(ip, prefix) == network(net_ip, prefix)
}

/// A range to sweep, parsed from what a person types:
/// `10.20.0.0/24`, `10.20.0.10-10.20.0.60`, `10.20.0.10-60`, or one address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub first: Ipv4Addr,
    pub last: Ipv4Addr,
    /// The subnet, when the range was given as one — its directed broadcast is
    /// worth a try too.
    pub subnet: Option<(Ipv4Addr, u8)>,
    pub label: String,
}

impl Range {
    pub fn parse(s: &str) -> Result<Range, String> {
        let s = s.trim();
        let bad = || format!("not an address, range or subnet: {s:?}");
        if let Some((a, p)) = s.split_once('/') {
            let ip: Ipv4Addr = a.trim().parse().map_err(|_| bad())?;
            let prefix: u8 = p.trim().parse().map_err(|_| bad())?;
            if prefix > 32 {
                return Err(bad());
            }
            let net = network(ip, prefix);
            let bc = broadcast(ip, prefix);
            // Skip the network and broadcast addresses of anything bigger than a /31.
            let (first, last) = if prefix >= 31 {
                (net, bc)
            } else {
                (Ipv4Addr::from(u32::from(net) + 1), Ipv4Addr::from(u32::from(bc) - 1))
            };
            return Ok(Range { first, last, subnet: Some((net, prefix)), label: format!("{net}/{prefix}") });
        }
        if let Some((a, b)) = s.split_once('-') {
            let first: Ipv4Addr = a.trim().parse().map_err(|_| bad())?;
            let b = b.trim();
            let last: Ipv4Addr = if b.contains('.') {
                b.parse().map_err(|_| bad())?
            } else {
                let o: u8 = b.parse().map_err(|_| bad())?;
                let f = first.octets();
                Ipv4Addr::new(f[0], f[1], f[2], o)
            };
            if u32::from(last) < u32::from(first) {
                return Err(format!("range runs backwards: {s:?}"));
            }
            return Ok(Range { first, last, subnet: None, label: format!("{first}-{last}") });
        }
        let ip: Ipv4Addr = s.parse().map_err(|_| bad())?;
        Ok(Range { first: ip, last: ip, subnet: None, label: ip.to_string() })
    }

    pub fn len(&self) -> u64 {
        u32::from(self.last) as u64 - u32::from(self.first) as u64 + 1
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> {
        (u32::from(self.first)..=u32::from(self.last)).map(Ipv4Addr::from)
    }
}

/// What a UDP socket should be.
#[derive(Clone, Copy, Debug, Default)]
pub struct UdpOpts {
    pub broadcast: bool,
    /// SO_REUSEADDR + SO_REUSEPORT, so Meerkat can share a well-known port with
    /// a vendor's own tool running at the same time.
    pub reuse: bool,
    /// Pin sends to this interface (macOS/iOS `IP_BOUND_IF`; elsewhere the bind
    /// address does that job).
    pub iface_index: Option<u32>,
    pub multicast_ttl: Option<u32>,
}

/// Bind a tokio UDP socket with the options vendor discovery needs.
pub fn udp_socket(bind: SocketAddrV4, opts: UdpOpts) -> io::Result<tokio::net::UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    if opts.reuse {
        s.set_reuse_address(true)?;
        #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
        s.set_reuse_port(true)?;
    }
    if opts.broadcast {
        s.set_broadcast(true)?;
    }
    if let Some(ttl) = opts.multicast_ttl {
        s.set_multicast_ttl_v4(ttl)?;
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if let Some(idx) = opts.iface_index.and_then(NonZeroU32::new) {
        // Best effort: without it a limited broadcast leaves by the primary
        // interface only, which is a missed camera, not an error.
        let _ = s.bind_device_by_index_v4(Some(idx));
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = opts.iface_index.and_then(NonZeroU32::new);
    s.set_nonblocking(true)?;
    s.bind(&SocketAddr::V4(bind).into())?;
    tokio::net::UdpSocket::from_std(s.into())
}

/// Problems with a NetConfig before it goes anywhere near a camera. Returns
/// every problem, worst first; an empty list means it is fit to send.
pub fn validate(cfg: &NetConfig) -> Vec<String> {
    let mut out = Vec::new();
    if cfg.dhcp {
        return out;
    }
    let ip = cfg.ip;
    if cfg.prefix == 0 || cfg.prefix > 30 {
        out.push(format!("a /{} subnet leaves no room for a camera and a gateway", cfg.prefix));
    }
    if ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() || ip.is_loopback() || ip.octets()[0] == 0 || ip.octets()[0] >= 240 {
        out.push(format!("{ip} is not a usable host address"));
    } else if cfg.prefix <= 30 {
        if ip == network(ip, cfg.prefix) {
            out.push(format!("{ip} is the network address of its /{} subnet", cfg.prefix));
        }
        if ip == broadcast(ip, cfg.prefix) {
            out.push(format!("{ip} is the broadcast address of its /{} subnet", cfg.prefix));
        }
    }
    if let Some(gw) = cfg.gateway {
        if !gw.is_unspecified() {
            if !in_subnet(gw, ip, cfg.prefix) {
                out.push(format!("gateway {gw} is outside {}/{}", network(ip, cfg.prefix), cfg.prefix));
            }
            if gw == ip {
                out.push("the gateway is the camera's own address".into());
            }
        }
    }
    if let Some(h) = &cfg.hostname {
        if h.len() > 63 {
            out.push("the name is longer than 63 characters".into());
        }
    }
    out
}

/// The hardware address of each interface, by name. Some vendor protocols put
/// the sender's MAC in the packet (Panasonic Easy IP); where the platform
/// does not say, callers send zeros, which cameras tolerate.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "android"))]
pub fn iface_macs() -> std::collections::HashMap<String, crate::model::Mac> {
    let mut out = std::collections::HashMap::new();
    // SAFETY: getifaddrs hands back a linked list we only read and then free;
    // every pointer is checked for null and the link-layer address is read
    // within the length the kernel reports.
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return out;
        }
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_addr.is_null() && !ifa.ifa_name.is_null() {
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                if let Some(mac) = link_addr(ifa.ifa_addr) {
                    if mac != [0; 6] {
                        out.insert(name, crate::model::Mac(mac));
                    }
                }
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(head);
    }
    out
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn link_addr(sa: *const libc::sockaddr) -> Option<[u8; 6]> {
    if (*sa).sa_family as i32 != libc::AF_LINK {
        return None;
    }
    let dl = &*(sa as *const libc::sockaddr_dl);
    if dl.sdl_alen != 6 {
        return None;
    }
    let base = dl.sdl_data.as_ptr() as *const u8;
    let p = base.add(dl.sdl_nlen as usize);
    let mut m = [0u8; 6];
    std::ptr::copy_nonoverlapping(p, m.as_mut_ptr(), 6);
    Some(m)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
unsafe fn link_addr(sa: *const libc::sockaddr) -> Option<[u8; 6]> {
    if (*sa).sa_family as i32 != libc::AF_PACKET {
        return None;
    }
    let ll = &*(sa as *const libc::sockaddr_ll);
    if ll.sll_halen != 6 {
        return None;
    }
    let mut m = [0u8; 6];
    m.copy_from_slice(&ll.sll_addr[..6]);
    Some(m)
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux", target_os = "android")))]
pub fn iface_macs() -> std::collections::HashMap<String, crate::model::Mac> {
    std::collections::HashMap::new()
}

/// The local interface whose subnet holds `ip`, if any.
pub fn local_for(ifaces: &[LocalIface], ip: Ipv4Addr) -> Option<&LocalIface> {
    ifaces.iter().filter(|i| !i.loopback || ip.is_loopback()).find(|i| i.contains(ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn masks_and_prefixes_round_trip() {
        for p in 0..=32u8 {
            assert_eq!(mask_to_prefix(prefix_to_mask(p)), Some(p));
        }
        assert_eq!(prefix_to_mask(24), ip("255.255.255.0"));
        assert_eq!(mask_to_prefix(ip("255.255.254.0")), Some(23));
        assert_eq!(mask_to_prefix(ip("255.0.255.0")), None);
    }

    #[test]
    fn ranges_parse_like_people_type_them() {
        let r = Range::parse("10.20.0.0/24").unwrap();
        assert_eq!((r.first, r.last, r.len()), (ip("10.20.0.1"), ip("10.20.0.254"), 254));
        assert_eq!(r.subnet, Some((ip("10.20.0.0"), 24)));
        let r = Range::parse("10.20.0.77/24").unwrap();
        assert_eq!(r.label, "10.20.0.0/24");
        let r = Range::parse(" 192.168.1.10 - 60 ").unwrap();
        assert_eq!((r.first, r.last, r.len()), (ip("192.168.1.10"), ip("192.168.1.60"), 51));
        let r = Range::parse("192.168.1.250-192.168.2.5").unwrap();
        assert_eq!(r.len(), 12);
        assert_eq!(Range::parse("192.168.0.10").unwrap().len(), 1);
        assert_eq!(Range::parse("192.168.0.10/32").unwrap().len(), 1);
        assert!(Range::parse("192.168.0.60-10").is_err());
        assert!(Range::parse("192.168.0/24").is_err());
        assert!(Range::parse("cam1").is_err());
    }

    #[test]
    fn validation_catches_the_classic_mistakes() {
        let ok = NetConfig { dhcp: false, ip: ip("10.0.0.20"), prefix: 24, gateway: Some(ip("10.0.0.1")), dns: vec![], hostname: None };
        assert!(validate(&ok).is_empty());
        let mut c = ok.clone();
        c.gateway = Some(ip("10.0.1.1"));
        assert!(validate(&c)[0].contains("outside 10.0.0.0/24"));
        let mut c = ok.clone();
        c.ip = ip("10.0.0.255");
        assert!(validate(&c)[0].contains("broadcast address"));
        let mut c = ok.clone();
        c.ip = ip("10.0.0.0");
        assert!(validate(&c)[0].contains("network address"));
        let mut c = ok.clone();
        c.prefix = 31;
        assert!(!validate(&c).is_empty());
        let mut c = ok.clone();
        c.gateway = None;
        assert!(validate(&c).is_empty(), "no gateway is allowed: an isolated show network");
        let dhcp = NetConfig { dhcp: true, ip: Ipv4Addr::UNSPECIFIED, prefix: 0, gateway: None, dns: vec![], hostname: None };
        assert!(validate(&dhcp).is_empty());
    }

    #[test]
    fn subnet_membership() {
        assert!(in_subnet(ip("192.168.0.10"), ip("192.168.0.200"), 24));
        assert!(!in_subnet(ip("192.168.1.10"), ip("192.168.0.200"), 24));
        assert_eq!(broadcast(ip("10.40.7.80"), 24), ip("10.40.7.255"));
    }
}
