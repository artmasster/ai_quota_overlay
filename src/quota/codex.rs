//! Codex (ChatGPT plan) usage, with the login the Codex CLI keeps in `~/.codex/auth.json`.

use std::path::Path;

use serde_json::Value;

use super::{js_number, qerr, read_json, truthy, FetchErr, Res};
use crate::http;
use crate::model::{now_ms, QErr, Quota, Win};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub(super) fn fetch(auth_file: &Path) -> Res<Quota> {
    if !auth_file.exists() {
        return Err(FetchErr::NotLoggedIn);
    }
    let auth = read_json(auth_file)?;
    let tokens = auth.get("tokens");
    let token = match tokens.and_then(|t| t.get("access_token")).and_then(Value::as_str) {
        Some(t) if !t.is_empty() => t,
        _ => return Err(FetchErr::NotLoggedIn),
    };
    let bearer = format!("Bearer {token}");
    let mut headers = vec![("Authorization", bearer.as_str()), ("User-Agent", "codex_cli_rs")];
    if let Some(account) = tokens.and_then(|t| t.get("account_id")).and_then(Value::as_str) {
        headers.push(("chatgpt-account-id", account));
    }

    let res = http::get(USAGE_URL, &headers)?;
    if res.status == 401 {
        return Err(qerr("codexExpired"));
    }
    if res.status == 429 {
        return Err(FetchErr::Quota(QErr::rate_limited(res.retry_at)));
    }
    if !res.ok() {
        return Err(FetchErr::Quota(QErr::status("Codex", res.status)));
    }
    Ok(quota(&res.json()?, now_ms()))
}

/// Usage response → quota (`ok` is set by `settle`).
fn quota(data: &Value, now: i64) -> Quota {
    let rl = data.get("rate_limit").filter(|v| !v.is_null());
    let windows = [("primary", "primary_window"), ("secondary", "secondary_window")]
        .into_iter()
        .filter_map(|(id, key)| {
            let w = rl.and_then(|r| r.get(key)).filter(|w| truthy(Some(w)))?;
            let resets_at = if truthy(w.get("reset_at")) {
                js_number(w.get("reset_at")).map(|s| (s * 1000.0) as i64)
            } else {
                js_number(w.get("reset_after_seconds")).map(|s| now + (s * 1000.0) as i64)
            };
            Some(Win {
                id: id.into(),
                span: w.get("limit_window_seconds").and_then(Value::as_f64),
                used_percent: w.get("used_percent").and_then(Value::as_f64),
                resets_at,
                ..Win::default()
            })
        })
        .collect();
    let c = data.get("credits");
    let credits = if truthy(c.and_then(|c| c.get("has_credits"))) && !truthy(c.and_then(|c| c.get("unlimited"))) {
        js_number(c.and_then(|c| c.get("balance")))
    } else {
        None
    };
    Quota {
        plan: data.get("plan_type").and_then(Value::as_str).unwrap_or("").to_string(),
        windows,
        credits,
        limit_reached: truthy(rl.and_then(|r| r.get("limit_reached"))),
        ..Quota::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_windows_and_credits() {
        let data = json!({
            "plan_type": "plus",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {"used_percent": 23, "limit_window_seconds": 18000, "reset_after_seconds": 600, "reset_at": 1_760_000_000},
                "secondary_window": {"used_percent": 61.5, "limit_window_seconds": 604800, "reset_after_seconds": 3600}
            },
            "credits": {"has_credits": true, "unlimited": false, "balance": "12.50"}
        });
        let q = quota(&data, 1_000_000);
        assert_eq!(q.plan, "plus");
        assert!(!q.limit_reached);
        assert_eq!(q.windows.len(), 2);
        assert_eq!(q.windows[0].id, "primary");
        assert_eq!(q.windows[0].span, Some(18000.0));
        assert_eq!(q.windows[0].used_percent, Some(23.0));
        assert_eq!(q.windows[0].resets_at, Some(1_760_000_000_000));
        assert_eq!(q.windows[1].id, "secondary");
        assert_eq!(q.windows[1].resets_at, Some(1_000_000 + 3_600_000));
        assert_eq!(q.credits, Some(12.5));
    }

    #[test]
    fn missing_parts() {
        let q = quota(&json!({"rate_limit": {"limit_reached": true, "primary_window": null}, "credits": {"has_credits": true, "unlimited": true, "balance": 5}}), 0);
        assert_eq!(q.plan, "");
        assert!(q.windows.is_empty());
        assert!(q.limit_reached);
        assert_eq!(q.credits, None);
        let q = quota(&json!({"credits": {"has_credits": true, "balance": 7}}), 0);
        assert_eq!(q.credits, Some(7.0));
        let q = quota(&json!({"credits": {"has_credits": false, "balance": 7}}), 0);
        assert_eq!(q.credits, None);
        assert!(quota(&json!(null), 0).windows.is_empty());
    }

    #[test]
    fn no_auth_file_is_not_logged_in() {
        let p = std::env::temp_dir().join(format!("aqo-codex-missing-{}", std::process::id())).join("auth.json");
        assert_eq!(fetch(&p).unwrap_err(), FetchErr::NotLoggedIn);
    }
}
