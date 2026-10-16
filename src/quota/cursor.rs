//! Cursor usage, with the login Cursor keeps in its `state.vscdb` (SQLite, read with crate::sqlite).

use std::path::Path;

use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use chrono::{Datelike, Days, LocalResult, NaiveDate, TimeZone};
use serde_json::Value;

use super::{entries_of, entry, js_number, parse_date_ms, qerr, truthy, FetchErr, Res};
use crate::model::{now_ms, Extra, QErr, Quota, Win};
use crate::{http, sqlite};

const TOKEN_KEY: &str = "cursorAuth/accessToken";
const PLAN_KEY: &str = "cursorAuth/stripeMembershipType";
const SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
const OLD_USAGE_URL: &str = "https://cursor.com/api/usage?user=";

fn read_auth(db: &Path) -> Res<(String, String)> {
    if !db.exists() {
        return Err(FetchErr::NotLoggedIn);
    }
    let m = sqlite::read_item_table(db, &[TOKEN_KEY, PLAN_KEY])?;
    let token = match m.get(TOKEN_KEY) {
        Some(t) if !t.is_empty() => String::from_utf8_lossy(t).into_owned(),
        _ => return Err(FetchErr::NotLoggedIn),
    };
    let plan = m.get(PLAN_KEY).map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default();
    Ok((token, plan))
}

/// The JSON payload of a JWT (base64url, padding optional, like Node's `base64url` decoding).
fn jwt_payload(token: &str) -> Option<Value> {
    const ENGINE: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::URL_SAFE,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let part = token.split('.').nth(1)?;
    let cleaned: String = part
        .chars()
        .filter_map(|c| match c {
            '+' => Some('-'),
            '/' => Some('_'),
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect();
    let bytes = ENGINE.decode(cleaned).ok()?;
    serde_json::from_str(&String::from_utf8_lossy(&bytes)).ok()
}

/// `String(payload.sub).split('|').pop()`.
fn user_id(payload: &Value) -> String {
    let sub = match payload.get("sub") {
        None => "undefined".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    };
    sub.rsplit('|').next().unwrap_or_default().to_string()
}

/// JS `encodeURIComponent`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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

pub(super) fn fetch(db: &Path) -> Res<Quota> {
    let (token, plan) = read_auth(db)?;
    let payload = jwt_payload(&token).ok_or_else(|| FetchErr::Unexpected("could not decode the Cursor access token".into()))?;
    if truthy(payload.get("exp")) && js_number(payload.get("exp")).is_some_and(|e| e * 1000.0 < now_ms() as f64) {
        return Err(qerr("cursorExpired"));
    }
    let user = user_id(&payload);
    let cookie = format!("WorkosCursorSessionToken={user}%3A%3A{token}");
    let headers = [("Cookie", cookie.as_str()), ("Origin", "https://cursor.com")];

    // Current (usage-based) plans.
    let res = http::get(SUMMARY_URL, &headers)?;
    if res.status == 401 || res.status == 403 {
        return Err(qerr("cursorExpired"));
    }
    if res.ok() {
        return Ok(summary_quota(&res.json()?, &plan));
    }

    // Older request-count plans.
    let old = http::get(&format!("{OLD_USAGE_URL}{}", encode_uri_component(&user)), &headers)?;
    if !old.ok() {
        return Err(FetchErr::Quota(QErr::status("Cursor", format!("{}/{}", res.status, old.status))));
    }
    Ok(old_quota(&entries_of(&old.body)?, &plan, &chrono::Local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
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

    #[test]
    fn reads_auth_from_fixture_db() {
        let db = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small.db");
        let (token, plan) = read_auth(&db).unwrap();
        assert_eq!(plan, "pro");
        let payload = jwt_payload(&token).unwrap();
        assert_eq!(user_id(&payload), "user_fixture");
        let missing = std::env::temp_dir().join(format!("aqo-cursor-missing-{}.vscdb", std::process::id()));
        assert_eq!(read_auth(&missing).unwrap_err(), FetchErr::NotLoggedIn);
    }
}
