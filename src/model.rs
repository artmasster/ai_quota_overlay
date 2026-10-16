//! Quota data shared by the fetchers and the UI.
use serde::{Deserialize, Serialize};

pub const SERVICES: [Svc; 3] = [Svc::Claude, Svc::Codex, Svc::Cursor];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Svc {
    Claude,
    Codex,
    Cursor,
}

impl Svc {
    pub fn key(self) -> &'static str {
        match self {
            Svc::Claude => "claude",
            Svc::Codex => "codex",
            Svc::Cursor => "cursor",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Svc::Claude => "Claude",
            Svc::Codex => "Codex",
            Svc::Cursor => "Cursor",
        }
    }
    pub fn from_key(k: &str) -> Option<Svc> {
        SERVICES.into_iter().find(|s| s.key() == k)
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

/// One quota window. `span` in seconds; `name` (a model) or `tag` (worded by i18n); `billing` = a billing cycle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Win {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
}

impl QErr {
    pub fn code(code: &str) -> Self {
        Self { code: code.into(), ..Self::default() }
    }
    pub fn status(service: &str, status: impl ToString) -> Self {
        Self { code: "status".into(), service: Some(service.into()), status: Some(status.to_string()), ..Self::default() }
    }
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
    pub error: Option<QErr>,
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub svc: [Option<Quota>; 3],
    /// Unix ms.
    pub fetched_at: i64,
}

impl Snapshot {
    pub fn get(&self, s: Svc) -> Option<&Quota> {
        self.svc[s.index()].as_ref()
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
