//! quota.json next to settings.json: each account's last good numbers and when they were fetched, so a check that
//! fails right after a restart ("too many requests", offline) still has numbers to show. Usage only, no logins.
//! Keyed by account id; older versions' entries under a bare service key (other apps' logins) are dropped.
use crate::model::{Extra, Quota, Svc, Win};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub type Good = BTreeMap<String, Quota>;

#[derive(Serialize, Deserialize)]
struct CWin {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    span: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    billing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resets_at: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct CQuota {
    at: i64,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    windows: Vec<CWin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra: Option<(f64, f64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credits: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    limit_reached: bool,
}

fn file() -> PathBuf {
    crate::settings::data_dir().join("quota.json")
}

/// A good quota as stored; `q.as_of` is when it was fetched.
fn to_c(q: &Quota) -> Option<CQuota> {
    Some(CQuota {
        at: q.as_of?,
        plan: q.plan.clone(),
        windows: q
            .windows
            .iter()
            .map(|w| CWin {
                id: w.id.clone(),
                span: w.span,
                name: w.name.clone(),
                tag: w.tag.clone(),
                billing: w.billing,
                used: w.used_percent,
                resets_at: w.resets_at,
            })
            .collect(),
        extra: q.extra.as_ref().map(|x| (x.used, x.limit)),
        credits: q.credits,
        limit_reached: q.limit_reached,
    })
}

fn from_c(c: CQuota) -> Quota {
    Quota {
        ok: true,
        plan: c.plan,
        windows: c
            .windows
            .into_iter()
            .map(|w| Win {
                id: w.id,
                span: w.span,
                name: w.name,
                tag: w.tag,
                billing: w.billing,
                used_percent: w.used,
                resets_at: w.resets_at,
            })
            .collect(),
        extra: c.extra.map(|(used, limit)| Extra { used, limit }),
        credits: c.credits,
        limit_reached: c.limit_reached,
        as_of: Some(c.at),
        ..Quota::default()
    }
}

pub fn parse(text: &str) -> Good {
    let m: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).unwrap_or_default();
    m.into_iter()
        .filter(|(id, _)| Svc::from_key(id).is_none())
        .filter_map(|(id, v)| Some((id, from_c(serde_json::from_value::<CQuota>(v).ok()?))))
        .collect()
}

pub fn to_json(good: &Good) -> String {
    let m: serde_json::Map<String, serde_json::Value> =
        good.iter().filter_map(|(id, q)| Some((id.clone(), serde_json::to_value(to_c(q)?).unwrap_or_default()))).collect();
    serde_json::Value::Object(m).to_string()
}

pub fn load() -> Good {
    parse(&std::fs::read_to_string(file()).unwrap_or_default())
}

pub fn save(good: &Good) {
    let path = file();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(&path, to_json(good));
}

/// `q` (good numbers fetched at `q.as_of`) as they most likely stand at `now`: a window whose reset time has
/// passed starts over (nothing used). None once they are older than `max_age_ms`.
pub fn aged(q: &Quota, now: i64, max_age_ms: i64) -> Option<Quota> {
    if now - q.as_of? > max_age_ms {
        return None;
    }
    let mut q = q.clone();
    for w in &mut q.windows {
        if w.resets_at.is_some_and(|r| r <= now) {
            w.used_percent = Some(0.0);
            w.resets_at = None;
        }
    }
    if q.windows.iter().all(|w| w.used() < 100.0) {
        q.limit_reached = false;
    }
    Some(q)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Quota {
        Quota {
            ok: true,
            plan: "max".into(),
            windows: vec![
                Win { id: "five_hour".into(), span: Some(18000.0), used_percent: Some(40.0), resets_at: Some(5_000), ..Win::default() },
                Win { id: "seven_day_opus".into(), span: Some(604800.0), name: Some("Opus".into()), used_percent: Some(100.0), resets_at: Some(90_000), ..Win::default() },
            ],
            extra: Some(Extra { used: 120.5, limit: 5000.0 }),
            limit_reached: true,
            as_of: Some(1_000),
            ..Quota::default()
        }
    }

    #[test]
    fn round_trip() {
        let all: Good = [
            ("claude-0a0b0c".to_string(), good()),
            ("cursor-77aa00".to_string(), Quota { ok: true, as_of: Some(7), credits: Some(3.5), ..Quota::default() }),
            ("claude-1a2b3c".to_string(), Quota { plan: "pro".into(), ..good() }),
        ]
        .into();
        let back = parse(&to_json(&all));
        assert_eq!(back, all);
        // Without a fetch time there is nothing to store.
        assert!(parse(&to_json(&[("claude-0a0b0c".to_string(), Quota { ok: true, ..Quota::default() })].into())).is_empty());
        assert!(parse("not json").is_empty());
        assert!(parse(r#"{"claude-0a0b0c":{"plan":"x"}}"#).is_empty());
        // An older version's entry for another app's login.
        assert!(parse(r#"{"claude":{"at":5,"plan":"max"}}"#).is_empty());
    }

    #[test]
    fn aging() {
        let q = good();
        assert_eq!(aged(&q, 2_000, 100_000).unwrap(), q);
        let later = aged(&q, 10_000, 100_000).unwrap();
        assert_eq!(later.windows[0].used_percent, Some(0.0));
        assert_eq!(later.windows[0].resets_at, None);
        assert_eq!(later.windows[1], q.windows[1]);
        assert!(later.limit_reached);
        let reset = aged(&q, 95_000, 100_000).unwrap();
        assert!(!reset.limit_reached);
        assert_eq!(aged(&q, 200_000, 100_000), None);
        assert_eq!(aged(&Quota { as_of: None, ..q }, 2_000, 100_000), None);
    }
}
