//! A small HTTP client for camera CGIs and REST APIs: plain HTTP (cameras
//! rarely ship a certificate a client could trust anyway), short timeouts, and
//! Basic or Digest auth answered from the server's challenge — Panasonic and
//! Sony CGIs want Digest, BirdDog's API wants nothing, some OEMs want Basic.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};

use crate::model::Credentials;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("{0}")]
    Transport(String),
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("the camera refused the username or password")]
    Unauthorized,
}

#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: String,
}

/// Where traffic for an address really goes. Real scans have none; the
/// simulator's sandbox sends every camera address to its loopback listeners
/// (and everything else nowhere), so a demo never touches the network.
pub type Route = Arc<dyn Fn(SocketAddrV4) -> SocketAddrV4 + Send + Sync>;

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    route: Option<Route>,
}

impl Http {
    pub fn new(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(3))
            .user_agent(concat!("Meerkat/", env!("CARGO_PKG_VERSION")))
            .no_proxy()
            .build()
            .expect("reqwest client");
        Http { client, route: None }
    }

    pub fn with_route(mut self, route: Option<Route>) -> Self {
        self.route = route;
        self
    }

    /// The socket to send to for `addr`, after the route (if any).
    pub fn dest(&self, addr: SocketAddrV4) -> SocketAddrV4 {
        match &self.route {
            Some(r) => r(addr),
            None => addr,
        }
    }

    /// `url` with its host and port routed, for a request or for handing a
    /// browser the simulator's web UI.
    pub fn url(&self, url: &str) -> String {
        let Some(r) = &self.route else { return url.to_string() };
        let Ok(mut u) = reqwest::Url::parse(url) else { return url.to_string() };
        let Some(ip) = u.host_str().and_then(|h| h.parse::<Ipv4Addr>().ok()) else { return url.to_string() };
        let to = r(SocketAddrV4::new(ip, u.port_or_known_default().unwrap_or(80)));
        let _ = u.set_ip_host((*to.ip()).into());
        let _ = u.set_port(Some(to.port()));
        u.to_string()
    }

    pub async fn get(&self, url: &str, creds: Option<&Credentials>) -> Result<Resp, HttpError> {
        self.send("GET", url, None, creds).await
    }

    pub async fn post(&self, url: &str, content_type: &str, body: Vec<u8>, creds: Option<&Credentials>) -> Result<Resp, HttpError> {
        self.send("POST", url, Some((content_type.to_string(), body)), creds).await
    }

    /// Send once; on a 401 with credentials in hand, answer the challenge and
    /// send again. Any status ≥ 400 after that is an error carrying the body.
    pub async fn send(&self, method: &str, url: &str, body: Option<(String, Vec<u8>)>, creds: Option<&Credentials>) -> Result<Resp, HttpError> {
        let first = self.once(method, url, body.clone(), None).await?;
        let resp = if first.status == 401 {
            let Some(c) = creds else { return Err(HttpError::Unauthorized) };
            let challenge = first.headers.get_all(WWW_AUTHENTICATE).iter().filter_map(|v| v.to_str().ok()).map(str::to_string).collect::<Vec<_>>();
            let auth = authorization(method, url, &challenge, c).ok_or(HttpError::Unauthorized)?;
            let second = self.once(method, url, body, Some(auth)).await?;
            if second.status == 401 {
                return Err(HttpError::Unauthorized);
            }
            second
        } else {
            first
        };
        if resp.status >= 400 {
            let mut b = resp.body.clone();
            b.truncate(300);
            return Err(HttpError::Status { status: resp.status, body: b });
        }
        Ok(resp)
    }

    async fn once(&self, method: &str, url: &str, body: Option<(String, Vec<u8>)>, auth: Option<String>) -> Result<Resp, HttpError> {
        let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| HttpError::Transport(e.to_string()))?;
        let routed = self.url(url);
        let mut rb = self.client.request(m, &routed);
        if let Some(a) = auth {
            rb = rb.header(AUTHORIZATION, a);
        }
        if let Some((ct, b)) = body {
            rb = rb.header(CONTENT_TYPE, ct).body(b);
        }
        let r = rb.send().await.map_err(|e| HttpError::Transport(describe(&e)))?;
        let status = r.status().as_u16();
        let headers = r.headers().clone();
        let bytes = r.bytes().await.map_err(|e| HttpError::Transport(describe(&e)))?;
        Ok(Resp { status, headers, body: String::from_utf8_lossy(&bytes).into_owned() })
    }
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".into()
    } else if e.is_connect() {
        "could not connect".into()
    } else {
        e.to_string()
    }
}

/// The Authorization header for the best challenge offered: Digest (MD5 or
/// MD5-sess) over Basic. None when the server offers neither.
pub fn authorization(method: &str, url: &str, challenges: &[String], c: &Credentials) -> Option<String> {
    let uri = request_uri(url);
    for ch in challenges {
        let (scheme, rest) = ch.trim().split_once(' ').unwrap_or((ch.trim(), ""));
        if scheme.eq_ignore_ascii_case("digest") {
            let p = parse_params(rest);
            let algo = p.get("algorithm").map(String::as_str).unwrap_or("MD5");
            if !algo.eq_ignore_ascii_case("md5") && !algo.eq_ignore_ascii_case("md5-sess") {
                continue;
            }
            return Some(digest_header(method, &uri, &p, c, &cnonce()));
        }
    }
    challenges.iter().find(|ch| ch.trim().to_ascii_lowercase().starts_with("basic")).map(|_| basic_header(c))
}

pub fn basic_header(c: &Credentials) -> String {
    format!("Basic {}", base64(format!("{}:{}", c.username, c.password).as_bytes()))
}

/// RFC 7616 Digest with MD5, `qop=auth` when offered (else the RFC 2069 form).
pub fn digest_header(method: &str, uri: &str, p: &std::collections::HashMap<String, String>, c: &Credentials, cnonce: &str) -> String {
    let realm = p.get("realm").cloned().unwrap_or_default();
    let nonce = p.get("nonce").cloned().unwrap_or_default();
    let algo = p.get("algorithm").cloned().unwrap_or_else(|| "MD5".into());
    let qop = p.get("qop").map(|q| q.split(',').map(str::trim).any(|q| q == "auth"));
    let nc = "00000001";
    let mut ha1 = md5_hex(&format!("{}:{}:{}", c.username, realm, c.password));
    if algo.eq_ignore_ascii_case("md5-sess") {
        ha1 = md5_hex(&format!("{ha1}:{nonce}:{cnonce}"));
    }
    let ha2 = md5_hex(&format!("{method}:{uri}"));
    let mut h = format!(r#"Digest username="{}", realm="{}", nonce="{}", uri="{}", algorithm={}"#, c.username, realm, nonce, uri, algo);
    if qop == Some(true) {
        let response = md5_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"));
        h += &format!(r#", response="{response}", qop=auth, nc={nc}, cnonce="{cnonce}""#);
    } else {
        let response = md5_hex(&format!("{ha1}:{nonce}:{ha2}"));
        h += &format!(r#", response="{response}""#);
    }
    if let Some(o) = p.get("opaque") {
        h += &format!(r#", opaque="{o}""#);
    }
    h
}

/// `key=value, key="quoted, value"` pairs from a challenge, keys lower-cased.
pub fn parse_params(s: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut chars = s.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == ',') {
            chars.next();
        }
        let key: String = chars.by_ref().take_while(|c| *c != '=').collect::<String>().trim().to_ascii_lowercase();
        if key.is_empty() {
            break;
        }
        let mut val = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            val.push(n)
                        }
                    }
                    '"' => break,
                    c => val.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                val.push(c);
                chars.next();
            }
            val = val.trim().to_string();
        }
        out.insert(key, val);
    }
    out
}

fn request_uri(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    match after_scheme.find('/') {
        Some(i) => after_scheme[i..].to_string(),
        None => "/".into(),
    }
}

fn md5_hex(s: &str) -> String {
    hex::encode(Md5::digest(s.as_bytes()))
}

fn cnonce() -> String {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b = (t as u64).to_le_bytes();
    }
    hex::encode(b)
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        let c = Credentials { username: "admin".into(), password: "12345".into() };
        assert_eq!(basic_header(&c), "Basic YWRtaW46MTIzNDU=");
    }

    #[test]
    fn digest_matches_rfc2617_example() {
        // RFC 2617 §3.5: Mufasa / "Circle Of Life", GET /dir/index.html.
        let p = parse_params(r#"realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#);
        assert_eq!(p["qop"], "auth,auth-int");
        let c = Credentials { username: "Mufasa".into(), password: "Circle Of Life".into() };
        let h = digest_header("GET", "/dir/index.html", &p, &c, "0a4f113b");
        assert!(h.contains(r#"response="6629fae49393a05397450978507c4ef1""#), "{h}");
        assert!(h.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#));
        assert!(h.contains("nc=00000001"));
    }

    #[test]
    fn request_uri_keeps_the_query() {
        assert_eq!(request_uri("http://10.0.0.5/cgi-bin/aw_cam?cmd=QID&res=1"), "/cgi-bin/aw_cam?cmd=QID&res=1");
        assert_eq!(request_uri("http://10.0.0.5:8080"), "/");
    }
}
