//! GitHub Copilot usage (the monthly premium requests, and chat / completions on the free plan), for an account
//! signed in here with GitHub's device code sign-in, as Copilot's editor plugins do (see `login`).

use serde_json::Value;

use super::store::{self, Saved};
use super::{js_number, parse_date_ms, truthy, FetchErr, Res};
use crate::http;
use crate::model::{QErr, Quota, Win};

/// Public client id of Copilot's editor plugins (a GitHub App: the scope is not used).
const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// Where the user types the code.
pub(crate) const VERIFY_URL: &str = "https://github.com/login/device";
const USER_URL: &str = "https://api.github.com/user";
const QUOTA_URL: &str = "https://api.github.com/copilot_internal/user";
/// What the quota call is sent with, as Copilot Chat sends it.
const HEADERS: [(&str, &str); 4] = [
    ("Editor-Version", "vscode/1.96.2"),
    ("Editor-Plugin-Version", "copilot-chat/0.26.7"),
    ("User-Agent", "GitHubCopilotChat/0.26.7"),
    ("X-Github-Api-Version", "2025-04-01"),
];

/// A device code sign-in under way: the code to show, and how to ask whether it went through.
pub(crate) struct Device {
    pub user_code: String,
    pub device_code: String,
    /// Seconds between asks, and until the code runs out.
    pub interval: u64,
    pub expires_in: u64,
}

pub(crate) fn device_code() -> Res<Device> {
    let res = http::post_form(DEVICE_CODE_URL, &[("Accept", "application/json")], &[("client_id", CLIENT_ID), ("scope", "read:user")])?;
    if !res.ok() {
        return Err(FetchErr::Quota(QErr::status("GitHub", res.status)));
    }
    let v: Value = res.json()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let n = |k: &str, d: u64| v.get(k).and_then(Value::as_u64).unwrap_or(d);
    let d = Device { user_code: s("user_code"), device_code: s("device_code"), interval: n("interval", 5), expires_in: n("expires_in", 900) };
    if d.user_code.is_empty() || d.device_code.is_empty() {
        return Err(FetchErr::Unexpected("GitHub sent no device code".into()));
    }
    Ok(d)
}

/// What one ask about the device code said.
pub(crate) enum Poll {
    Wait,
    /// Ask less often: this many seconds apart.
    Slower(u64),
    Token(String),
    /// Refused, ran out or cancelled on GitHub: the reason.
    Failed(String),
}

pub(crate) fn poll(device_code: &str) -> Res<Poll> {
    let res = http::post_form(
        TOKEN_URL,
        &[("Accept", "application/json")],
        &[("client_id", CLIENT_ID), ("device_code", device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
    )?;
    let v: Value = res.json().unwrap_or(Value::Null);
    if let Some(t) = v.get("access_token").and_then(Value::as_str).filter(|t| !t.is_empty()) {
        return Ok(Poll::Token(t.to_string()));
    }
    Ok(match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => Poll::Wait,
        Some("slow_down") => Poll::Slower(v.get("interval").and_then(Value::as_u64).unwrap_or(10)),
        Some(e) => Poll::Failed(v.get("error_description").and_then(Value::as_str).unwrap_or(e).to_string()),
        None if !res.ok() => Poll::Failed(format!("GitHub returned {}", res.status)),
        None => Poll::Wait,
    })
}

/// The new account for `token`: its GitHub login and Copilot plan.
pub(crate) fn sign_in(token: &str) -> Res<Saved> {
    let auth = format!("token {token}");
    let mut s = Saved { svc: "copilot".into(), access: token.to_string(), ..Saved::default() };
    if let Some(res) = http::get(USER_URL, &[("Authorization", auth.as_str()), ("Accept", "application/json")]).ok().filter(|r| r.ok()) {
        if let Ok(u) = res.json::<Value>() {
            s.email = u.get("login").and_then(Value::as_str).unwrap_or_default().to_string();
        }
    }
    // Copilot answers for this account (else it has no Copilot plan at all).
    let q = usage(token)?;
    s.plan = q.plan;
    Ok(s)
}

fn usage(token: &str) -> Res<Quota> {
    let auth = format!("token {token}");
    let mut headers = vec![("Authorization", auth.as_str()), ("Accept", "application/json")];
    headers.extend(HEADERS);
    let res = http::get(QUOTA_URL, &headers)?;
    if res.status == 401 {
        return Err(super::app_expired("Copilot", Some("401".into())));
    }
    if res.status == 429 {
        return Err(FetchErr::Quota(QErr::rate_limited(res.retry_at)));
    }
    if res.status == 404 || res.status == 403 {
        // Signed in, but no Copilot on this GitHub account.
        return Err(FetchErr::Quota(QErr::code("noCopilot")));
    }
    if !res.ok() {
        return Err(FetchErr::Quota(QErr::status("Copilot", res.status)));
    }
    Ok(quota(&res.json()?))
}

pub(super) fn fetch(id: &str) -> Res<Quota> {
    let s = store::get(id).ok_or(FetchErr::NotLoggedIn)?;
    usage(&s.access)
}

/// "individual" → "pro", as GitHub names the plans.
fn plan_of(data: &Value) -> String {
    if data.get("access_type_sku").and_then(Value::as_str).is_some_and(|s| s.starts_with("free")) {
        return "free".into();
    }
    match data.get("copilot_plan").and_then(Value::as_str).unwrap_or_default() {
        "individual" => "pro".into(),
        "individual_pro" => "pro+".into(),
        p => p.to_string(),
    }
}

/// The quota answer → one window per limited quota (premium requests; chat and completions on the free plan), all
/// resetting on the plan's monthly date.
fn quota(data: &Value) -> Quota {
    const NAMES: [(&str, &str); 3] = [("premium_interactions", "Premium"), ("chat", "Chat"), ("completions", "Completions")];
    let mut windows = Vec::new();
    if let Some(snaps) = data.get("quota_snapshots").filter(|v| v.is_object()) {
        let reset = if truthy(data.get("quota_reset_date")) { parse_date_ms(data.get("quota_reset_date")) } else { None };
        for (key, name) in NAMES {
            let Some(q) = snaps.get(key) else { continue };
            // Unlimited, or a seat billed some other way (nothing to count down).
            if truthy(q.get("unlimited")) || js_number(q.get("entitlement")).is_none_or(|e| e <= 0.0) {
                continue;
            }
            let left = js_number(q.get("percent_remaining")).unwrap_or(0.0).clamp(0.0, 100.0);
            windows.push(Win { id: key.into(), name: Some(name.into()), used_percent: Some(100.0 - left), resets_at: reset, ..Win::default() });
        }
    } else if let (Some(left), Some(monthly)) = (data.get("limited_user_quotas"), data.get("monthly_quotas")) {
        // The free plan: what is left of each monthly allowance.
        let reset = if truthy(data.get("limited_user_reset_date")) { parse_date_ms(data.get("limited_user_reset_date")) } else { None };
        for (key, name) in NAMES.iter().skip(1) {
            let (Some(l), Some(m)) = (js_number(left.get(*key)), js_number(monthly.get(*key))) else { continue };
            if m <= 0.0 {
                continue;
            }
            let pct = (l / m * 100.0).clamp(0.0, 100.0);
            windows.push(Win { id: (*key).into(), name: Some((*name).into()), used_percent: Some(100.0 - pct), resets_at: reset, ..Win::default() });
        }
    }
    let limit_reached = !windows.is_empty() && windows.iter().any(|w| w.used() >= 100.0);
    Quota { plan: plan_of(data), windows, limit_reached, ..Quota::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paid_plan_counts_premium_requests() {
        let q = quota(&json!({
            "access_type_sku": "plus_monthly_subscriber_quota", "copilot_plan": "individual", "quota_reset_date": "2026-11-01",
            "quota_snapshots": {
                "premium_interactions": {"entitlement": 300, "remaining": 240, "percent_remaining": 80, "unlimited": false},
                "chat": {"unlimited": true}, "completions": {"unlimited": true}
            }
        }));
        assert_eq!(q.plan, "pro");
        assert_eq!(q.windows.len(), 1);
        let w = &q.windows[0];
        assert_eq!((w.id.as_str(), w.name.as_deref(), w.used_percent), ("premium_interactions", Some("Premium"), Some(20.0)));
        assert_eq!(w.resets_at, Some(1_793_491_200_000));
        assert!(!q.limit_reached);
        // A seat billed some other way: nothing to show.
        let q = quota(&json!({"copilot_plan": "business", "quota_snapshots": {"premium_interactions": {"entitlement": 0, "percent_remaining": 100}}}));
        assert!(q.windows.is_empty());
        assert_eq!(q.plan, "business");
    }

    #[test]
    fn free_plan_counts_chat_and_completions() {
        let q = quota(&json!({
            "access_type_sku": "free_limited_copilot", "copilot_plan": "individual", "limited_user_reset_date": "2026-11-12",
            "limited_user_quotas": {"chat": 10, "completions": 2000}, "monthly_quotas": {"chat": 50, "completions": 2000}
        }));
        assert_eq!(q.plan, "free");
        let got: Vec<(&str, Option<f64>)> = q.windows.iter().map(|w| (w.id.as_str(), w.used_percent)).collect();
        assert_eq!(got, [("chat", Some(80.0)), ("completions", Some(0.0))]);
        let used_up = quota(&json!({"limited_user_quotas": {"chat": 0}, "monthly_quotas": {"chat": 50}}));
        assert!(used_up.limit_reached);
    }
}
