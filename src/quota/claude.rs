//! Claude (claude.ai subscription) usage, with the login Claude Code keeps in `~/.claude/.credentials.json`
//! (or the macOS Keychain).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::{entries_of, entry, qerr, read_json, truthy, FetchErr, Res};
use crate::http;
use crate::model::{now_ms, Extra, QErr, Quota, Win};

/// Public OAuth client of Claude Code; used only to refresh the user's own token.
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_URLS: [&str; 2] = ["https://platform.claude.com/v1/oauth/token", "https://console.anthropic.com/v1/oauth/token"];
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    File,
    Keychain,
}

/// The `claudeAiOauth` object (Null when absent) and where it came from.
fn read_oauth(creds: &Path) -> Res<(Value, Source)> {
    if creds.exists() {
        let v = read_json(creds)?;
        return Ok((v.get("claudeAiOauth").cloned().unwrap_or(Value::Null), Source::File));
    }
    #[cfg(target_os = "macos")]
    if let Some(raw) = keychain() {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            return Ok((v.get("claudeAiOauth").cloned().unwrap_or(Value::Null), Source::Keychain));
        }
    }
    Err(FetchErr::NotLoggedIn)
}

#[cfg(target_os = "macos")]
fn keychain() -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => {
                let mut s = String::new();
                child.stdout.take()?.read_to_string(&mut s).ok()?;
                return Some(s);
            }
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn access_token(creds: &Path, force: bool) -> Res<String> {
    let (oauth, source) = read_oauth(creds)?;
    let token = match oauth.get("accessToken").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return Err(FetchErr::NotLoggedIn),
    };
    let expires_at = super::js_number(oauth.get("expiresAt"));
    if !force && expires_at.is_some_and(|e| e - now_ms() as f64 > 60_000.0) {
        return Ok(token);
    }
    // Refreshing rotates the refresh token; only do it where the new one can be written back for the CLI.
    if source != Source::File {
        return Err(qerr("claudeExpiredKeychain"));
    }
    refresh(creds, &oauth)
}

fn refresh(creds: &Path, oauth: &Value) -> Res<String> {
    let mut body = Map::new();
    body.insert("grant_type".into(), json!("refresh_token"));
    if let Some(rt) = oauth.get("refreshToken") {
        body.insert("refresh_token".into(), rt.clone());
    }
    body.insert("client_id".into(), json!(CLIENT_ID));
    let body = Value::Object(body);

    let mut last_status = 0u16;
    for url in TOKEN_URLS {
        let Ok(res) = http::post_json(url, &[], &body) else { continue };
        last_status = res.status;
        if !res.ok() {
            continue;
        }
        let t: Value = res.json()?;
        return write_back(creds, oauth.get("refreshToken"), &t, now_ms());
    }
    Err(if last_status != 0 {
        FetchErr::Quota(QErr { code: "claudeExpired".into(), status: Some(last_status.to_string()), ..QErr::default() })
    } else {
        FetchErr::Offline
    })
}

/// Stores a refresh response `t` and returns the access token to use. Re-reads the file first, so a refresh
/// done meanwhile by the CLI (its refresh token no longer `old_refresh`) is not overwritten: then the file's
/// current access token is used instead.
fn write_back(creds: &Path, old_refresh: Option<&Value>, t: &Value, now: i64) -> Res<String> {
    let mut current = read_json(creds)?;
    let cur = current.get("claudeAiOauth");
    if cur.and_then(|o| o.get("refreshToken")) != old_refresh {
        return cur
            .and_then(|o| o.get("accessToken"))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| FetchErr::Unexpected("Claude credentials changed without an access token".into()));
    }
    let Some(access) = t.get("access_token").and_then(Value::as_str) else {
        return Err(FetchErr::Unexpected("token refresh returned no access_token".into()));
    };
    let Some(root) = current.as_object_mut() else {
        return Err(FetchErr::Unexpected("Claude credentials are not a JSON object".into()));
    };
    let mut oauth = match root.get("claudeAiOauth") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    oauth.insert("accessToken".into(), json!(access));
    let refresh = match t.get("refresh_token") {
        Some(v) if !v.is_null() => Some(v.clone()),
        _ => old_refresh.cloned(),
    };
    match refresh {
        Some(r) => oauth.insert("refreshToken".into(), r),
        None => oauth.remove("refreshToken"),
    };
    let expires_in = t.get("expires_in").and_then(Value::as_f64).unwrap_or(3600.0);
    let expires_at = now as f64 + expires_in * 1000.0;
    let expires_at = if expires_at.fract() == 0.0 && expires_at.abs() < 9e15 { json!(expires_at as i64) } else { json!(expires_at) };
    oauth.insert("expiresAt".into(), expires_at);
    root.insert("claudeAiOauth".into(), Value::Object(oauth));
    write_json_atomic(creds, &current)?;
    Ok(access.to_string())
}

fn tmp_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".quota-overlay.tmp");
    PathBuf::from(s)
}

/// Writes `<file>.quota-overlay.tmp` (mode 0600 on unix) and renames it over `file`.
fn write_json_atomic(file: &Path, data: &Value) -> std::io::Result<()> {
    let tmp = tmp_path(file);
    let result = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(&serde_json::to_vec(data)?)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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

pub(super) fn fetch(creds: &Path) -> Res<Quota> {
    let plan = read_oauth(creds)?.0.get("subscriptionType").and_then(Value::as_str).unwrap_or("").to_string();
    let call = |token: String| {
        let auth = format!("Bearer {token}");
        http::get(USAGE_URL, &[("Authorization", auth.as_str()), ("anthropic-beta", "oauth-2025-04-20")])
    };

    let mut res = call(access_token(creds, false)?)?;
    if res.status == 401 {
        res = call(access_token(creds, true)?)?;
    }
    if res.status == 429 {
        return Err(qerr("rateLimited"));
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

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aqo-claude-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
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

    #[test]
    fn atomic_write_replaces_file() {
        let d = scratch("atomic");
        let f = d.join(".credentials.json");
        std::fs::write(&f, b"{\"old\":true}").unwrap();
        write_json_atomic(&f, &json!({"new": 1})).unwrap();
        assert_eq!(read_json(&f).unwrap(), json!({"new": 1}));
        assert!(!tmp_path(&f).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_back_stores_new_tokens() {
        let d = scratch("store");
        let f = d.join(".credentials.json");
        let creds = json!({
            "claudeAiOauth": {"accessToken": "a-old", "refreshToken": "r-old", "expiresAt": 1, "scopes": ["x"], "subscriptionType": "max"},
            "other": {"keep": true}
        });
        std::fs::write(&f, serde_json::to_vec(&creds).unwrap()).unwrap();
        let t = json!({"access_token": "a-new", "refresh_token": "r-new", "expires_in": 28800});
        let got = write_back(&f, Some(&json!("r-old")), &t, 1_000_000).unwrap();
        assert_eq!(got, "a-new");
        let now = read_json(&f).unwrap();
        assert_eq!(now["claudeAiOauth"]["accessToken"], "a-new");
        assert_eq!(now["claudeAiOauth"]["refreshToken"], "r-new");
        assert_eq!(now["claudeAiOauth"]["expiresAt"], json!(1_000_000 + 28_800_000));
        assert_eq!(now["claudeAiOauth"]["scopes"], json!(["x"]));
        assert_eq!(now["claudeAiOauth"]["subscriptionType"], "max");
        assert_eq!(now["other"], json!({"keep": true}));
        assert!(!tmp_path(&f).exists());

        // No new refresh token / expiry in the response: keep the old one, default to an hour.
        let t = json!({"access_token": "a-3"});
        assert_eq!(write_back(&f, Some(&json!("r-new")), &t, 5).unwrap(), "a-3");
        let now = read_json(&f).unwrap();
        assert_eq!(now["claudeAiOauth"]["refreshToken"], "r-new");
        assert_eq!(now["claudeAiOauth"]["expiresAt"], json!(5 + 3_600_000));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_back_defers_to_a_newer_cli_refresh() {
        let d = scratch("defer");
        let f = d.join(".credentials.json");
        // The CLI refreshed meanwhile: the file's refresh token is no longer the one we used.
        let creds = br#"{"claudeAiOauth":{"accessToken":"a-cli","refreshToken":"r-cli","expiresAt":99}}"#;
        std::fs::write(&f, creds).unwrap();
        let t = json!({"access_token": "a-ours", "refresh_token": "r-ours", "expires_in": 3600});
        assert_eq!(write_back(&f, Some(&json!("r-old")), &t, 0).unwrap(), "a-cli");
        assert_eq!(std::fs::read(&f).unwrap(), creds.to_vec(), "file must be untouched");
        assert!(!tmp_path(&f).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn read_oauth_and_expiry_without_network() {
        let d = scratch("oauth");
        let f = d.join(".credentials.json");
        assert_eq!(read_oauth(&f).unwrap_err(), FetchErr::NotLoggedIn);
        std::fs::write(&f, br#"{"other":1}"#).unwrap();
        assert_eq!(access_token(&f, false).unwrap_err(), FetchErr::NotLoggedIn);
        let far = now_ms() + 3_600_000;
        std::fs::write(&f, format!(r#"{{"claudeAiOauth":{{"accessToken":"a","refreshToken":"r","expiresAt":{far}}}}}"#)).unwrap();
        // Still valid for more than a minute: no refresh, no HTTP.
        assert_eq!(access_token(&f, false).unwrap(), "a");
        std::fs::write(&f, b"{not json").unwrap();
        assert!(matches!(read_oauth(&f), Err(FetchErr::Unexpected(_))));
        let _ = std::fs::remove_dir_all(&d);
    }
}
