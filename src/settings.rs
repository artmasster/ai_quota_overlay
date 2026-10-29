//! settings.json in the same folder and shape the Electron app used (`app.getPath('userData')`), so existing
//! users keep their settings: card, opacity, vertical, lang, network, services, hidden, seen, skipVersion.
//! 2.x adds `size` (small / medium / large overlay). Since accounts are signed in here, `services`, `hidden` and
//! `seen` are keyed by account id ("claude-3f9a2c"); the bare service keys older versions wrote (for the logins
//! other apps keep, no longer used) are dropped on load.
use crate::model::{Svc, Win};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const APP_NAME: &str = "Ai Quota Overlay";

/// Where the card was left (DIP, top-left) and its size then; None = bottom-right of the primary display.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardPos {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub card: Option<CardPos>,
    pub opacity: f64,
    pub vertical: bool,
    /// Overlay size: "small", "medium" or "large" (see `zoom`).
    pub size: String,
    pub lang: String,
    pub network: bool,
    /// Accounts switched off (false) or on; not listed = on.
    pub services: BTreeMap<String, bool>,
    /// Window ids left off the card, per account.
    pub hidden: BTreeMap<String, Vec<String>>,
    /// The windows each account had last time (shown in the app window while it is off).
    pub seen: BTreeMap<String, Vec<Win>>,
    pub skip_version: Option<String>,
    /// Keys this version doesn't know, written back unchanged.
    pub other: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            card: None,
            opacity: 0.92,
            vertical: false,
            size: "medium".into(),
            lang: "en".into(),
            network: false,
            services: BTreeMap::new(),
            hidden: Default::default(),
            seen: Default::default(),
            skip_version: None,
            other: Map::new(),
        }
    }
}

/// Per-user data folder: %APPDATA%\Ai Quota Overlay, ~/Library/Application Support/Ai Quota Overlay,
/// $XDG_CONFIG_HOME (or ~/.config)/Ai Quota Overlay.
pub fn data_dir() -> PathBuf {
    // Dev aid: keep test settings away from the real ones without moving %APPDATA% (Cursor's login lives there).
    if let Some(d) = std::env::var_os("QUOTA_OVERLAY_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Roaming"))
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".config"))
    };
    base.join(APP_NAME)
}

pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn file() -> PathBuf {
    data_dir().join("settings.json")
}

/// Overlay sizes and their zoom (CSS px × zoom = DIP). Medium is the size every version so far had.
pub const SIZES: [(&str, f64); 3] = [("small", 1.0), ("medium", 1.2), ("large", 1.45)];

impl Settings {
    /// The overlay's zoom for the chosen size.
    pub fn zoom(&self) -> f64 {
        SIZES.iter().find(|(k, _)| *k == self.size).map_or(1.2, |(_, z)| *z)
    }

    /// Whether account `id` is switched on.
    pub fn on(&self, id: &str) -> bool {
        self.services.get(id).copied().unwrap_or(true)
    }

    /// The window ids of account `id` left off the card.
    pub fn hidden_of(&self, id: &str) -> &[String] {
        self.hidden.get(id).map_or(&[], Vec::as_slice)
    }

    /// Drops what was kept for an account that was signed out.
    pub fn forget(&mut self, id: &str) {
        self.services.remove(id);
        self.hidden.remove(id);
        self.seen.remove(id);
    }

    pub fn load() -> Self {
        Self::parse(&std::fs::read_to_string(file()).unwrap_or_default())
    }

    /// Settings from settings.json text; anything missing or malformed falls back to the defaults.
    pub fn parse(text: &str) -> Self {
        let raw = serde_json::from_str::<Value>(text).ok();
        let mut s = Settings::default();
        let Some(Value::Object(mut m)) = raw else { return s };
        if let Some(v) = m.remove("card") {
            s.card = serde_json::from_value(v).ok();
        }
        if let Some(v) = m.remove("opacity").and_then(|v| v.as_f64()) {
            s.opacity = v;
        }
        if let Some(v) = m.remove("vertical").and_then(|v| v.as_bool()) {
            s.vertical = v;
        }
        if let Some(v) = m.remove("size").and_then(|v| v.as_str().map(String::from)) {
            if SIZES.iter().any(|(k, _)| *k == v) {
                s.size = v;
            }
        }
        if let Some(v) = m.remove("lang").and_then(|v| v.as_str().map(String::from)) {
            s.lang = v;
        }
        if crate::i18n::Lang::from_key(&s.lang).is_none() {
            s.lang = "en".into();
        }
        if let Some(v) = m.remove("network").and_then(|v| v.as_bool()) {
            s.network = v;
        }
        // Per account; a bare service key is an older version's entry for another app's login.
        let account = |id: &str| Svc::from_key(id).is_none();
        if let Some(Value::Object(sv)) = m.remove("services") {
            for (id, v) in sv {
                if let (true, Some(b)) = (account(&id), v.as_bool()) {
                    s.services.insert(id, b);
                }
            }
        }
        if let Some(Value::Object(h)) = m.remove("hidden") {
            for (id, v) in h {
                if let (true, Value::Array(a)) = (account(&id), v) {
                    s.hidden.insert(id, a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                }
            }
        }
        if let Some(Value::Object(seen)) = m.remove("seen") {
            for (id, v) in seen {
                if let (true, Ok(w)) = (account(&id), serde_json::from_value(v)) {
                    s.seen.insert(id, w);
                }
            }
        }
        m.remove("removed");
        if let Some(v) = m.remove("skipVersion") {
            s.skip_version = v.as_str().map(String::from);
        }
        s.other = m;
        s
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("card".into(), self.card.map(|c| serde_json::to_value(c).unwrap_or(Value::Null)).unwrap_or(Value::Null));
        m.insert("opacity".into(), Value::from(self.opacity));
        m.insert("vertical".into(), Value::from(self.vertical));
        m.insert("lang".into(), Value::from(self.lang.clone()));
        m.insert("network".into(), Value::from(self.network));
        let services: Map<String, Value> = self.services.iter().map(|(id, on)| (id.clone(), Value::from(*on))).collect();
        let hidden: Map<String, Value> = self.hidden.iter().map(|(id, h)| (id.clone(), Value::from(h.clone()))).collect();
        let seen: Map<String, Value> = self.seen.iter().map(|(id, w)| (id.clone(), serde_json::to_value(w).unwrap_or(Value::Null))).collect();
        m.insert("services".into(), Value::Object(services));
        m.insert("hidden".into(), Value::Object(hidden));
        m.insert("seen".into(), Value::Object(seen));
        m.insert("skipVersion".into(), self.skip_version.clone().map(Value::from).unwrap_or(Value::Null));
        // Keys added after Electron come after its own.
        m.insert("size".into(), Value::from(self.size.clone()));
        // Unknown keys go last so the known ones keep the old order.
        let ordered: BTreeMap<_, _> = self.other.clone().into_iter().collect();
        for (k, v) in ordered {
            m.entry(k).or_insert(v);
        }
        Value::Object(m)
    }

    pub fn save(&self) {
        let path = file();
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&path, serde_json::to_string(&self.to_json()).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_defaults_to_medium() {
        assert_eq!(Settings::parse("{}").size, "medium");
        assert_eq!(Settings::parse("{}").zoom(), 1.2);
        assert_eq!(Settings::parse(r#"{"size":"huge"}"#).size, "medium");
        let s = Settings::parse(r#"{"size":"large"}"#);
        assert_eq!(s.zoom(), 1.45);
        assert_eq!(s.to_json()["size"], "large");
    }

    #[test]
    fn reads_and_writes_the_electron_shape() {
        let raw = r#"{"card":{"x":2271.6,"y":1265,"w":296,"h":97},"opacity":0.85,"vertical":true,"lang":"th","network":true,
            "services":{"claude":true,"codex":false,"cursor":true},"hidden":{"claude":["seven_day_opus"],"codex":[],"cursor":[]},
            "seen":{"claude":[{"id":"five_hour","span":18000},{"id":"seven_day_opus","span":604800,"name":"Opus"}],
            "cursor":[{"id":"billing","billing":true}]},"skipVersion":"1.3.0","futureKey":42}"#;
        let s = Settings::parse(raw);
        assert_eq!(s.card.unwrap().x, 2271.6);
        assert_eq!(s.lang, "th");
        assert_eq!(s.skip_version.as_deref(), Some("1.3.0"));
        // The other apps' logins' entries are gone.
        assert!(s.services.is_empty() && s.hidden.is_empty() && s.seen.is_empty());
        let out = s.to_json();
        assert_eq!(out["futureKey"], 42);
        assert_eq!(out["services"], serde_json::json!({}));
        // JSON.stringify order: the known keys first, as Electron wrote them.
        let text = serde_json::to_string(&out).unwrap();
        assert!(text.starts_with(r#"{"card":{"x":2271.6,"y":1265.0,"w":296.0,"h":97.0},"opacity":0.85,"vertical":true,"lang":"th""#), "{text}");
    }

    #[test]
    fn old_and_broken_files_fall_back_to_defaults() {
        let s = Settings::parse(r#"{"card":{"x":2434,"y":761,"w":65,"h":356},"opacity":0.92,"vertical":true}"#);
        assert_eq!(s.card.unwrap().h, Some(356.0));
        assert!(s.vertical);
        assert_eq!(s.lang, "en");
        assert!(s.on("claude-1a2b3c"));
        assert_eq!(s.to_json()["hidden"], serde_json::json!({}));
        assert_eq!(Settings::parse("not json"), Settings::default());
        assert_eq!(Settings::parse(r#"{"lang":"fr"}"#).lang, "en");
    }

    #[test]
    fn accounts_signed_in_here_have_their_own_keys() {
        let mut s = Settings::parse(r#"{"services":{"claude-1a2b3c":false,"claude":true},"hidden":{"claude-1a2b3c":["five_hour"]},
            "seen":{"claude-1a2b3c":[{"id":"five_hour","span":18000,"name":"x"}]},"removed":["cursor"]}"#);
        assert!(!s.on("claude-1a2b3c"));
        assert_eq!(s.hidden_of("claude-1a2b3c"), ["five_hour".to_string()]);
        assert_eq!(s.seen["claude-1a2b3c"][0].name.as_deref(), Some("x"));
        let text = serde_json::to_string(&s.to_json()).unwrap();
        assert!(text.contains(r#""services":{"claude-1a2b3c":false},"hidden":{"claude-1a2b3c":["five_hour"]}"#), "{text}");
        assert!(!text.contains("removed"), "{text}");
        s.forget("claude-1a2b3c");
        assert!(s.on("claude-1a2b3c") && s.hidden_of("claude-1a2b3c").is_empty() && s.seen.is_empty());
    }
}