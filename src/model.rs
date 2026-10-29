//! Quota data shared by the fetchers and the UI.
use serde::{Deserialize, Serialize};

pub const SERVICES: [Svc; 4] = [Svc::Claude, Svc::Codex, Svc::Cursor, Svc::Copilot];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Svc {
    #[default]
    Claude,
    Codex,
    Cursor,
    /// GitHub Copilot.
    Copilot,
}

impl Svc {
    pub fn key(self) -> &'static str {
        match self {
            Svc::Claude => "claude",
            Svc::Codex => "codex",
            Svc::Cursor => "cursor",
            Svc::Copilot => "copilot",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Svc::Claude => "Claude",
            Svc::Codex => "Codex",
            Svc::Cursor => "Cursor",
            Svc::Copilot => "Copilot",
        }
    }

    pub fn from_key(k: &str) -> Option<Svc> {
        SERVICES.into_iter().find(|s| s.key() == k)
    }
}

/// One account signed in here. `id` is the service key plus a random suffix ("claude-3f9a2c"); settings and the
/// cache are keyed by it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Acct {
    pub id: String,
    pub svc: Svc,
    /// Email (or name) of the account, when known.
    pub who: String,
}

impl Acct {
    pub fn aid(&self) -> Aid {
        aid(&self.id)
    }
}

/// A copyable stand-in for an account id (hit targets and hover keys need `Copy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Aid(pub u64);

/// FNV-1a of the id: stable across runs and reorderings.
pub fn aid(id: &str) -> Aid {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Aid(h)
}

/// The short form of `who` for tight spots: an email's name part.
pub fn short_who(who: &str) -> &str {
    who.split('@').next().unwrap_or(who)
}

/// One quota window. `span` in seconds; `name` (a model) or `tag` (worded by i18n); `billing` = a billing cycle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Win {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "whole_as_int")]
    pub span: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub billing: bool,
    #[serde(skip)]
    pub used_percent: Option<f64>,
    /// Unix ms.
    #[serde(skip)]
    pub resets_at: Option<i64>,
}

impl Win {
    pub fn used(&self) -> f64 {
        self.used_percent.unwrap_or(0.0)
    }
    /// What `seen` keeps of a window.
    pub fn seen(&self) -> Win {
        Win { id: self.id.clone(), span: self.span, name: self.name.clone(), tag: self.tag.clone(), billing: self.billing, ..Win::default() }
    }
}

/// Error code worded through i18n (`errors.*`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QErr {
    pub code: String,
    pub status: Option<String>,
    pub service: Option<String>,
    pub message: Option<String>,
    /// Unix ms the server asked us not to call before (`Retry-After`).
    pub retry_at: Option<i64>,
}

impl QErr {
    pub fn code(code: &str) -> Self {
        Self { code: code.into(), ..Self::default() }
    }
    pub fn status(service: &str, status: impl ToString) -> Self {
        Self { code: "status".into(), service: Some(service.into()), status: Some(status.to_string()), ..Self::default() }
    }
    /// "Too many requests", with the server's `Retry-After` when it sent one.
    pub fn rate_limited(retry_at: Option<i64>) -> Self {
        Self { code: "rateLimited".into(), retry_at, ..Self::default() }
    }
}

/// When the app checks a service again by itself (unix ms); `manual` = a double-click may try sooner
/// (false while the server's own `Retry-After` still holds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retry {
    pub at: i64,
    pub manual: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extra {
    pub used: f64,
    pub limit: f64,
}

/// Result for one service: ok with data, absent (no login on this machine, or switched off), or an error.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quota {
    pub ok: bool,
    pub absent: bool,
    pub disabled: bool,
    pub plan: String,
    pub windows: Vec<Win>,
    pub extra: Option<Extra>,
    pub credits: Option<f64>,
    pub limit_reached: bool,
    /// With `ok`: the latest check failed with this, and the numbers are the last good ones (see `as_of`).
    pub error: Option<QErr>,
    /// Unix ms the numbers were fetched, when they come from an earlier check (or the cache on disk).
    pub as_of: Option<i64>,
    /// Set while the service is left alone after "too many requests".
    pub retry: Option<Retry>,
}

impl Quota {
    pub fn absent() -> Self {
        Self { absent: true, ..Self::default() }
    }
    pub fn disabled() -> Self {
        Self { absent: true, disabled: true, ..Self::default() }
    }
    pub fn err(e: QErr) -> Self {
        Self { error: Some(e), ..Self::default() }
    }
    pub fn rate_limited(&self) -> bool {
        self.error.as_ref().is_some_and(|e| e.code == "rateLimited" || e.status.as_deref() == Some("429"))
    }
    /// Old numbers kept on screen because the latest check failed.
    pub fn stale(&self) -> bool {
        self.ok && self.as_of.is_some()
    }
}

/// Every account's result, by service (Claude, Codex, Cursor), each service's own login first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub accts: Vec<(Acct, Quota)>,
    /// Unix ms.
    pub fetched_at: i64,
}

impl Snapshot {
    pub fn get(&self, id: &str) -> Option<&Quota> {
        self.accts.iter().find(|(a, _)| a.id == id).map(|(_, q)| q)
    }
    pub fn by_aid(&self, x: Aid) -> Option<&(Acct, Quota)> {
        self.accts.iter().find(|(a, _)| a.aid() == x)
    }
    /// How many of the service's accounts are checked and logged in (more than one: they are told apart by `who`).
    pub fn count(&self, s: Svc) -> usize {
        self.accts.iter().filter(|(a, q)| a.svc == s && !q.absent).count()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetState {
    pub online: bool,
    pub ms: Option<u64>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Writes 18000 rather than 18000.0, as JSON.stringify did.
fn whole_as_int<S: serde::Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(x) if x.fract() == 0.0 && x.abs() < 9e15 => s.serialize_i64(*x as i64),
        Some(x) => s.serialize_f64(*x),
        None => s.serialize_none(),
    }
}