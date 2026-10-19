//! Fetches remaining quota for Claude (claude.ai subscription), Codex (ChatGPT plan) and Cursor.
//! Credentials are read from where the official apps already keep them; nothing is stored elsewhere.
//! Port of the Electron app's `src/quota.js`.
#![allow(dead_code)]

mod claude;
mod codex;
mod cursor;

use std::path::{Path, PathBuf};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::Value;

use crate::http::HttpErr;
use crate::model::{now_ms, QErr, Quota, Snapshot, Svc, SERVICES};

/// Why a fetcher did not produce data; mapped onto a [`Quota`] by [`settle`] like JS `settle()`.
#[derive(Debug, PartialEq)]
pub(crate) enum FetchErr {
    /// No login for this service on this machine at all; the overlay hides it.
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
// Where the official apps keep their logins.

pub(crate) fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

/// `$var` when set (even empty, like JS `??`), else `default`.
fn env_dir(var: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(default)
}

pub(crate) fn claude_creds_path() -> PathBuf {
    env_dir("CLAUDE_CONFIG_DIR", || home().join(".claude")).join(".credentials.json")
}

pub(crate) fn codex_auth_path() -> PathBuf {
    env_dir("CODEX_HOME", || home().join(".codex")).join("auth.json")
}

pub(crate) fn cursor_db_path() -> PathBuf {
    let base = if cfg!(windows) {
        env_dir("APPDATA", || home().join("AppData").join("Roaming"))
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        env_dir("XDG_CONFIG_HOME", || home().join(".config"))
    };
    base.join("Cursor").join("User").join("globalStorage").join("state.vscdb")
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

pub(crate) fn read_json(file: &Path) -> Res<Value> {
    Ok(serde_json::from_slice(&std::fs::read(file)?)?)
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

fn fetch_one(s: Svc) -> Quota {
    if let Some(q) = fake_429(std::env::var("QUOTA_OVERLAY_FAKE_429").ok().as_deref(), s, now_ms()) {
        crate::http::log_request("FAKE429", s.key());
        return q;
    }
    settle(match s {
        Svc::Claude => claude::fetch(&claude_creds_path()),
        Svc::Codex => codex::fetch(&codex_auth_path()),
        Svc::Cursor => cursor::fetch(&cursor_db_path()),
    })
}

/// Fetches all services in parallel. `enabled[svc.index()] == false` (switched off in Settings) and services
/// left out of `QUOTA_OVERLAY_SERVICES` come back as [`Quota::disabled`].
pub fn fetch_all(enabled: [bool; 3]) -> Snapshot {
    let only = parse_only(std::env::var("QUOTA_OVERLAY_SERVICES").ok().as_deref());
    let skip = |s: Svc| !enabled[s.index()] || only.as_ref().is_some_and(|o| !o.iter().any(|k| k == s.key()));
    let svc = std::thread::scope(|scope| {
        let handles = SERVICES.map(|s| (!skip(s)).then(|| scope.spawn(move || fetch_one(s))));
        handles.map(|h| {
            Some(match h {
                None => Quota::disabled(),
                Some(h) => h.join().unwrap_or_else(|_| settle(Err(FetchErr::Unexpected("fetcher panicked".into())))),
            })
        })
    });
    Snapshot { svc, fetched_at: now_ms() }
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
        let s = fetch_all([false; 3]);
        for q in &s.svc {
            assert_eq!(q.as_ref(), Some(&Quota::disabled()));
        }
        assert!(s.fetched_at > 0);
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

    #[test]
    fn paths_end_where_the_apps_keep_them() {
        assert!(claude_creds_path().ends_with(".credentials.json"));
        assert!(codex_auth_path().ends_with("auth.json"));
        assert!(cursor_db_path().ends_with(Path::new("Cursor/User/globalStorage/state.vscdb")));
    }
}
