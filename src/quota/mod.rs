//! Fetches remaining quota for Claude (claude.ai subscription), Codex (ChatGPT plan) and Cursor, per account: the
//! accounts signed in from this app (`login`, kept in accounts.json by `store`), any number per service. Logins
//! that other apps keep on this machine are not used.
#![allow(dead_code)]

mod claude;
mod codex;
mod copilot;
mod cursor;
pub mod login;
pub mod store;

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::Value;

use crate::http::HttpErr;
use crate::model::{now_ms, Acct, QErr, Quota, Snapshot, Svc};

/// Why a fetcher did not produce data; mapped onto a [`Quota`] by [`settle`] like JS `settle()`.
#[derive(Debug, PartialEq)]
pub(crate) enum FetchErr {
    /// The account is gone (signed out meanwhile); the overlay hides it.
    NotLoggedIn,
    /// The request could not be made (JS: "fetch failed").
    Offline,
    /// A known error (`QuotaError`): code worded through i18n plus its params.
    Quota(QErr),
    /// Anything else.
    Unexpected(String),
}

pub(crate) type Res<T> = Result<T, FetchErr>;

impl From<HttpErr> for FetchErr {
    fn from(e: HttpErr) -> Self {
        match e {
            HttpErr::Offline(_) => FetchErr::Offline,
            HttpErr::Other(m) => FetchErr::Unexpected(m),
        }
    }
}

impl From<serde_json::Error> for FetchErr {
    fn from(e: serde_json::Error) -> Self {
        FetchErr::Unexpected(e.to_string())
    }
}

impl From<std::io::Error> for FetchErr {
    fn from(e: std::io::Error) -> Self {
        FetchErr::Unexpected(e.to_string())
    }
}

pub(crate) fn qerr(code: &str) -> FetchErr {
    FetchErr::Quota(QErr::code(code))
}

/// An account signed in here whose login no longer works: it has to be signed in again from the app window.
pub(crate) fn app_expired(service: &str, status: Option<String>) -> FetchErr {
    FetchErr::Quota(QErr { code: "appExpired".into(), service: Some(service.into()), status, ..QErr::default() })
}

/// What `FetchErr` says, for a sign-in that failed.
pub(crate) fn describe(e: &FetchErr, lang: crate::i18n::Lang) -> String {
    match e {
        FetchErr::NotLoggedIn => "no login".into(),
        FetchErr::Offline => crate::i18n::error_text(&QErr::code("offline"), lang),
        FetchErr::Quota(q) => crate::i18n::error_text(q, lang),
        FetchErr::Unexpected(m) => m.clone(),
    }
}

pub(crate) fn settle(r: Res<Quota>) -> Quota {
    match r {
        Ok(q) => Quota { ok: true, ..q },
        Err(FetchErr::NotLoggedIn) => Quota::absent(),
        Err(FetchErr::Offline) => Quota::err(QErr::code("offline")),
        Err(FetchErr::Quota(e)) => Quota::err(e),
        Err(FetchErr::Unexpected(m)) => Quota::err(QErr { code: "unexpected".into(), message: Some(m), ..QErr::default() }),
    }
}

// ---------------------------------------------------------------------------------------------------------
// JS value semantics used by the mappings.

/// JS truthiness.
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// JS `Number(v)`; None where JS gives NaN.
pub(crate) fn js_number(v: Option<&Value>) -> Option<f64> {
    let n = match v? {
        Value::Null => 0.0,
        Value::Bool(b) => *b as u8 as f64,
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                0.0
            } else {
                s.parse::<f64>().ok()?
            }
        }
        Value::Array(_) | Value::Object(_) => return None,
    };
    (!n.is_nan()).then_some(n)
}

/// JS `Date.parse` for the date strings these APIs send (ISO-8601 / RFC 3339, RFC 2822); unix ms.
pub(crate) fn parse_date_ms(v: Option<&Value>) -> Option<i64> {
    use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
    let s = v?.as_str()?.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp_millis());
    }
    // A date-time without an offset is local time; a date alone is UTC midnight (as in JS).
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Local.from_local_datetime(&n).earliest().map(|d| d.timestamp_millis());
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    DateTime::parse_from_rfc2822(s).ok().map(|d| d.timestamp_millis())
}

/// A JSON object's entries in document order (JS `Object.entries`); serde_json's own map is sorted.
struct Ordered(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Ordered, A::Error> {
                let mut out: Vec<(String, Value)> = Vec::new();
                while let Some((k, v)) = a.next_entry::<String, Value>()? {
                    // A repeated key keeps its first position and its last value, like JSON.parse.
                    match out.iter_mut().find(|(kk, _)| *kk == k) {
                        Some(e) => e.1 = v,
                        None => out.push((k, v)),
                    }
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(V)
    }
}

/// Top-level entries of a JSON response body, in order; a non-object body has none.
pub(crate) fn entries_of(body: &[u8]) -> serde_json::Result<Vec<(String, Value)>> {
    let v: Value = serde_json::from_slice(body)?;
    if !v.is_object() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_slice::<Ordered>(body)?.0)
}

pub(crate) fn entry<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub(crate) fn tmp_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".quota-overlay.tmp");
    PathBuf::from(s)
}

/// Writes `<file>.quota-overlay.tmp` (mode 0600 on unix) and renames it over `file`.
pub(crate) fn write_json_atomic(file: &Path, data: &Value) -> std::io::Result<()> {
    let tmp = tmp_path(file);
    let result = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(&serde_json::to_vec(data)?)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// `n` bytes from the system's random source.
pub(crate) fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).expect("system random source");
    b
}

pub(crate) fn b64url(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

/// The JSON payload of a JWT (base64url, padding optional, like Node's `base64url` decoding).
pub(crate) fn jwt_payload(token: &str) -> Option<Value> {
    const ENGINE: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::URL_SAFE,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let part = token.split('.').nth(1)?;
    let cleaned: String = part
        .chars()
        .filter_map(|c| match c {
            '+' => Some('-'),
            '/' => Some('_'),
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect();
    let bytes = ENGINE.decode(cleaned).ok()?;
    serde_json::from_str(&String::from_utf8_lossy(&bytes)).ok()
}

/// A JWT's `exp` in unix ms (0 when it has none).
pub(crate) fn jwt_exp_ms(token: &str) -> i64 {
    jwt_payload(token).and_then(|p| js_number(p.get("exp"))).map_or(0, |e| (e * 1000.0) as i64)
}

/// JS `encodeURIComponent`.
pub(crate) fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `a=1&b=2` with each value percent-encoded.
pub(crate) fn query(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}", encode_uri_component(v))).collect::<Vec<_>>().join("&")
}

// ---------------------------------------------------------------------------------------------------------

/// `QUOTA_OVERLAY_SERVICES=claude,codex` → the services to check (None = all).
fn parse_only(var: Option<&str>) -> Option<Vec<String>> {
    var.map(|s| s.to_lowercase().split(',').map(|k| k.trim().to_string()).collect())
}

/// Dev aid: `QUOTA_OVERLAY_FAKE_429=claude:90,codex` → those services answer "too many requests" (with a
/// Retry-After of 90 s for Claude) without sending anything.
fn fake_429(var: Option<&str>, s: Svc, now: i64) -> Option<Quota> {
    let item = var?.split(',').map(str::trim).find(|i| i.split(':').next().is_some_and(|k| k.eq_ignore_ascii_case(s.key())))?;
    let secs = item.split(':').nth(1).and_then(|v| v.trim().parse::<i64>().ok());
    Some(settle(Err(FetchErr::Quota(QErr::rate_limited(secs.map(|v| now + v * 1000))))))
}

fn fetch_one(a: &Acct) -> Quota {
    if let Some(q) = fake_429(std::env::var("QUOTA_OVERLAY_FAKE_429").ok().as_deref(), a.svc, now_ms()) {
        crate::http::log_request("FAKE429", &a.id);
        return q;
    }
    settle(match a.svc {
        Svc::Claude => claude::fetch(&a.id),
        Svc::Codex => codex::fetch(&a.id),
        Svc::Cursor => cursor::fetch(&a.id),
        Svc::Copilot => copilot::fetch(&a.id),
    })
}

/// Every account to check: the ones signed in here, in the order the user put them in (new ones last).
pub fn accounts() -> Vec<Acct> {
    store::list().iter().filter_map(store::Saved::acct).collect()
}

/// Checks `accts` in parallel. Ids in `skip` (switched off in Settings, or waiting out "too many requests") and
/// services left out of `QUOTA_OVERLAY_SERVICES` come back as [`Quota::disabled`].
fn fetch_these(accts: Vec<Acct>, skip: &HashSet<String>) -> Snapshot {
    let only = parse_only(std::env::var("QUOTA_OVERLAY_SERVICES").ok().as_deref());
    let skipped = |a: &Acct| skip.contains(&a.id) || only.as_ref().is_some_and(|o| !o.iter().any(|k| k == a.svc.key()));
    let accts = std::thread::scope(|scope| {
        let handles: Vec<_> = accts.iter().map(|a| (!skipped(a)).then(|| scope.spawn(move || fetch_one(a)))).collect();
        accts
            .iter()
            .cloned()
            .zip(handles)
            .map(|(a, h)| {
                let q = match h {
                    None => Quota::disabled(),
                    Some(h) => h.join().unwrap_or_else(|_| settle(Err(FetchErr::Unexpected("fetcher panicked".into())))),
                };
                (a, q)
            })
            .collect()
    });
    Snapshot { accts, fetched_at: now_ms() }
}

/// Checks every account (see [`accounts`]), but not those in `skip`.
pub fn fetch_all(skip: &HashSet<String>) -> Snapshot {
    fetch_these(accounts(), skip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn services_env() {
        assert_eq!(parse_only(None), None);
        assert_eq!(parse_only(Some("Claude, CODEX")), Some(vec!["claude".to_string(), "codex".to_string()]));
        assert_eq!(parse_only(Some("cursor")), Some(vec!["cursor".to_string()]));
        // Set but empty checks nothing, as in JS.
        assert_eq!(parse_only(Some("")), Some(vec![String::new()]));
    }

    #[test]
    fn fake_429_env() {
        assert_eq!(fake_429(None, Svc::Claude, 0), None);
        let q = fake_429(Some("codex, Claude:90"), Svc::Claude, 1_000).unwrap();
        assert!(q.rate_limited() && q.error.unwrap().retry_at == Some(91_000));
        assert_eq!(fake_429(Some("codex"), Svc::Codex, 0).unwrap().error.unwrap().retry_at, None);
        assert_eq!(fake_429(Some("codex"), Svc::Cursor, 0), None);
    }

    #[test]
    fn all_switched_off() {
        let acct = |id: &str, svc| Acct { id: id.into(), svc, who: "a@b".into() };
        let accts = vec![acct("claude-1a2b3c", Svc::Claude), acct("claude-abc123", Svc::Claude), acct("cursor-0f0f0f", Svc::Cursor)];
        let skip: HashSet<String> = accts.iter().map(|a| a.id.clone()).collect();
        let s = fetch_these(accts.clone(), &skip);
        assert_eq!(s.accts.len(), 3);
        for ((a, q), want) in s.accts.iter().zip(&accts) {
            assert_eq!(a, want);
            assert_eq!(q, &Quota::disabled());
        }
        assert!(s.fetched_at > 0);
    }

    #[test]
    fn helpers() {
        assert_eq!(query(&[("a", "x y"), ("redirect_uri", "http://localhost:1/cb")]), "a=x%20y&redirect_uri=http%3A%2F%2Flocalhost%3A1%2Fcb");
        assert_eq!(random(16).len(), 16);
        assert_ne!(random(16), random(16));
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
        let jwt = format!("x.{}.y", b64url(br#"{"exp":1700000000,"email":"a@b"}"#));
        assert_eq!(jwt_exp_ms(&jwt), 1_700_000_000_000);
        assert_eq!(jwt_exp_ms("nope"), 0);
    }

    #[test]
    fn settle_maps_outcomes() {
        assert_eq!(settle(Err(FetchErr::NotLoggedIn)), Quota::absent());
        assert_eq!(settle(Err(FetchErr::Offline)).error.unwrap().code, "offline");
        assert_eq!(settle(Err(HttpErr::Offline("x".into()).into())).error.unwrap().code, "offline");
        let e = settle(Err(FetchErr::Quota(QErr::status("Codex", 500)))).error.unwrap();
        assert_eq!((e.code.as_str(), e.service.as_deref(), e.status.as_deref()), ("status", Some("Codex"), Some("500")));
        let e = settle(Err(FetchErr::Unexpected("boom".into()))).error.unwrap();
        assert_eq!((e.code.as_str(), e.message.as_deref()), ("unexpected", Some("boom")));
        let q = settle(Ok(Quota { plan: "max".into(), ..Quota::default() }));
        assert!(q.ok && !q.absent && q.plan == "max");
    }

    #[test]
    fn js_semantics() {
        assert!(!truthy(None) && !truthy(Some(&json!(null))) && !truthy(Some(&json!(0))) && !truthy(Some(&json!(""))));
        assert!(truthy(Some(&json!(1))) && truthy(Some(&json!("x"))) && truthy(Some(&json!({}))));
        assert_eq!(js_number(Some(&json!("12.5"))), Some(12.5));
        assert_eq!(js_number(Some(&json!(""))), Some(0.0));
        assert_eq!(js_number(Some(&json!("abc"))), None);
        assert_eq!(js_number(None), None);
        assert_eq!(parse_date_ms(Some(&json!("2025-01-01T00:00:00Z"))), Some(1_735_689_600_000));
        assert_eq!(parse_date_ms(Some(&json!("2025-01-01T05:00:00.123456+05:00"))), Some(1_735_689_600_123));
        assert_eq!(parse_date_ms(Some(&json!("2025-01-01"))), Some(1_735_689_600_000));
        assert_eq!(parse_date_ms(Some(&json!("not a date"))), None);
        assert_eq!(parse_date_ms(Some(&json!(5))), None);
    }

    #[test]
    fn entries_keep_order() {
        let e = entries_of(br#"{"b":1,"a":2,"c":{"z":1},"b":3}"#).unwrap();
        let keys: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["b", "a", "c"]);
        assert_eq!(entry(&e, "b"), Some(&json!(3)));
        assert!(entries_of(b"null").unwrap().is_empty());
        assert!(entries_of(b"{").is_err());
    }
}
