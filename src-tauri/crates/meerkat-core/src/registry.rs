//! One row per camera, however many drivers found it.
//!
//! A Panasonic AW-UE150 can answer Easy IP, ONVIF and NDI in one scan. Each
//! report is merged into the row with the same MAC — or, for a report that
//! carries no MAC (ONVIF over unicast, mDNS), the row at the same address. The
//! highest-priority driver that has seen a camera owns it: its name, model and
//! capabilities win, and its write path is the one the UI uses. Lower-priority
//! drivers only fill gaps.

use std::collections::{BTreeMap, HashMap};

use crate::model::Camera;

#[derive(Default)]
pub struct Registry {
    cams: BTreeMap<String, Camera>,
    /// Owner priority per row, kept beside the rows so Camera stays plain data.
    prio: HashMap<String, u8>,
}

pub struct Upsert {
    pub camera: Camera,
    /// Rows folded into this one (an `ip:` row that turned out to have a MAC),
    /// which the UI should drop.
    pub replaces: Vec<String>,
    pub is_new: bool,
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&Camera> {
        self.cams.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Camera> {
        self.cams.get_mut(id)
    }

    pub fn all(&self) -> Vec<Camera> {
        self.cams.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.cams.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cams.is_empty()
    }

    pub fn clear(&mut self) {
        self.cams.clear();
        self.prio.clear();
    }

    pub fn remove(&mut self, id: &str) -> Option<Camera> {
        self.prio.remove(id);
        self.cams.remove(id)
    }

    /// Merge one driver report (priority `prio`) into the table.
    pub fn upsert(&mut self, mut inc: Camera, prio: u8) -> Upsert {
        inc.id = inc.key();
        let mut replaces = Vec::new();

        // Which existing row is this camera?
        let target = if inc.mac.is_some() {
            // A MAC report absorbs a MAC-less row at the same address.
            let ip_key = format!("ip:{}", inc.ip);
            if self.cams.contains_key(&ip_key) && !self.cams.contains_key(&inc.id) {
                let old_prio = *self.prio.get(&ip_key).unwrap_or(&0);
                let old = self.remove(&ip_key).expect("row");
                self.cams.insert(inc.id.clone(), Camera { id: inc.id.clone(), ..old });
                self.prio.insert(inc.id.clone(), old_prio);
                replaces.push(ip_key);
            } else if self.cams.contains_key(&ip_key) {
                self.remove(&ip_key);
                replaces.push(ip_key);
            }
            Some(inc.id.clone()).filter(|k| self.cams.contains_key(k))
        } else {
            // No MAC: attach to whatever row holds this address now.
            self.cams.values().find(|c| c.ip == inc.ip).map(|c| c.id.clone())
        };

        let Some(id) = target else {
            let id = inc.id.clone();
            self.prio.insert(id.clone(), prio);
            self.cams.insert(id.clone(), inc.clone());
            return Upsert { camera: inc, replaces, is_new: true };
        };

        let cur = self.cams.get_mut(&id).expect("row");
        let cur_prio = *self.prio.get(&id).unwrap_or(&0);
        // Only a strictly higher priority takes a row from another driver: two
        // vendor drivers that both claim one address (a VISCA reply that says
        // "Sony" from a camera Easy IP already named) must not flip-flop.
        let wins = prio > cur_prio || (prio == cur_prio && cur.driver == inc.driver);
        merge(cur, inc, wins);
        if wins {
            self.prio.insert(id.clone(), prio);
        }
        Upsert { camera: cur.clone(), replaces, is_new: false }
    }
}

/// Fold `inc` into `cur`. When `wins`, the incoming driver's identity and
/// capabilities replace the current ones; otherwise it only fills gaps.
fn merge(cur: &mut Camera, inc: Camera, wins: bool) {
    fn pick<T>(cur: &mut Option<T>, inc: Option<T>, wins: bool) {
        if inc.is_some() && (wins || cur.is_none()) {
            *cur = inc;
        }
    }
    if inc.driver == cur.driver {
        // The same driver by two routes (Easy IP broadcast and an HTTP probe):
        // what it can do is everything either route said.
        let (a, b) = (&mut cur.caps, &inc.caps);
        a.read_network |= b.read_network;
        a.write_network |= b.write_network;
        a.write_by_mac |= b.write_by_mac;
        a.needs_auth |= b.needs_auth;
        a.dhcp |= b.dhcp;
        a.dns |= b.dns;
        a.hostname |= b.hostname;
        a.reboot |= b.reboot;
        cur.ip = inc.ip;
    } else if wins {
        cur.driver = inc.driver.clone();
        if !inc.vendor.is_empty() {
            cur.vendor = inc.vendor.clone();
        }
        cur.ip = inc.ip;
        cur.caps = inc.caps.clone();
    }
    if cur.mac.is_none() {
        cur.mac = inc.mac;
    }
    pick(&mut cur.model, inc.model, wins);
    pick(&mut cur.name, inc.name, wins);
    pick(&mut cur.net, inc.net, wins);
    pick(&mut cur.firmware, inc.firmware, wins);
    pick(&mut cur.serial, inc.serial, wins);
    pick(&mut cur.web_port, inc.web_port, wins);
    pick(&mut cur.web_url, inc.web_url, wins);
    for (k, v) in inc.fields {
        if wins || !cur.fields.contains_key(&k) {
            cur.fields.insert(k, v);
        }
    }
    for s in inc.seen {
        if !cur.seen.contains(&s) {
            cur.seen.push(s);
        }
    }
    cur.last_seen = cur.last_seen.max(inc.last_seen);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mac, Method, Sighting};
    use std::net::Ipv4Addr;

    fn cam(driver: &str, ip: [u8; 4], mac: Option<[u8; 6]>, name: Option<&str>) -> Camera {
        let mut c = Camera::new(driver, driver, Ipv4Addr::from(ip));
        c.mac = mac.map(Mac);
        c.name = name.map(str::to_string);
        c.seen = vec![Sighting { driver: driver.into(), method: Method::Broadcast, via: "en0".into() }];
        c
    }

    #[test]
    fn vendor_driver_owns_a_camera_onvif_also_saw() {
        let mut r = Registry::default();
        let a = r.upsert(cam("onvif", [10, 0, 0, 5], None, Some("onvif name")), 30);
        assert!(a.is_new);
        assert_eq!(a.camera.id, "ip:10.0.0.5");
        let b = r.upsert(cam("panasonic", [10, 0, 0, 5], Some([0, 0x80, 0x45, 1, 2, 3]), Some("CAM 1")), 60);
        assert_eq!(b.replaces, vec!["ip:10.0.0.5".to_string()]);
        assert_eq!(b.camera.id, "mac:00:80:45:01:02:03");
        assert_eq!(b.camera.driver, "panasonic");
        assert_eq!(b.camera.name.as_deref(), Some("CAM 1"));
        assert_eq!(b.camera.seen.len(), 2);
        assert_eq!(r.len(), 1);
        // A later ONVIF report for the same address fills gaps but does not take over.
        let mut late = cam("onvif", [10, 0, 0, 5], None, Some("onvif name"));
        late.firmware = Some("9.9".into());
        let c = r.upsert(late, 30);
        assert_eq!(c.camera.driver, "panasonic");
        assert_eq!(c.camera.name.as_deref(), Some("CAM 1"));
        assert_eq!(c.camera.firmware.as_deref(), Some("9.9"));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn a_readdressed_camera_keeps_its_row() {
        let mut r = Registry::default();
        let mac = Some([2, 0, 0, 0, 0, 9]);
        r.upsert(cam("sony", [192, 168, 0, 100], mac, None), 60);
        let moved = r.upsert(cam("sony", [10, 0, 0, 50], mac, None), 60);
        assert!(!moved.is_new);
        assert_eq!(moved.camera.ip, Ipv4Addr::new(10, 0, 0, 50));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn an_equal_priority_driver_cannot_take_a_row() {
        let mut r = Registry::default();
        r.upsert(cam("panasonic", [10, 0, 0, 5], Some([0, 0x80, 0x45, 1, 2, 3]), Some("CAM 1")), 70);
        let s = r.upsert(cam("sony", [10, 0, 0, 5], None, Some("visca")), 70);
        assert_eq!(s.camera.driver, "panasonic");
        assert_eq!(s.camera.name.as_deref(), Some("CAM 1"));
    }

    #[test]
    fn two_cameras_two_rows() {
        let mut r = Registry::default();
        r.upsert(cam("sony", [10, 0, 0, 1], Some([2, 0, 0, 0, 0, 1]), None), 60);
        r.upsert(cam("sony", [10, 0, 0, 2], Some([2, 0, 0, 0, 0, 2]), None), 60);
        r.upsert(cam("ndi", [10, 0, 0, 3], None, None), 10);
        assert_eq!(r.len(), 3);
    }
}
