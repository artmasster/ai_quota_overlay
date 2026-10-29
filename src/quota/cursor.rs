//! Cursor usage, for an account signed in here with the Cursor app's own browser sign-in (see `login`).

use chrono::{Datelike, Days, LocalResult, NaiveDate, TimeZone};
use serde_json::{json, Value};

use super::store::{self, Saved};
use super::{encode_uri_component, entries_of, entry, js_number, jwt_exp_ms, jwt_payload, parse_date_ms, qerr, truthy, FetchErr, Res};
use crate::http;
use crate::model::{now_ms, Extra, QErr, Quota, Win};

const SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const OLD_USAGE_URL: &str = "https://cursor.com/api/usage?user=";
const ME_URL: &str = "https://cursor.com/api/auth/me";
/// The Cursor app's sign-in: the browser page, then this app asks until the page says yes.
const LOGIN_URL: &str = "https://cursor.com/loginDeepControl";
const POLL_URL: &str = "https://api2.cursor.sh/auth/poll";
const REFRESH_URL: &str = "https://api2.cursor.sh/oauth/token";
/// Public OAuth client of the Cursor app (its token refresh).
const CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";

/// `String(payload.sub).split('|').pop()`.
fn user_id(payload: &Value) -> String {
    let sub = match payload.get("sub") {
        None => "undefined".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    };
    sub.rsplit('|').next().unwrap_or_default().to_string()
}

/// JS `d.setMonth(d.getMonth() + 1)` in time zone `tz`: same local day and time a month later, with JS's
/// day overflow (Jan 31 → Mar 2/3).
fn add_month<Tz: TimeZone>(ms: i64, tz: &Tz) -> Option<i64> {
    let local = tz.timestamp_millis_opt(ms).single()?.naive_local();
    let (y, m) = if local.month() == 12 { (local.year() + 1, 1) } else { (local.year(), local.month() + 1) };
    let date = NaiveDate::from_ymd_opt(y, m, 1)?.checked_add_days(Days::new(local.day() as u64 - 1))?;
    let naive = date.and_time(local.time());
    let dt = match tz.from_local_datetime(&naive) {
        LocalResult::Single(d) => d,
        LocalResult::Ambiguous(earlier, _) => earlier,
        // In a DST gap JS moves forward by the gap.
        LocalResult::None => tz.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest()?,
    };
    Some(dt.timestamp_millis())
}

/// usage-summary response (current, usage-based plans) → quota.
fn summary_quota(data: &Value, plan: &str) -> Quota {
    let usage = data.get("individualUsage");
    let p = usage.and_then(|u| u.get("plan")).filter(|p| truthy(Some(p)));
    let windows = p
        .map(|p| {
            let used = match p.get("totalPercentUsed") {
                Some(Value::Number(n)) => n.as_f64(),
                _ if truthy(p.get("limit")) => match (js_number(p.get("used")), js_number(p.get("limit"))) {
                    (Some(u), Some(l)) => Some(u / l * 100.0).filter(|v| !v.is_nan()),
                    _ => None,
                },
                _ => Some(0.0),
            };
            let resets_at = if truthy(data.get("billingCycleEnd")) { parse_date_ms(data.get("billingCycleEnd")) } else { None };
            vec![Win { id: "billing".into(), billing: true, used_percent: used, resets_at, ..Win::default() }]
        })
        .unwrap_or_default();
    let od = usage.and_then(|u| u.get("onDemand"));
    let extra = od.filter(|od| truthy(od.get("enabled")) && truthy(od.get("limit"))).map(|od| Extra {
        used: js_number(od.get("used")).unwrap_or(0.0),
        limit: js_number(od.get("limit")).unwrap_or(0.0),
    });
    let plan = match data.get("membershipType") {
        None | Some(Value::Null) => plan.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    };
    Quota { plan, windows, extra, ..Quota::default() }
}

/// Old `/api/usage` response (request-count plans) → quota: one window per model, all resetting a month
/// after `startOfMonth` (local time).
fn old_quota<Tz: TimeZone>(entries: &[(String, Value)], plan: &str, tz: &Tz) -> Quota {
    let start = entry(entries, "startOfMonth").filter(|v| truthy(Some(v)));
    let start_ms = start.and_then(|v| match v {
        Value::Number(n) => n.as_f64().map(|f| f as i64),
        v => parse_date_ms(Some(v)),
    });
    let reset = start_ms.and_then(|ms| add_month(ms, tz));
    let windows = entries
        .iter()
        .filter(|(_, u)| truthy(u.get("maxRequestUsage")))
        .map(|(model, u)| {
            let used = match (js_number(u.get("numRequests")), js_number(u.get("maxRequestUsage"))) {
                (Some(n), Some(m)) => Some(n / m * 100.0).filter(|v| !v.is_nan()),
                _ => None,
            };
            Win { id: model.clone(), name: Some(model.clone()), used_percent: used, resets_at: reset, ..Win::default() }
        })
        .collect();
    Quota { plan: plan.to_string(), windows, extra: None, ..Quota::default() }
}

/// The dashboard's session cookie for a Cursor access token.
fn cookie(user: &str, token: &str) -> String {
    format!("WorkosCursorSessionToken={user}%3A%3A{token}")
}

fn usage(token: &str, user: &str, plan: &str) -> Res<Quota> {
    let cookie = cookie(user, token);
    let headers = [("Cookie", cookie.as_str()), ("Origin", "https://cursor.com")];

    // Current (usage-based) plans.
    let res = http::get(SUMMARY_URL, &headers)?;
    if res.status == 401 || res.status == 403 {
        return Err(qerr("cursorExpired"));
    }
    if res.ok() {
        return Ok(summary_quota(&res.json()?, plan));
    }

    // Older request-count plans.
    let old = http::get(&format!("{OLD_USAGE_URL}{}", encode_uri_component(user)), &headers)?;
    if !old.ok() && (res.status == 429 || old.status == 429) {
        return Err(FetchErr::Quota(QErr::rate_limited(res.retry_at.max(old.retry_at))));
    }
    if !old.ok() {
        return Err(FetchErr::Quota(QErr::status("Cursor", format!("{}/{}", res.status, old.status))));
    }
    Ok(old_quota(&entries_of(&old.body)?, plan, &chrono::Local))
}

// ---------- accounts signed in here ----------

pub(crate) fn login_url(challenge: &str, uuid: &str) -> String {
    format!("{LOGIN_URL}?{}", super::query(&[("challenge", challenge), ("uuid", uuid), ("mode", "login")]))
}

/// One ask whether the browser sign-in `uuid` went through: None while it hasn't (yet).
pub(crate) fn poll(uuid: &str, verifier: &str) -> Res<Option<Saved>> {
    let res = http::get(&format!("{POLL_URL}?{}", super::query(&[("uuid", uuid), ("verifier", verifier)])), &[])?;
    if res.status == 404 {
        return Ok(None);
    }
    if !res.ok() {
        return Err(FetchErr::Quota(QErr::status("Cursor", res.status)));
    }
    let v: Value = res.json()?;
    let access = v.get("accessToken").and_then(Value::as_str).unwrap_or_default().to_string();
    if access.is_empty() {
        return Ok(None);
    }
    let user = match v.get("authId").and_then(Value::as_str) {
        Some(a) if !a.is_empty() => a.rsplit('|').next().unwrap_or(a).to_string(),
        _ => jwt_payload(&access).map(|p| user_id(&p)).unwrap_or_default(),
    };
    let mut s = Saved {
        svc: "cursor".into(),
        refresh: v.get("refreshToken").and_then(Value::as_str).unwrap_or(&access).to_string(),
        expires_at: jwt_exp_ms(&access),
        account: user,
        access,
        ..Saved::default()
    };
    let c = cookie(&s.account, &s.access);
    if let Some(me) = http::get(ME_URL, &[("Cookie", c.as_str()), ("Origin", "https://cursor.com")]).ok().filter(|r| r.status == 200) {
        if let Ok(m) = me.json::<Value>() {
            s.email = m.get("email").and_then(Value::as_str).unwrap_or_default().to_string();
        }
    }
    Ok(Some(s))
}

/// A new access token for account `s`, stored; a refusal (or "log out") means signing in again.
fn refresh(s: &Saved) -> Res<Saved> {
    let res = http::post_json(REFRESH_URL, &[], &json!({"grant_type": "refresh_token", "client_id": CLIENT_ID, "refresh_token": s.refresh}))?;
    let v: Value = if res.ok() { res.json()? } else { Value::Null };
    let access = v.get("access_token").and_then(Value::as_str).unwrap_or_default();
    if access.is_empty() || truthy(v.get("shouldLogout")) {
        return Err(super::app_expired("Cursor", (!res.ok()).then(|| res.status.to_string())));
    }
    let access = access.to_string();
    let stored = store::update(&s.id, |x| {
        if x.refresh == s.refresh {
            x.expires_at = jwt_exp_ms(&access);
            x.access = access.clone();
        }
    })?;
    stored.ok_or(FetchErr::NotLoggedIn)
}

pub(super) fn fetch(id: &str) -> Res<Quota> {
    let mut s = store::get(id).ok_or(FetchErr::NotLoggedIn)?;
    if s.expires_at > 0 && s.expires_at - now_ms() < 60_000 {
        s = refresh(&s)?;
    }
    match usage(&s.access, &s.account, &s.plan) {
        Err(FetchErr::Quota(e)) if e.code == "cursorExpired" => {
            s = refresh(&s)?;
            usage(&s.access, &s.account, &s.plan).map_err(|e| match e {
                FetchErr::Quota(q) if q.code == "cursorExpired" => super::app_expired("Cursor", None),
                e => e,
            })
        }
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
    use base64::Engine;
    use chrono::{FixedOffset, Utc};
    use serde_json::json;

    fn fake_jwt(payload: &Value, padded: bool) -> String {
        let p = serde_json::to_vec(payload).unwrap();
        let p = if padded { URL_SAFE.encode(p) } else { URL_SAFE_NO_PAD.encode(p) };
        format!("eyJhbGciOiJub25lIn0.{p}.sig")
    }

    #[test]
    fn jwt_payload_decoding() {
        let payload = json!({"sub": "auth0|user_01ABC", "exp": 4102444800u64, "n": "é?>"});
        for padded in [false, true] {
            let got = jwt_payload(&fake_jwt(&payload, padded)).unwrap();
            assert_eq!(got, payload);
            assert_eq!(user_id(&got), "user_01ABC");
        }
        assert_eq!(jwt_payload("no-dots"), None);
        assert_eq!(jwt_payload("a.!!!.c"), None);
        assert_eq!(user_id(&json!({"sub": "plain"})), "plain");
        assert_eq!(user_id(&json!({})), "undefined");
        assert_eq!(user_id(&json!({"sub": 42})), "42");
    }

    #[test]
    fn uri_component() {
        assert_eq!(encode_uri_component("user_01AB-c.d~"), "user_01AB-c.d~");
        assert_eq!(encode_uri_component("a b|c/é"), "a%20b%7Cc%2F%C3%A9");
    }

    #[test]
    fn summary_mapping() {
        let data = json!({
            "billingCycleStart": "2025-10-01T00:00:00.000Z",
            "billingCycleEnd": "2025-11-01T00:00:00.000Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {"enabled": true, "used": 1500, "limit": 2000, "totalPercentUsed": 42.5},
                "onDemand": {"enabled": true, "used": 300, "limit": 1000}
            }
        });
        let q = summary_quota(&data, "free");
        assert_eq!(q.plan, "pro");
        assert_eq!(q.windows.len(), 1);
        let w = &q.windows[0];
        assert_eq!((w.id.as_str(), w.billing, w.used_percent), ("billing", true, Some(42.5)));
        assert_eq!(w.resets_at, Some(1_761_955_200_000));
        assert_eq!(q.extra, Some(Extra { used: 300.0, limit: 1000.0 }));

        let q = summary_quota(&json!({"individualUsage": {"plan": {"used": 500, "limit": 2000}, "onDemand": {"enabled": true, "limit": 0}}}), "free");
        assert_eq!(q.plan, "free");
        assert_eq!(q.windows[0].used_percent, Some(25.0));
        assert_eq!(q.windows[0].resets_at, None);
        assert_eq!(q.extra, None);
        let q = summary_quota(&json!({"individualUsage": {"plan": {"used": 5}}}), "");
        assert_eq!(q.windows[0].used_percent, Some(0.0));
        assert!(summary_quota(&json!({}), "").windows.is_empty());
    }

    #[test]
    fn old_plan_mapping() {
        let body = br#"{"gpt-4":{"numRequests":150,"numRequestsTotal":160,"maxRequestUsage":500},
                         "gpt-3.5-turbo":{"numRequests":3,"maxRequestUsage":null},
                         "gpt-4-32k":{"numRequests":0,"maxRequestUsage":50},
                         "startOfMonth":"2025-01-31T10:00:00.000Z"}"#;
        let q = old_quota(&entries_of(body).unwrap(), "pro", &Utc);
        assert_eq!(q.plan, "pro");
        let ids: Vec<&str> = q.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["gpt-4", "gpt-4-32k"]);
        assert_eq!(q.windows[0].name.as_deref(), Some("gpt-4"));
        assert_eq!(q.windows[0].used_percent, Some(30.0));
        // Jan 31 + 1 month overflows like JS setMonth: Feb 31 → Mar 3 (2025 is not a leap year).
        let mar3 = Utc.with_ymd_and_hms(2025, 3, 3, 10, 0, 0).unwrap().timestamp_millis();
        assert_eq!(q.windows[0].resets_at, Some(mar3));
        assert_eq!(q.windows[1].used_percent, Some(0.0));
        assert!(old_quota(&entries_of(br#"{"gpt-4":{"numRequests":1,"maxRequestUsage":2}}"#).unwrap(), "", &Utc).windows[0].resets_at.is_none());
    }

    #[test]
    fn add_month_is_local() {
        // 2025-12-15 20:00 UTC is 2025-12-16 03:00 at +07:00 → 2026-01-16 03:00 +07:00.
        let tz = FixedOffset::east_opt(7 * 3600).unwrap();
        let start = Utc.with_ymd_and_hms(2025, 12, 15, 20, 0, 0).unwrap().timestamp_millis();
        let want = tz.with_ymd_and_hms(2026, 1, 16, 3, 0, 0).unwrap().timestamp_millis();
        assert_eq!(add_month(start, &tz), Some(want));
        let start = Utc.with_ymd_and_hms(2024, 1, 30, 0, 0, 0).unwrap().timestamp_millis();
        assert_eq!(add_month(start, &Utc), Some(Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap().timestamp_millis()));
        // The real local zone works too.
        assert!(add_month(start, &chrono::Local).is_some());
    }

}
