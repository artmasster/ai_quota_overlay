//! Thin blocking HTTP helpers over ureq.
//!
//! HTTP status codes never become errors here: callers look at [`Resp::status`]. An [`HttpErr::Offline`] is
//! what JS `fetch` reported as "fetch failed" (no connection, DNS, TLS, timeout).
#![allow(dead_code)]

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::de::DeserializeOwned;
use ureq::Agent;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Body limit for normal reads (JSON APIs).
const READ_LIMIT: u64 = 10 * 1024 * 1024;
/// Whole-transfer limit for downloads; connects and the response head still use [`TIMEOUT`].
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const USER_AGENT: &str = concat!("ai-quota-overlay/", env!("CARGO_PKG_VERSION"));

pub struct Resp {
    pub status: u16,
    pub body: Vec<u8>,
    /// When the server's `Retry-After` says to call again (unix ms), if it sent one.
    pub retry_at: Option<i64>,
}

impl Resp {
    /// 2xx.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn json<T: DeserializeOwned>(&self) -> serde_json::Result<T> {
        serde_json::from_slice(&self.body)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpErr {
    /// Could not reach the server: connection refused, DNS, TLS, timeout, connection dropped mid-body.
    Offline(String),
    /// Anything else (bad URL or header, body too large, too many redirects, local file errors, non-2xx download).
    Other(String),
}

impl fmt::Display for HttpErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpErr::Offline(m) => write!(f, "offline: {m}"),
            HttpErr::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for HttpErr {}

fn map_err(e: ureq::Error) -> HttpErr {
    use ureq::Error as E;
    match e {
        E::StatusCode(_)
        | E::Http(_)
        | E::BadUri(_)
        | E::InvalidProxyUrl
        | E::BodyExceedsLimit(_)
        | E::TooManyRedirects
        | E::RedirectFailed
        | E::RequireHttpsOnly(_)
        | E::LargeResponseHeader(..)
        | E::Decompress(..)
        | E::TlsRequired => HttpErr::Other(e.to_string()),
        // Io, Timeout, HostNotFound, ConnectionFailed, Tls/NativeTls/Rustls, Protocol, BodyStalled, proxy connect…
        _ => HttpErr::Offline(e.to_string()),
    }
}

/// Windows and macOS use the system TLS (Schannel / Secure Transport) and its root store; Linux uses rustls.
fn tls() -> ureq::tls::TlsConfig {
    #[cfg(any(windows, target_os = "macos"))]
    {
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::Rustls).build()
    }
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::new_with_config(
            Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(TIMEOUT))
                .user_agent(USER_AGENT)
                .tls_config(tls())
                .build(),
        )
    })
}

/// Agent for [`head_ok`]: no redirects followed (a 3xx is an answer), timeout set per request.
fn head_agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::new_with_config(
            Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .user_agent(USER_AGENT)
                .tls_config(tls())
                .build(),
        )
    })
}

fn read_resp(mut resp: ureq::http::Response<ureq::Body>) -> Result<Resp, HttpErr> {
    let status = resp.status().as_u16();
    let retry_at = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| retry_at(v, crate::model::now_ms()));
    let body = resp.body_mut().with_config().limit(READ_LIMIT).read_to_vec().map_err(map_err)?;
    Ok(Resp { status, body, retry_at })
}

/// `Retry-After` (whole seconds, or an HTTP date) → unix ms.
fn retry_at(v: &str, now: i64) -> Option<i64> {
    let v = v.trim();
    if let Ok(s) = v.parse::<u64>() {
        return Some(now.saturating_add(s.min(i64::MAX as u64 / 1000) as i64 * 1000));
    }
    chrono::DateTime::parse_from_rfc2822(v).ok().map(|d| d.timestamp_millis())
}

pub fn get(url: &str, headers: &[(&str, &str)]) -> Result<Resp, HttpErr> {
    log_request("GET", url);
    let mut req = agent().get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    read_resp(req.call().map_err(map_err)?)
}

pub fn post_json(url: &str, headers: &[(&str, &str)], body: &serde_json::Value) -> Result<Resp, HttpErr> {
    log_request("POST", url);
    let bytes = serde_json::to_vec(body).map_err(|e| HttpErr::Other(e.to_string()))?;
    let mut req = agent().post(url);
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
        req = req.header("Content-Type", "application/json");
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    read_resp(req.send(&bytes[..]).map_err(map_err)?)
}

/// True when any HTTP status comes back within `timeout`; false only when the request itself failed.
pub fn head_ok(url: &str, timeout: Duration) -> bool {
    log_request("HEAD", url);
    head_agent().head(url).config().timeout_global(Some(timeout)).build().call().is_ok()
}

fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Streams `url` into `<dest>.part`, then renames it to `dest`. Follows redirects; non-2xx is an error.
/// `progress(done, total)` is called as bytes arrive; total is None when the server sends no length.
pub fn download(url: &str, dest: &Path, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<(), HttpErr> {
    log_request("DOWNLOAD", url);
    let resp = agent()
        .get(url)
        .config()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .timeout_connect(Some(TIMEOUT))
        .timeout_recv_response(Some(TIMEOUT))
        .build()
        .call()
        .map_err(map_err)?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(HttpErr::Other(format!("HTTP {status}")));
    }
    let total = resp.body().content_length();
    let mut reader = resp.into_body().into_with_config().limit(u64::MAX).reader();

    let part = part_path(dest);
    let local = |e: std::io::Error| HttpErr::Other(format!("{}: {e}", part.display()));
    let result = (|| {
        let mut file = File::create(&part).map_err(local)?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        progress(0, total);
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(HttpErr::Offline(e.to_string())),
            };
            file.write_all(&buf[..n]).map_err(local)?;
            done += n as u64;
            progress(done, total);
        }
        file.sync_all().map_err(local)?;
        drop(file);
        std::fs::rename(&part, dest).map_err(|e| HttpErr::Other(format!("{}: {e}", dest.display())))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resp_helpers() {
        let r = Resp { status: 204, body: br#"{"a":1}"#.to_vec(), retry_at: None };
        assert!(r.ok());
        assert_eq!(r.text(), r#"{"a":1}"#);
        let v: serde_json::Value = r.json().unwrap();
        assert_eq!(v["a"], 1);
        assert!(!Resp { status: 301, body: vec![], retry_at: None }.ok());
        assert!(!Resp { status: 199, body: vec![], retry_at: None }.ok());
    }

    #[test]
    fn retry_after_forms() {
        assert_eq!(retry_at("120", 1_000), Some(121_000));
        assert_eq!(retry_at(" 0 ", 5), Some(5));
        assert_eq!(retry_at("Wed, 21 Oct 2015 07:28:00 GMT", 0), Some(1_445_412_480_000));
        assert_eq!(retry_at("soon", 0), None);
        assert_eq!(retry_at("-5", 0), None);
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(part_path(Path::new("a/b/setup.exe")), PathBuf::from("a/b/setup.exe.part"));
    }

    #[test]
    fn unreachable_is_offline() {
        // Port 9 on localhost: nothing listens, so the connect fails locally without touching the network.
        match get("http://127.0.0.1:9/", &[]) {
            Err(HttpErr::Offline(_)) => {}
            Err(other) => panic!("expected Offline, got {other:?}"),
            Ok(r) => panic!("unexpected response {}", r.status),
        }
        assert!(!head_ok("http://127.0.0.1:9/", Duration::from_secs(2)));
    }

    #[test]
    fn bad_url_is_other() {
        assert!(matches!(get("not a url", &[]), Err(HttpErr::Other(_))));
    }
}

/// Dev aid: QUOTA_OVERLAY_LOG_REQUESTS=<file> appends "<unix ms> <method> <url without query>" per request
/// (never headers or bodies), to check the polling rhythm.
pub(crate) fn log_request(method: &str, url: &str) {
    static PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    let Some(path) = PATH.get_or_init(|| std::env::var_os("QUOTA_OVERLAY_LOG_REQUESTS").map(Into::into)) else { return };
    let bare = url.split('?').next().unwrap_or(url);
    let line = format!("{} {method} {bare}\n", crate::model::now_ms());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}