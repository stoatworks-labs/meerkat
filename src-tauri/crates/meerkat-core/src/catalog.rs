//! What Meerkat knows about cameras that is not a protocol: the login each
//! model leaves the factory with, and where its maker publishes firmware.
//!
//! The rows are brand knowledge, so they live in `meerkat-drivers`
//! (`logins.rs`, `firmware.rs`), not here. This module is their shape and the
//! one rule that decides which rows are about a camera ([`pick`]): its vendor
//! string names the brand, then its model is matched against each row's
//! patterns. When the model is unknown, or is not in the table, the brand's
//! rows are still offered, marked as a hint rather than a match.
//!
//! Nothing here touches a camera. A factory login is filled in when the user
//! asks for it and is never tried on a camera by Meerkat itself: some cameras
//! block an address after a few wrong passwords.

use serde::{Deserialize, Serialize};

use crate::model::Camera;

/// The tables the engine is given besides its drivers. Serialised whole for
/// the UI's reference list; cameras carry only the ids of their rows.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Catalog {
    pub logins: &'static [LoginEntry],
    pub firmware: &'static [FirmwareEntry],
}

impl Catalog {
    /// Which rows of each table are about this camera.
    pub fn about(&self, cam: &Camera) -> CatalogMatch {
        let model = cam.model.as_deref();
        CatalogMatch { logins: pick(self.logins, &cam.vendor, model), firmware: pick(self.firmware, &cam.vendor, model) }
    }
}

/// The rows a camera matched, by id, in table order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Matched {
    pub ids: Vec<String>,
    /// True when the rows are for this camera's model, or for every model of
    /// its brand. False when they are the brand's rows for other models,
    /// offered because Meerkat does not know this one.
    pub exact: bool,
}

/// What the catalogue says about one camera.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMatch {
    pub logins: Matched,
    pub firmware: Matched,
}

/// Which cameras a row is about.
pub trait Applies {
    fn id(&self) -> &'static str;
    /// Lower-case pieces of a camera's vendor string that mean this brand.
    fn vendors(&self) -> &'static [&'static str];
    /// Model patterns, matched against the start of the camera's model with
    /// case, spaces and punctuation ignored. `*` stands for anything; a
    /// pattern ending in a digit does not run on into more digits (`AW-UE4`
    /// is not `AW-UE40`); a trailing `$` means the model ends there (`X1$`
    /// is not `X1 Ultra`). Empty means every model of the brand.
    fn models(&self) -> &'static [&'static str];
}

/// Where a fact in the tables comes from.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// The document and the place in it, e.g. "AW-UE160 Operating Instructions, p. 52".
    pub title: &'static str,
    pub url: &'static str,
    pub kind: SourceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// The maker's own document or page, read.
    Vendor,
    /// The maker's own document, but only an excerpt of it could be checked.
    VendorExcerpt,
    /// Not the maker: a manual mirror, a reseller's FAQ.
    ThirdParty,
}

/// The login a family of cameras leaves the factory with.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginEntry {
    pub id: &'static str,
    pub brand: &'static str,
    #[serde(skip)]
    pub vendors: &'static [&'static str],
    #[serde(skip)]
    pub models: &'static [&'static str],
    /// The models, the way the maker names them.
    pub family: &'static str,
    /// The firmware this row is for, when the firmware decides.
    pub firmware: Option<&'static str>,
    pub login: Login,
    /// The camera makes you replace the factory login the first time it is used.
    pub change_at_first_login: bool,
    /// What the maker says happens after wrong passwords, when it says.
    pub lockout: Option<&'static str>,
    pub notes: &'static [&'static str],
    pub sources: &'static [Source],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Login {
    /// Set at the factory. An empty username means the camera asks for a
    /// password only; an empty password means it is blank.
    Factory { username: &'static str, password: &'static str },
    /// None: the first visit to the camera's web page (or its maker's tool)
    /// creates the administrator account. `username` when the maker fixes it.
    Create { username: Option<&'static str> },
}

/// Where a family of cameras gets its firmware, and how the maker says to
/// apply it. Meerkat links to the maker; it never hosts or fetches firmware.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareEntry {
    pub id: &'static str,
    pub brand: &'static str,
    #[serde(skip)]
    pub vendors: &'static [&'static str],
    #[serde(skip)]
    pub models: &'static [&'static str],
    pub family: &'static str,
    /// The maker's download page for these models.
    pub downloads: &'static str,
    /// What that page asks for before a download, if anything.
    pub downloads_note: Option<&'static str>,
    /// How the maker says to apply it, in the camera's own web page or tool.
    pub how: &'static [&'static str],
    /// The path of the camera's own firmware page, where the maker's
    /// procedure shows it, to open on the camera's web address.
    pub web_path: Option<&'static str>,
    /// The maker's warnings: power, order, duration, no way back.
    pub warnings: &'static [&'static str],
    /// A published network upload for these cameras, when there is one.
    /// Shown for information: Meerkat does not upload firmware.
    pub upload: Option<&'static str>,
    pub sources: &'static [Source],
}

impl Applies for LoginEntry {
    fn id(&self) -> &'static str {
        self.id
    }
    fn vendors(&self) -> &'static [&'static str] {
        self.vendors
    }
    fn models(&self) -> &'static [&'static str] {
        self.models
    }
}

impl Applies for FirmwareEntry {
    fn id(&self) -> &'static str {
        self.id
    }
    fn vendors(&self) -> &'static [&'static str] {
        self.vendors
    }
    fn models(&self) -> &'static [&'static str] {
        self.models
    }
}

/// Upper-case letters and digits, plus a pattern's `*` and `$`: `AW-UE150`
/// and `aw ue150` are one model.
pub fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '*' || *c == '$').map(|c| c.to_ascii_uppercase()).collect()
}

/// A squashed `pattern` against the start of a squashed `model`. `*` matches
/// anything. What follows the pattern is allowed (`AW-UE150` matches
/// `AW-UE150W`), except more digits after a pattern ending in one, and
/// anything at all after a pattern ending in `$`.
fn glob_prefix(pattern: &str, model: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pattern, false),
    };
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = model.strip_prefix(first) else { return false };
    for p in parts.filter(|p| !p.is_empty()) {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    if anchored {
        return rest.is_empty() || pattern.ends_with('*');
    }
    let ends_in_digit = pattern.chars().last().is_some_and(|c| c.is_ascii_digit());
    !(ends_in_digit && rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// A model may carry its brand in front ("BirdDog P400"); patterns do not.
fn model_matches(pattern: &str, model: &str, vendors: &[&str]) -> bool {
    let p = squash(pattern);
    if p.replace(['*', '$'], "").is_empty() {
        return false;
    }
    glob_prefix(&p, model) || vendors.iter().map(|v| squash(v)).filter(|v| !v.is_empty()).any(|v| model.strip_prefix(v.as_str()).is_some_and(|m| glob_prefix(&p, m)))
}

/// The rows about a camera with this vendor string and model.
///
/// 1. Rows of its brand whose patterns match its model: exact.
/// 2. Otherwise, rows of its brand with no patterns, which cover every
///    model: exact.
/// 3. Otherwise, every row of its brand: a hint, not a match.
pub fn pick<T: Applies>(rows: &[T], vendor: &str, model: Option<&str>) -> Matched {
    let v = vendor.to_ascii_lowercase();
    let brand: Vec<&T> = rows.iter().filter(|r| r.vendors().iter().any(|k| !k.is_empty() && v.contains(k))).collect();
    let ids = |rs: &[&T]| rs.iter().map(|r| r.id().to_string()).collect::<Vec<_>>();
    if brand.is_empty() {
        return Matched::default();
    }
    if let Some(m) = model.map(squash).filter(|m| !m.is_empty()) {
        let hit: Vec<&T> = brand.iter().copied().filter(|r| r.models().iter().any(|p| model_matches(p, &m, r.vendors()))).collect();
        if !hit.is_empty() {
            return Matched { ids: ids(&hit), exact: true };
        }
    }
    let whole: Vec<&T> = brand.iter().copied().filter(|r| r.models().is_empty()).collect();
    if !whole.is_empty() {
        return Matched { ids: ids(&whole), exact: true };
    }
    Matched { ids: ids(&brand), exact: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn row(id: &'static str, vendors: &'static [&'static str], models: &'static [&'static str]) -> LoginEntry {
        LoginEntry {
            id,
            brand: "",
            vendors,
            models,
            family: "",
            firmware: None,
            login: Login::Create { username: None },
            change_at_first_login: false,
            lockout: None,
            notes: &[],
            sources: &[],
        }
    }

    static ROWS: &[LoginEntry] = &[
        row("pana-old", &["panasonic"], &["AW-HE40", "AW-HE130"]),
        row("pana-new", &["panasonic"], &["AW-UE4", "AW-UE150"]),
        row("ptz-g2", &["ptzoptics"], &["PT*-G2"]),
        row("bird-p", &["birddog"], &["P400"]),
        row("avonic", &["avonic"], &[]),
    ];

    fn ids(m: &Matched) -> Vec<&str> {
        m.ids.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_known_model_matches_its_row_whatever_its_spelling() {
        for model in ["AW-UE150", "aw ue150", "AW-UE150W", "AW-UE4KG"] {
            let m = pick(ROWS, "Panasonic", Some(model));
            assert_eq!((ids(&m), m.exact), (vec!["pana-new"], true), "{model}");
        }
        assert_eq!(ids(&pick(ROWS, "Panasonic", Some("AW-HE130K"))), ["pana-old"]);
    }

    #[test]
    fn a_pattern_ending_in_a_digit_does_not_run_on_into_more() {
        // AW-UE4 is not AW-UE40, but it is AW-UE4KG.
        assert!(!pick(ROWS, "Panasonic", Some("AW-UE40")).exact);
        assert!(glob_prefix("AWUE4", "AWUE4KG") && !glob_prefix("AWUE4", "AWUE40"));
        assert!(glob_prefix("TR31*", "TR311N") && !glob_prefix("TR31", "TR311"));
    }

    #[test]
    fn a_trailing_dollar_anchors_the_end() {
        assert!(glob_prefix("X1$", "X1") && !glob_prefix("X1$", "X1ULTRA"));
        assert!(glob_prefix("X1", "X1ULTRA"));
        assert_eq!(squash("X1 $"), "X1$");
    }

    #[test]
    fn star_stands_for_anything_and_the_brand_may_lead_the_model() {
        assert_eq!(ids(&pick(ROWS, "PTZOptics / VHD", Some("PT20X-SDI-GY-G2"))), ["ptz-g2"]);
        assert!(!pick(ROWS, "PTZOptics / VHD", Some("PT20X-SDI-G3")).exact);
        assert_eq!(ids(&pick(ROWS, "BirdDog", Some("BirdDog P400"))), ["bird-p"]);
    }

    #[test]
    fn an_unknown_model_gets_its_brands_rows_as_a_hint() {
        let m = pick(ROWS, "Panasonic", Some("AW-HE42"));
        assert_eq!((ids(&m), m.exact), (vec!["pana-old", "pana-new"], false));
        let m = pick(ROWS, "Panasonic", None);
        assert_eq!((ids(&m), m.exact), (vec!["pana-old", "pana-new"], false));
    }

    #[test]
    fn a_brand_wide_row_covers_every_model() {
        let m = pick(ROWS, "Avonic", Some("CM93-IP"));
        assert_eq!((ids(&m), m.exact), (vec!["avonic"], true));
    }

    #[test]
    fn another_brand_gets_nothing() {
        assert_eq!(pick(ROWS, "ONVIF", Some("AW-UE150")), Matched::default());
        assert_eq!(pick(ROWS, "", None), Matched::default());
    }

    #[test]
    fn rows_serialise_without_their_patterns() {
        let v = serde_json::to_value(ROWS[0]).unwrap();
        assert!(v.get("vendors").is_none() && v.get("models").is_none(), "{v}");
        assert_eq!(v["login"]["kind"], "create");
        assert_eq!(v["changeAtFirstLogin"], false);
    }
}
