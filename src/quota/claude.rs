//! Claude (claude.ai subscription) usage, for an account signed in here with Claude Code's own browser sign-in
//! (see `login`).

use serde_json::{json, Value};

use super::store::{self, Saved};
use super::{entries_of, entry, truthy, FetchErr, Res};
use crate::http;
use crate::model::{now_ms, Extra, QErr, Quota, Win};

/// Public OAuth client of Claude Code: signs the user in to their own account and refreshes its token.
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
/// What Claude Code asks for when it signs in with a claude.ai account.
const SCOPES: &str = "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
const TOKEN_URLS: [&str; 2] = ["https://platform.claude.com/v1/oauth/token", "https://console.anthropic.com/v1/oauth/token"];
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const BETA: (&str, &str) = ("anthropic-beta", "oauth-2025-04-20");

const H: f64 = 3600.0;

/// Known windows: span in seconds, plus a model name or an i18n tag.
fn known(key: &str) -> Option<(f64, Option<&'static str>, Option<&'static str>)> {
    Some(match key {
        "five_hour" => (5.0 * H, None, None),
        "seven_day" => (168.0 * H, None, None),
        "seven_day_opus" => (168.0 * H, Some("Opus"), None),
        "seven_day_sonnet" => (168.0 * H, Some("Sonnet"), None),
        "seven_day_oauth_apps" => (168.0 * H, None, Some("otherApps")),
        _ => return None,
    })
}

/// Posts `body` to the token endpoint (the current one, then the old one); the answer of the first that says yes.
/// A refusal means signing in again.
fn token_request(body: Value) -> Res<Value> {
    let mut last_status = 0u16;
    for url in TOKEN_URLS {
        let Ok(res) = http::post_json(url, &[], &body) else { continue };
        last_status = res.status;
        if !res.ok() {
            continue;
        }
        return Ok(res.json()?);
    }
    Err(if last_status != 0 { super::app_expired("Claude", Some(last_status.to_string())) } else { FetchErr::Offline })
}

/// The browser page that asks the user to let this app see their account; it comes back to `redirect` with a code.
pub(crate) fn authorize_url(redirect: &str, challenge: &str, state: &str) -> String {
    let q = super::query(&[
        ("code", "true"),
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect),
        ("scope", SCOPES),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ]);
    format!("{AUTHORIZE_URL}?{q}")
}

/// "claude_max" → "max", as Claude Code names its subscription types.
fn plan_of(org_type: &str) -> String {
    org_type.strip_prefix("claude_").unwrap_or(org_type).to_string()
}

/// Turns the code from the browser into tokens, and finds the account's email and plan.
pub(crate) fn sign_in(code: &str, redirect: &str, verifier: &str, state: &str) -> Res<Saved> {
    let t = token_request(json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect,
        "client_id": CLIENT_ID,
        "code_verifier": verifier,
        "state": state,
    }))
    .map_err(|e| match e {
        // Refused at sign-in: say so with the status, not "sign in again".
        FetchErr::Quota(q) => FetchErr::Quota(QErr::status("Claude", q.status.unwrap_or_default())),
        e => e,
    })?;
    let access = t.get("access_token").and_then(Value::as_str).unwrap_or_default().to_string();
    if access.is_empty() {
        return Err(FetchErr::Unexpected("the sign-in returned no access_token".into()));
    }
    let mut s = Saved {
        svc: "claude".into(),
        email: t.pointer("/account/email_address").and_then(Value::as_str).unwrap_or_default().to_string(),
        access,
        refresh: t.get("refresh_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        expires_at: now_ms() + (t.get("expires_in").and_then(Value::as_f64).unwrap_or(3600.0) * 1000.0) as i64,
        ..Saved::default()
    };
    // The plan (and the email, if the token answer had none) from the profile; the numbers work without it.
    let auth = format!("Bearer {}", s.access);
    if let Some(res) = http::get(PROFILE_URL, &[("Authorization", auth.as_str()), BETA]).ok().filter(|r| r.ok()) {
        if let Ok(p) = res.json::<Value>() {
            s.plan = plan_of(p.pointer("/organization/organization_type").and_then(Value::as_str).unwrap_or_default());
            if s.email.is_empty() {
                s.email = p.pointer("/account/email").and_then(Value::as_str).unwrap_or_default().to_string();
            }
        }
    }
    Ok(s)
}

/// A fresh access token for account `id`, refreshed (and stored) when it is about to run out or `force`.
fn token(id: &str, force: bool) -> Res<String> {
    let s = store::get(id).ok_or(FetchErr::NotLoggedIn)?;
    if !force && s.expires_at - now_ms() > 60_000 {
        return Ok(s.access);
    }
    let t = token_request(json!({"grant_type": "refresh_token", "refresh_token": s.refresh, "client_id": CLIENT_ID}))?;
    let Some(access) = t.get("access_token").and_then(Value::as_str) else {
        return Err(FetchErr::Unexpected("token refresh returned no access_token".into()));
    };
    let refresh = t.get("refresh_token").and_then(Value::as_str).unwrap_or(&s.refresh).to_string();
    let expires_at = now_ms() + (t.get("expires_in").and_then(Value::as_f64).unwrap_or(3600.0) * 1000.0) as i64;
    // Signed in again meanwhile (new tokens): keep those.
    let stored = store::update(id, |x| {
        if x.refresh == s.refresh {
            x.access = access.to_string();
            x.refresh = refresh;
            x.expires_at = expires_at;
        }
    })?;
    stored.map(|x| x.access).ok_or(FetchErr::NotLoggedIn)
}

/// Usage response entries (in document order) → windows and extra usage.
fn windows(entries: &[(String, Value)]) -> (Vec<Win>, Option<Extra>) {
    let windows = entries
        .iter()
        .filter_map(|(key, w)| {
            // Unknown internal buckets without a reset time are placeholders, not real limits.
            let used = match w.get("utilization") {
                Some(Value::Number(n)) => n.as_f64()?,
                _ => return None,
            };
            let k = known(key);
            if k.is_none() && !truthy(w.get("resets_at")) {
                return None;
            }
            let (span, name, tag) = match k {
                Some((span, name, tag)) => (Some(span), name.map(String::from), tag.map(String::from)),
                None => (None, Some(key.replace('_', " ")), None),
            };
            let resets_at = if truthy(w.get("resets_at")) { super::parse_date_ms(w.get("resets_at")) } else { None };
            Some(Win { id: key.clone(), span, name, tag, billing: false, used_percent: Some(used), resets_at })
        })
        .collect();
    let extra = entry(entries, "extra_usage").filter(|x| truthy(x.get("is_enabled"))).map(|x| Extra {
        used: x.get("used_credits").and_then(Value::as_f64).unwrap_or(0.0),
        limit: x.get("monthly_limit").and_then(Value::as_f64).unwrap_or(0.0),
    });
    (windows, extra)
}

/// Account `id`'s usage; a 401 tries once more with a refreshed token.
pub(super) fn fetch(id: &str) -> Res<Quota> {
    let plan = store::get(id).ok_or(FetchErr::NotLoggedIn)?.plan;
    let call = |token: String| {
        let auth = format!("Bearer {token}");
        http::get(USAGE_URL, &[("Authorization", auth.as_str()), BETA])
    };
    let mut res = call(token(id, false)?)?;
    if res.status == 401 {
        res = call(token(id, true)?)?;
    }
    if res.status == 429 {
        return Err(FetchErr::Quota(QErr::rate_limited(res.retry_at)));
    }
    if res.status == 401 {
        return Err(super::app_expired("Claude", Some("401".into())));
    }
    if !res.ok() {
        return Err(FetchErr::Quota(QErr::status("Claude", res.status)));
    }
    let (windows, extra) = windows(&entries_of(&res.body)?);
    Ok(Quota { plan, windows, extra, ..Quota::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_pieces() {
        let u = authorize_url("http://localhost:5555/callback", "chal", "st");
        assert!(u.starts_with("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c250a-"), "{u}");
        assert!(u.contains("&redirect_uri=http%3A%2F%2Flocalhost%3A5555%2Fcallback&scope=user%3Aprofile%20user%3Ainference"), "{u}");
        assert!(u.ends_with("&code_challenge=chal&code_challenge_method=S256&state=st"), "{u}");
        assert_eq!(plan_of("claude_max"), "max");
        assert_eq!(plan_of("claude_pro"), "pro");
        assert_eq!(plan_of(""), "");
    }

    #[test]
    fn maps_windows_in_response_order() {
        let body = br#"{
            "five_hour": {"utilization": 12.5, "resets_at": "2025-10-16T10:00:00.123456+00:00"},
            "seven_day": {"utilization": 40, "resets_at": "2025-10-20T00:00:00Z"},
            "seven_day_oauth_apps": null,
            "seven_day_opus": {"utilization": 0, "resets_at": null},
            "iguana_necktie": {"utilization": 3, "resets_at": null},
            "new_bucket_x": {"utilization": 7, "resets_at": "2025-10-17T00:00:00Z"},
            "seven_day_sonnet": {"utilization": "5", "resets_at": "2025-10-20T00:00:00Z"},
            "extra_usage": {"is_enabled": true, "used_credits": 120.5, "monthly_limit": 5000, "utilization": null}
        }"#;
        let (w, extra) = windows(&entries_of(body).unwrap());
        let ids: Vec<&str> = w.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_opus", "new_bucket_x"]);
        assert_eq!(w[0].span, Some(18000.0));
        assert_eq!(w[0].used_percent, Some(12.5));
        assert_eq!(w[0].resets_at, Some(1_760_608_800_123));
        assert_eq!(w[1].span, Some(604800.0));
        assert_eq!(w[2].name.as_deref(), Some("Opus"));
        assert_eq!(w[2].resets_at, None);
        assert_eq!(w[3].name.as_deref(), Some("new bucket x"));
        assert_eq!(w[3].span, None);
        assert_eq!(extra, Some(Extra { used: 120.5, limit: 5000.0 }));

        let (w, extra) = windows(&entries_of(br#"{"seven_day_oauth_apps": {"utilization": 1}, "extra_usage": {"is_enabled": false}}"#).unwrap());
        assert_eq!(w[0].tag.as_deref(), Some("otherApps"));
        assert_eq!(extra, None);
    }
}
