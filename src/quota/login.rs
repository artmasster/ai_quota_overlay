//! Signing in to an account from this app, in the browser, the way each service's own app does it: Claude and
//! Codex send the browser back to a one-off server on localhost with a code (OAuth with PKCE), Cursor's page is
//! asked about until it says yes, and GitHub (Copilot) shows a code to type on its device page. The new account
//! goes into accounts.json (`store`) and `notify` hears about it.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::store::{self, Saved};
use super::{b64url, claude, codex, copilot, cursor, describe, random, FetchErr, Res};
use crate::i18n::{self, Lang};
use crate::model::Svc;

/// How long a sign-in waits for the browser.
const WAIT: Duration = Duration::from_secs(10 * 60);
const CURSOR_POLL: Duration = Duration::from_millis(1500);

/// A sign-in under way; dropping it gives up on it.
pub struct Pending {
    pub svc: Svc,
    /// The page that was opened in the browser (to open again, or copy into another browser).
    pub url: String,
    cancel: Arc<AtomicBool>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// What became of a sign-in: the account's id, or why not (None: given up on here, nothing to tell).
pub type Outcome = Result<String, Option<String>>;

/// What a sign-in tells the app, from another thread.
pub enum Event {
    /// The code to type on the page (GitHub's device sign-in).
    Code(String),
    Done(Outcome),
}

/// Starts signing in to `svc`; the caller opens `Pending::url` in the browser. `notify` hears `Event::Done` once
/// at the end (and before that, for Copilot, the code).
pub fn start(svc: Svc, lang: Lang, notify: impl Fn(Event) + Send + 'static) -> Result<Pending, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let (verifier, challenge) = pkce();
    let state = b64url(&random(24));
    let until = Instant::now() + WAIT;
    let url = match svc {
        Svc::Claude => {
            let (lb, port) = Loopback::bind(0).map_err(|e| e.to_string())?;
            let redirect = format!("http://localhost:{port}/callback");
            let url = claude::authorize_url(&redirect, &challenge, &state);
            std::thread::spawn(move || {
                let o = callback(&lb, "/callback", &state, &c, until, lang, svc, |code| claude::sign_in(code, &redirect, &verifier, &state));
                notify(Event::Done(o))
            });
            url
        }
        Svc::Codex => {
            // A sign-in given up on a moment ago lets go of the port within a tenth of a second.
            let mut bound = Loopback::bind(codex::PORT);
            for _ in 0..10 {
                if bound.is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
                bound = Loopback::bind(codex::PORT);
            }
            let (lb, _) = bound.map_err(|_| i18n::login_port_busy(lang, codex::PORT))?;
            let url = codex::authorize_url(&challenge, &state);
            std::thread::spawn(move || {
                let o = callback(&lb, "/auth/callback", &state, &c, until, lang, svc, |code| codex::sign_in(code, &verifier));
                notify(Event::Done(o))
            });
            url
        }
        Svc::Cursor => {
            let uuid = uuid4();
            let url = cursor::login_url(&challenge, &uuid);
            std::thread::spawn(move || notify(Event::Done(poll_cursor(&uuid, &verifier, &c, until, lang))));
            url
        }
        Svc::Copilot => {
            // The page asks for the code, which comes a moment later (shown in the app and put on the clipboard).
            std::thread::spawn(move || {
                let o = poll_copilot(&notify, &c, lang);
                notify(Event::Done(o))
            });
            copilot::VERIFY_URL.to_string()
        }
    };
    Ok(Pending { svc, url, cancel })
}

/// Sleeps `d` in small steps; false when the sign-in was given up on meanwhile.
fn wait(d: Duration, cancel: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !cancel.load(Ordering::Relaxed)
}

/// GitHub's device sign-in: get a code, show it, then ask until the user has typed it on github.com.
fn poll_copilot(notify: &dyn Fn(Event), cancel: &AtomicBool, lang: Lang) -> Outcome {
    let d = copilot::device_code().map_err(|e| Some(describe(&e, lang)))?;
    notify(Event::Code(d.user_code.clone()));
    let until = Instant::now() + Duration::from_secs(d.expires_in);
    let mut every = Duration::from_secs(d.interval.max(1));
    while Instant::now() < until {
        if !wait(every, cancel) {
            return Err(None);
        }
        match copilot::poll(&d.device_code) {
            Ok(copilot::Poll::Wait) | Err(FetchErr::Offline) => {}
            Ok(copilot::Poll::Slower(s)) => every = Duration::from_secs(s.max(every.as_secs() + 5)),
            Ok(copilot::Poll::Token(t)) => return store_it(copilot::sign_in(&t), lang).map_err(Some),
            Ok(copilot::Poll::Failed(why)) => return Err(Some(why)),
            Err(e) => return Err(Some(describe(&e, lang))),
        }
    }
    Err(Some(i18n::login_timeout(lang).to_string()))
}

/// PKCE: a random verifier and its S256 challenge.
fn pkce() -> (String, String) {
    let verifier = b64url(&random(32));
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn uuid4() -> String {
    let mut b = random(16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn store_it(s: Res<Saved>, lang: Lang) -> Result<String, String> {
    s.map_err(|e| describe(&e, lang)).and_then(|s| store::add(s).map_err(|e| e.to_string()))
}

/// Waits for the browser to come back to `path` with a code, turns it into an account with `exchange`, stores it
/// and shows the browser how it went.
#[allow(clippy::too_many_arguments)]
fn callback(lb: &Loopback, path: &str, state: &str, cancel: &AtomicBool, until: Instant, lang: Lang, svc: Svc, exchange: impl FnOnce(&str) -> Res<Saved>) -> Outcome {
    let Some((stream, q)) = lb.wait(path, cancel, until) else {
        return Err((!cancel.load(Ordering::Relaxed)).then(|| i18n::login_timeout(lang).to_string()));
    };
    let get = |k: &str| q.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str());
    let result = if get("state") != Some(state) {
        Err(i18n::login_state(lang).to_string())
    } else if let Some(e) = get("error") {
        Err(get("error_description").unwrap_or(e).to_string())
    } else if let Some(code) = get("code").filter(|c| !c.is_empty()) {
        store_it(exchange(code), lang)
    } else {
        Err("no code".into())
    };
    let (title, text) = i18n::login_page(lang, svc, result.as_ref().err().map(String::as_str));
    respond(stream, &title, &text);
    result.map_err(Some)
}

fn poll_cursor(uuid: &str, verifier: &str, cancel: &AtomicBool, until: Instant, lang: Lang) -> Outcome {
    while Instant::now() < until {
        if !wait(CURSOR_POLL, cancel) {
            return Err(None);
        }
        match cursor::poll(uuid, verifier) {
            Ok(Some(s)) => return store_it(Ok(s), lang).map_err(Some),
            Ok(None) | Err(FetchErr::Offline) => {}
            Err(e) => return Err(Some(describe(&e, lang))),
        }
    }
    Err(Some(i18n::login_timeout(lang).to_string()))
}

/// The one-off server the browser comes back to.
struct Loopback(Vec<TcpListener>);

impl Loopback {
    /// 127.0.0.1:`port` (0: any free port) and, where it can, [::1] on the same port, since the browser may take
    /// "localhost" in the redirect for either.
    fn bind(port: u16) -> std::io::Result<(Self, u16)> {
        let v4 = TcpListener::bind(("127.0.0.1", port))?;
        let port = v4.local_addr()?.port();
        let mut ls = vec![v4];
        if let Ok(v6) = TcpListener::bind(("::1", port)) {
            ls.push(v6);
        }
        for l in &ls {
            l.set_nonblocking(true)?;
        }
        Ok((Self(ls), port))
    }

    /// The browser's request for `path` and its query; anything else (a favicon) gets a 404.
    fn wait(&self, path: &str, cancel: &AtomicBool, until: Instant) -> Option<(TcpStream, Vec<(String, String)>)> {
        while !cancel.load(Ordering::Relaxed) && Instant::now() < until {
            let mut any = false;
            for l in &self.0 {
                if let Ok((s, _)) = l.accept() {
                    any = true;
                    if let Some(r) = read_request(s, path) {
                        return Some(r);
                    }
                }
            }
            if !any {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        None
    }
}

fn read_request(mut s: TcpStream, path: &str) -> Option<(TcpStream, Vec<(String, String)>)> {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 32 * 1024 {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    // "GET /callback?code=…&state=… HTTP/1.1"
    let target = text.lines().next()?.split(' ').nth(1)?;
    let (p, q) = target.split_once('?').unwrap_or((target, ""));
    if p != path {
        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return None;
    }
    Some((s, parse_query(q)))
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (pct_decode(k), pct_decode(v))
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'+', ..) => out.push(b' '),
            (b'%', Some(h), Some(l)) => {
                out.push(h << 4 | l);
                i += 2;
            }
            (c, ..) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A small dark page in the app's colours saying how the sign-in went.
fn respond(mut s: TcpStream, title: &str, text: &str) {
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Ai Quota Overlay</title><style>\
         html,body{{height:100%;margin:0}}body{{background:#111318;color:#eceef2;font:15px/1.5 system-ui,-apple-system,'Segoe UI',sans-serif;\
         display:flex;align-items:center;justify-content:center}}div{{max-width:440px;padding:32px;text-align:center}}\
         h1{{font-size:20px;font-weight:600;margin:0 0 8px}}p{{color:#8b90a0;margin:0}}</style></head>\
         <body><div><h1>{}</h1><p>{}</p></div></body></html>",
        esc(title),
        esc(text)
    );
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", html.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(html.as_bytes());
    let _ = s.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_ids() {
        let q = parse_query("code=ab%2Fc%3D&state=x+y&empty=&flag");
        assert_eq!(q, [("code".into(), "ab/c=".into()), ("state".into(), "x y".into()), ("empty".into(), String::new()), ("flag".into(), String::new())]);
        assert_eq!(pct_decode("100%"), "100%");
        assert_eq!(pct_decode("%zz"), "%zz");
        assert_eq!(pct_decode("%e0%b8%81"), "ก");
        let u = uuid4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"), "{u}");
        let (v, c) = pkce();
        assert_eq!(v.len(), 43);
        assert_eq!(c, b64url(&Sha256::digest(v.as_bytes())));
    }

    #[test]
    fn loopback_takes_the_callback() {
        let (lb, port) = Loopback::bind(0).unwrap();
        let t = std::thread::spawn(move || {
            // A stray request first, then the real one.
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut r = String::new();
            let _ = s.read_to_string(&mut r);
            assert!(r.starts_with("HTTP/1.1 404"), "{r}");
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=c0de&state=st HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut r = String::new();
            let _ = s.read_to_string(&mut r);
            r
        });
        let cancel = AtomicBool::new(false);
        let (stream, q) = lb.wait("/callback", &cancel, Instant::now() + Duration::from_secs(10)).unwrap();
        assert_eq!(q[0], ("code".into(), "c0de".into()));
        respond(stream, "Signed in <ok>", "Close this tab");
        let page = t.join().unwrap();
        assert!(page.starts_with("HTTP/1.1 200 OK") && page.contains("Signed in &lt;ok&gt;"), "{page}");
        // Cancelled: gives up at once.
        cancel.store(true, Ordering::Relaxed);
        assert!(lb.wait("/callback", &cancel, Instant::now() + Duration::from_secs(10)).is_none());
    }
}
