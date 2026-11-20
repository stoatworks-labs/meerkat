//! Helpers the drivers share: just-enough XML for SOAP replies, vendor
//! guessing from names, and UTC timestamps without a date crate.

/// Inner text of every element whose local name is `local`, whatever its
/// namespace prefix (`<tds:Model>`, `<tt:Model>` and `<Model>` all match).
pub fn xml_texts(xml: &str, local: &str) -> Vec<String> {
    xml_blocks(xml, local).into_iter().map(|b| unescape(strip_tags(b).trim())).collect()
}

pub fn xml_text(xml: &str, local: &str) -> Option<String> {
    xml_texts(xml, local).into_iter().next()
}

/// The raw inner XML of every element with local name `local`.
pub fn xml_blocks<'a>(xml: &'a str, local: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rel) = xml[at..].find('<') {
        let start = at + rel;
        let rest = &xml[start + 1..];
        if rest.starts_with('/') || rest.starts_with('?') || rest.starts_with('!') {
            at = start + 1;
            continue;
        }
        let name_end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
        let qname = &rest[..name_end];
        let lname = qname.rsplit(':').next().unwrap_or(qname);
        let Some(gt) = rest.find('>') else { break };
        if lname != local {
            at = start + 1;
            continue;
        }
        if rest[..gt].ends_with('/') {
            out.push("");
            at = start + 1 + gt;
            continue;
        }
        let body_start = start + 1 + gt + 1;
        let close = format!("</{qname}>");
        // Nested elements of the same name are rare in these replies; the
        // first matching close tag is the right one for every shape we parse.
        match xml[body_start..].find(&close) {
            Some(end) => {
                out.push(&xml[body_start..body_start + end]);
                at = body_start + end + close.len();
            }
            None => break,
        }
    }
    out
}

/// An attribute of the first element named `local`.
pub fn xml_attr(xml: &str, local: &str, attr: &str) -> Option<String> {
    let mut at = 0;
    while let Some(rel) = xml[at..].find('<') {
        let start = at + rel;
        let rest = &xml[start + 1..];
        let name_end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
        let qname = &rest[..name_end];
        if qname.rsplit(':').next() == Some(local) {
            let gt = rest.find('>')?;
            let tag = &rest[name_end..gt];
            let key = format!("{attr}=");
            let i = tag.find(&key)?;
            let v = &tag[i + key.len()..];
            let q = v.chars().next()?;
            let v = &v[1..];
            return v.find(q).map(|e| unescape(&v[..e]));
        }
        at = start + 1;
    }
    None
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

pub fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The brand a model string, hostname or NDI name points at. Used by the
/// generic drivers (ONVIF, NDI) to label what they find; the vendor drivers
/// then confirm it with their own probe.
pub fn guess_vendor(s: &str) -> Option<&'static str> {
    let u = s.to_ascii_uppercase();
    let has = |p: &str| u.contains(p);
    let starts = |p: &str| u.starts_with(p) || u.contains(&format!(" {p}")) || u.contains(&format!("({p}"));
    Some(if has("BIRDDOG") || has("BIRD DOG") {
        "BirdDog"
    } else if has("PANASONIC") || starts("AW-") || starts("AK-U") {
        "Panasonic"
    } else if has("SONY") || starts("SRG-") || starts("BRC-") || has("ILME-FR7") || starts("FR7") {
        "Sony"
    } else if has("PTZOPTICS") || has("PTZ OPTICS") {
        "PTZOptics"
    } else if has("CANON") || starts("CR-N") || starts("CR-X") {
        "Canon"
    } else if has("AVER") && !has("AVERAGE") {
        "AVer"
    } else if has("LUMENS") || starts("VC-A") || starts("VC-R") || starts("VC-TR") || starts("VC-BC") {
        "Lumens"
    } else if has("MARSHALL") || starts("CV6") || starts("CV7") {
        "Marshall"
    } else if has("JVC") || starts("KY-PZ") {
        "JVC"
    } else if has("DATAVIDEO") || starts("PTC-") {
        "Datavideo"
    } else if has("OBSBOT") {
        "OBSBOT"
    } else if has("HUDDLECAM") {
        "HuddleCam"
    } else if has("MINRRAY") {
        "Minrray"
    } else if has("AVONIC") {
        "Avonic"
    } else if has("BOLIN") {
        "Bolin"
    } else if has("AXIS") {
        "Axis"
    } else {
        return None;
    })
}

/// `2026-11-20T10:04:05Z` for a Unix time.
pub fn iso8601(unix: i64) -> String {
    let days = unix.div_euclid(86400);
    let secs = unix.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Unix time for a UTC civil date and time.
pub fn unix_from_civil(y: i64, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
    days_from_civil(y, m, d) * 86400 + (hh * 3600 + mm * 60 + ss) as i64
}

// Howard Hinnant's algorithms (http://howardhinnant.github.io/date_algorithms.html).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    if getrandom::fill(&mut b).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t >> ((i % 16) * 8)) as u8 ^ (i as u8).wrapping_mul(37);
        }
    }
    b
}

pub fn uuid4() -> String {
    let mut b: [u8; 16] = random_bytes();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_ignores_prefixes() {
        let x = r#"<s:Envelope><s:Body><tds:GetDeviceInformationResponse><tds:Manufacturer>Panasonic</tds:Manufacturer><tds:Model>AW-UE150</tds:Model><Model>x</Model><tt:Empty/></tds:GetDeviceInformationResponse></s:Body></s:Envelope>"#;
        assert_eq!(xml_text(x, "Manufacturer").as_deref(), Some("Panasonic"));
        assert_eq!(xml_texts(x, "Model"), vec!["AW-UE150", "x"]);
        assert_eq!(xml_texts(x, "Empty"), vec![""]);
        assert_eq!(xml_text(x, "Missing"), None);
        let a = r#"<tt:NetworkInterfaces token="eth0"><tt:Enabled>true</tt:Enabled></tt:NetworkInterfaces>"#;
        assert_eq!(xml_attr(a, "NetworkInterfaces", "token").as_deref(), Some("eth0"));
    }

    #[test]
    fn vendors_from_names() {
        assert_eq!(guess_vendor("BIRDDOG-608F9 (CAM)"), Some("BirdDog"));
        assert_eq!(guess_vendor("AW-UE150"), Some("Panasonic"));
        assert_eq!(guess_vendor("SRG-X400"), Some("Sony"));
        assert_eq!(guess_vendor("PTZOPTICS-1234 (Studio)"), Some("PTZOptics"));
        assert_eq!(guess_vendor("STUDIO-PC (OBS)"), None);
    }

    #[test]
    fn utc_round_trips() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_795_000_000), "2026-11-18T11:06:40Z");
        assert_eq!(unix_from_civil(2026, 11, 18, 11, 6, 40), 1_795_000_000);
        assert_eq!(unix_from_civil(2024, 2, 29, 0, 0, 0), 1_709_164_800);
    }
}
