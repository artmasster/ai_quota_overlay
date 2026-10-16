//! settings.json in the same folder and shape the Electron app used (`app.getPath('userData')`), so existing
//! users keep their settings: card, opacity, vertical, lang, network, services, hidden, seen, skipVersion.
use crate::model::{Svc, Win, SERVICES};
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
    pub lang: String,
    pub network: bool,
    pub services: [bool; 3],
    /// Window ids left off the card, per service.
    pub hidden: [Vec<String>; 3],
    /// The windows each service had last time (shown in the app window while it is off).
    pub seen: [Option<Vec<Win>>; 3],
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
            lang: "en".into(),
            network: false,
            services: [true; 3],
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

impl Settings {
    pub fn load() -> Self {
        let raw = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
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
        if let Some(v) = m.remove("lang").and_then(|v| v.as_str().map(String::from)) {
            s.lang = v;
        }
        if crate::i18n::Lang::from_key(&s.lang).is_none() {
            s.lang = "en".into();
        }
        if let Some(v) = m.remove("network").and_then(|v| v.as_bool()) {
            s.network = v;
        }
        if let Some(Value::Object(sv)) = m.remove("services") {
            for svc in SERVICES {
                if let Some(b) = sv.get(svc.key()).and_then(|v| v.as_bool()) {
                    s.services[svc.index()] = b;
                }
            }
        }
        if let Some(Value::Object(h)) = m.remove("hidden") {
            for svc in SERVICES {
                if let Some(Value::Array(a)) = h.get(svc.key()) {
                    s.hidden[svc.index()] = a.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
            }
        }
        if let Some(Value::Object(seen)) = m.remove("seen") {
            for svc in SERVICES {
                if let Some(v) = seen.get(svc.key()) {
                    s.seen[svc.index()] = serde_json::from_value(v.clone()).ok();
                }
            }
        }
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
        let per = |f: &dyn Fn(Svc) -> Value| -> Value {
            let mut o = Map::new();
            for svc in SERVICES {
                o.insert(svc.key().into(), f(svc));
            }
            Value::Object(o)
        };
        m.insert("services".into(), per(&|s| Value::from(self.services[s.index()])));
        m.insert("hidden".into(), per(&|s| Value::from(self.hidden[s.index()].clone())));
        let mut seen = Map::new();
        for svc in SERVICES {
            if let Some(w) = &self.seen[svc.index()] {
                seen.insert(svc.key().into(), serde_json::to_value(w).unwrap_or(Value::Null));
            }
        }
        m.insert("seen".into(), Value::Object(seen));
        m.insert("skipVersion".into(), self.skip_version.clone().map(Value::from).unwrap_or(Value::Null));
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
    fn reads_and_writes_the_electron_shape() {
        let raw = r#"{"card":{"x":2271.6,"y":1265,"w":296,"h":97},"opacity":0.85,"vertical":true,"lang":"th","network":true,
            "services":{"claude":true,"codex":false,"cursor":true},"hidden":{"claude":["seven_day_opus"],"codex":[],"cursor":[]},
            "seen":{"claude":[{"id":"five_hour","span":18000},{"id":"seven_day_opus","span":604800,"name":"Opus"}],
            "cursor":[{"id":"billing","billing":true}]},"skipVersion":"1.3.0","futureKey":42}"#;
        let v: Value = serde_json::from_str(raw).unwrap();
        let path = std::env::temp_dir().join("aqo-settings-test.json");
        std::fs::write(&path, raw).unwrap();
        // Parse through the same code path as load().
        let mut s = Settings::default();
        if let Value::Object(_) = &v {
            s = parse_for_test(raw);
        }
        assert_eq!(s.card.unwrap().x, 2271.6);
        assert_eq!(s.lang, "th");
        assert!(!s.services[1]);
        assert_eq!(s.hidden[0], vec!["seven_day_opus".to_string()]);
        assert_eq!(s.seen[0].as_ref().unwrap()[1].name.as_deref(), Some("Opus"));
        assert!(s.seen[2].as_ref().unwrap()[0].billing);
        assert_eq!(s.skip_version.as_deref(), Some("1.3.0"));
        let out = s.to_json();
        assert_eq!(out["futureKey"], 42);
        assert_eq!(out["seen"]["claude"][0], serde_json::json!({"id": "five_hour", "span": 18000.0}));
        assert_eq!(out["services"]["codex"], false);
    }

    fn parse_for_test(raw: &str) -> Settings {
        let dir = std::env::temp_dir().join(format!("aqo-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(APP_NAME)).unwrap();
        std::fs::write(dir.join(APP_NAME).join("settings.json"), raw).unwrap();
        let var = if cfg!(windows) { "APPDATA" } else if cfg!(target_os = "macos") { "HOME" } else { "XDG_CONFIG_HOME" };
        let old = std::env::var_os(var);
        let base = if cfg!(target_os = "macos") {
            let h = dir.join("home");
            std::fs::create_dir_all(h.join("Library/Application Support").join(APP_NAME)).unwrap();
            std::fs::write(h.join("Library/Application Support").join(APP_NAME).join("settings.json"), raw).unwrap();
            h
        } else {
            dir.clone()
        };
        std::env::set_var(var, &base);
        let s = Settings::load();
        match old {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
        s
    }
}
