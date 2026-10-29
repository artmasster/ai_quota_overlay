//! accounts.json next to settings.json: the accounts signed in from this app and their tokens, kept apart from the
//! services' own apps so a service can have any number of them. Written atomically (owner-only on unix) and only
//! under LOCK, since checks refresh tokens from several threads while a sign-in may be adding one.
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::model::{Acct, Svc};

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    pub svc: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub plan: String,
    pub access: String,
    #[serde(default)]
    pub refresh: String,
    /// When the access token runs out (unix ms); 0 = unknown.
    #[serde(default)]
    pub expires_at: i64,
    /// Codex: the ChatGPT account id (a request header). Cursor: the user id (part of the session cookie).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account: String,
}

impl Saved {
    pub fn acct(&self) -> Option<Acct> {
        Some(Acct { id: self.id.clone(), svc: Svc::from_key(&self.svc)?, who: self.email.clone() })
    }
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    accounts: Vec<Saved>,
}

fn path() -> PathBuf {
    crate::settings::data_dir().join("accounts.json")
}

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn read() -> Vec<Saved> {
    std::fs::read(path()).ok().and_then(|b| serde_json::from_slice::<File>(&b).ok()).map(|f| f.accounts).unwrap_or_default()
}

fn write(list: &[Saved]) -> std::io::Result<()> {
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    super::write_json_atomic(&p, &serde_json::to_value(File { accounts: list.to_vec() })?)
}

pub fn list() -> Vec<Saved> {
    let _g = lock();
    read()
}

pub fn get(id: &str) -> Option<Saved> {
    list().into_iter().find(|s| s.id == id)
}

/// Stores a new sign-in and returns its id. Signing in again to an account that is already here (same service and
/// email) replaces its tokens and keeps its id, so its settings stay.
pub fn add(mut s: Saved) -> std::io::Result<String> {
    let _g = lock();
    let mut list = read();
    let same = |o: &Saved| o.svc == s.svc && !s.email.is_empty() && o.email.eq_ignore_ascii_case(&s.email);
    if let Some(old) = list.iter_mut().find(|o| same(o)) {
        s.id = old.id.clone();
        *old = s.clone();
    } else {
        s.id = new_id(&s.svc, &list);
        list.push(s.clone());
    }
    write(&list)?;
    Ok(s.id)
}

/// Changes an account in place (after a token refresh) and returns it as stored; None once it was removed.
pub fn update(id: &str, f: impl FnOnce(&mut Saved)) -> std::io::Result<Option<Saved>> {
    let _g = lock();
    let mut list = read();
    let Some(s) = list.iter_mut().find(|s| s.id == id) else { return Ok(None) };
    f(s);
    let out = s.clone();
    write(&list)?;
    Ok(Some(out))
}

/// Moves an account to place `to` in the list (the order the app shows them in).
pub fn move_to(id: &str, to: usize) -> std::io::Result<()> {
    let _g = lock();
    let mut list = read();
    let Some(i) = list.iter().position(|s| s.id == id) else { return Ok(()) };
    let j = to.min(list.len() - 1);
    if i != j {
        let s = list.remove(i);
        list.insert(j, s);
        write(&list)?;
    }
    Ok(())
}

pub fn remove(id: &str) -> std::io::Result<()> {
    let _g = lock();
    let mut list = read();
    let n = list.len();
    list.retain(|s| s.id != id);
    if list.len() != n {
        write(&list)?;
    }
    Ok(())
}

/// "claude-3f9a2c": the service key and six random hex digits, unused so far.
fn new_id(svc: &str, list: &[Saved]) -> String {
    loop {
        let r = super::random(3);
        let id = format!("{svc}-{:02x}{:02x}{:02x}", r[0], r[1], r[2]);
        if !list.iter().any(|s| s.id == id) {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All in one test: the data folder is process-wide (QUOTA_OVERLAY_DATA_DIR).
    #[test]
    fn add_update_remove() {
        let d = std::env::temp_dir().join(format!("aqo-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::env::set_var("QUOTA_OVERLAY_DATA_DIR", &d);
        assert!(list().is_empty());
        let a = Saved { svc: "claude".into(), email: "a@x.com".into(), access: "t1".into(), ..Saved::default() };
        let id = add(a.clone()).unwrap();
        assert!(id.starts_with("claude-") && id.len() == "claude-".len() + 6, "{id}");
        let id2 = add(Saved { email: "b@x.com".into(), ..a.clone() }).unwrap();
        assert_ne!(id, id2);
        // The same account again (email in another case): same id, new tokens.
        assert_eq!(add(Saved { email: "A@x.com".into(), access: "t2".into(), ..a.clone() }).unwrap(), id);
        assert_eq!(list().len(), 2);
        assert_eq!(get(&id).unwrap().access, "t2");
        let got = update(&id, |s| s.access = "t3".into()).unwrap().unwrap();
        assert_eq!(got.access, "t3");
        assert_eq!(get(&id).unwrap().acct().unwrap(), Acct { id: id.clone(), svc: Svc::Claude, who: "A@x.com".into() });
        // Order: moved by the user, kept within the list.
        let id3 = add(Saved { svc: "codex".into(), email: "c@x.com".into(), ..a.clone() }).unwrap();
        let ids = || list().into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(), [id.clone(), id2.clone(), id3.clone()]);
        move_to(&id3, 1).unwrap();
        assert_eq!(ids(), [id.clone(), id3.clone(), id2.clone()]);
        move_to(&id3, 0).unwrap();
        assert_eq!(ids(), [id3.clone(), id.clone(), id2.clone()]);
        move_to(&id3, 9).unwrap();
        assert_eq!(ids(), [id.clone(), id2.clone(), id3.clone()]);
        move_to(&id3, 0).unwrap();
        remove(&id3).unwrap();
        remove(&id).unwrap();
        assert_eq!(update(&id, |_| {}).unwrap(), None);
        assert_eq!(list().len(), 1);
        std::env::remove_var("QUOTA_OVERLAY_DATA_DIR");
        let _ = std::fs::remove_dir_all(&d);
    }
}
