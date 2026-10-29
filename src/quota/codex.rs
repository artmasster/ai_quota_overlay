//! Codex (ChatGPT plan) usage, for an account signed in here with the Codex CLI's own browser sign-in (see
//! `login`).

use serde_json::{json, Value};

use super::store::{self, Saved};
use super::{js_number, jwt_exp_ms, jwt_payload, qerr, truthy, FetchErr, Res};
use crate::http;
use crate::model::{now_ms, QErr, Quota, Win};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// Public OAuth client of the Codex CLI. OpenAI only takes this redirect for it, so sign-in needs port 1455.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
pub(crate) const PORT: u16 = 1455;
pub(crate) const REDIRECT: &str = "http://localhost:1455/auth/callback";
/// The ChatGPT claims inside OpenAI's tokens.
const AUTH_CLAIM: &str = "https://api.openai.com/auth";

fn usage(token: &str, account: &str) -> Res<Quota> {
    let bearer = format!("Bearer {token}");
    let mut headers = vec![("Authorization", bearer.as_str()), ("User-Agent", "codex_cli_rs")];
    if !account.is_empty() {
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

// ---------- accounts signed in here ----------

pub(crate) fn authorize_url(challenge: &str, state: &str) -> String {
    let q = super::query(&[
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", REDIRECT),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "codex_cli_rs"),
    ]);
    format!("{ISSUER}/oauth/authorize?{q}")
}

/// Fills in what the tokens say about the account: email and plan (id token), ChatGPT account id, expiry.
fn from_tokens(s: &mut Saved, t: &Value) {
    let id_claims = t.get("id_token").and_then(Value::as_str).and_then(jwt_payload);
    let access_claims = jwt_payload(&s.access);
    let claim = |k: &str| {
        [&id_claims, &access_claims]
            .iter()
            .find_map(|c| c.as_ref()?.get(AUTH_CLAIM)?.get(k)?.as_str().filter(|v| !v.is_empty()).map(String::from))
    };
    if let Some(e) = id_claims.as_ref().and_then(|c| c.get("email")).and_then(Value::as_str) {
        s.email = e.to_string();
    }
    if let Some(a) = claim("chatgpt_account_id") {
        s.account = a;
    }
    if let Some(p) = claim("chatgpt_plan_type") {
        s.plan = p;
    }
    s.expires_at = jwt_exp_ms(&s.access);
}

pub(crate) fn sign_in(code: &str, verifier: &str) -> Res<Saved> {
    let res = http::post_form(
        &format!("{ISSUER}/oauth/token"),
        &[],
        &[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", REDIRECT), ("client_id", CLIENT_ID), ("code_verifier", verifier)],
    )?;
    if !res.ok() {
        return Err(FetchErr::Quota(QErr::status("auth.openai.com", res.status)));
    }
    let t: Value = res.json()?;
    let mut s = Saved {
        svc: "codex".into(),
        access: t.get("access_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        refresh: t.get("refresh_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        ..Saved::default()
    };
    if s.access.is_empty() {
        return Err(FetchErr::Unexpected("the sign-in returned no access_token".into()));
    }
    from_tokens(&mut s, &t);
    Ok(s)
}

/// New tokens for account `s` (as the CLI refreshes), stored; a refusal means signing in again.
fn refresh(s: &Saved) -> Res<Saved> {
    let body = json!({"client_id": CLIENT_ID, "grant_type": "refresh_token", "refresh_token": s.refresh, "scope": "openid profile email"});
    let res = http::post_json(&format!("{ISSUER}/oauth/token"), &[], &body)?;
    if !res.ok() {
        return Err(super::app_expired("Codex", Some(res.status.to_string())));
    }
    let t: Value = res.json()?;
    let Some(access) = t.get("access_token").and_then(Value::as_str) else {
        return Err(FetchErr::Unexpected("token refresh returned no access_token".into()));
    };
    let mut next = Saved { access: access.to_string(), ..s.clone() };
    if let Some(r) = t.get("refresh_token").and_then(Value::as_str) {
        next.refresh = r.to_string();
    }
    from_tokens(&mut next, &t);
    let stored = store::update(&s.id, |x| {
        if x.refresh == s.refresh {
            *x = next.clone();
        }
    })?;
    stored.ok_or(FetchErr::NotLoggedIn)
}

pub(super) fn fetch(id: &str) -> Res<Quota> {
    let mut s = store::get(id).ok_or(FetchErr::NotLoggedIn)?;
    if s.expires_at > 0 && s.expires_at - now_ms() < 60_000 {
        s = refresh(&s)?;
    }
    let q = match usage(&s.access, &s.account) {
        Err(FetchErr::Quota(e)) if e.code == "codexExpired" => {
            s = refresh(&s)?;
            usage(&s.access, &s.account)
        }
        r => r,
    };
    let q = match q {
        Err(FetchErr::Quota(e)) if e.code == "codexExpired" => return Err(super::app_expired("Codex", Some("401".into()))),
        r => r?,
    };
    // The usage answer names the plan too; the token's claim is the fallback.
    Ok(if q.plan.is_empty() { Quota { plan: s.plan, ..q } } else { q })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::b64url;

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

    fn jwt(v: Value) -> String {
        format!("h.{}.s", b64url(&serde_json::to_vec(&v).unwrap()))
    }

    #[test]
    fn tokens_name_the_account() {
        let id = jwt(json!({"email": "me@example.com", AUTH_CLAIM: {"chatgpt_plan_type": "plus", "chatgpt_account_id": "acc-1"}}));
        let mut s = Saved { access: jwt(json!({"exp": 1_800_000_000})), ..Saved::default() };
        from_tokens(&mut s, &json!({"id_token": id}));
        assert_eq!((s.email.as_str(), s.plan.as_str(), s.account.as_str()), ("me@example.com", "plus", "acc-1"));
        assert_eq!(s.expires_at, 1_800_000_000_000);
        let u = authorize_url("ch", "st");
        assert!(u.starts_with("https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_EMoamEEZ73f0CkXaXp7hrann&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&scope=openid%20profile%20email%20offline_access"), "{u}");
    }
}
