//! Updates from GitHub Releases (port of src/updater.js). Reads the electron-builder style `latest.yml`
//! (`latest-linux.yml` on Linux) of the latest release, so the feed format stays the same for the Electron builds
//! out there. Nothing downloads on its own: a newer version is offered in the toast, and Update downloads it,
//! checks size and SHA-512, installs it silently and restarts. Skip remembers the version.
//!
//! macOS builds aren't signed, and an unsigned app can't replace itself there, so on macOS the check reads the
//! latest release tag and Update opens the release page. A build that wasn't installed checks nothing, unless
//! QUOTA_OVERLAY_FAKE_UPDATE=1.9.0 is set: that pretends 1.9.0 is out and plays the whole flow (download progress
//! included) without installing anything.
use crate::app::{Msg, Waker};
use base64::Engine;
use sha2::{Digest, Sha512};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

pub const CHECK_DELAY: Duration = Duration::from_secs(5);
pub const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);
pub const STALE: Duration = Duration::from_secs(2 * 60);
const REPO: &str = "artmasster/ai_quota_overlay";
pub const RELEASES_URL: &str = "https://github.com/artmasster/ai_quota_overlay/releases/latest";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Checking,
    Latest,
    Available,
    Downloading,
    Installing,
    Error,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum During {
    Check,
    Download,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub file: String,
    pub sha512: String,
    pub size: Option<u64>,
}

pub enum UpMsg {
    Checked(Result<Option<Release>, String>),
    Progress(u32),
    Downloaded(Result<PathBuf, String>),
}

/// What the toast and the app window show.
#[derive(Clone, Debug)]
pub struct View {
    pub phase: Phase,
    pub current: String,
    pub version: Option<String>,
    pub percent: u32,
    pub manual: bool,
    pub error: Option<(During, String)>,
    pub skipped: bool,
    pub dismissed: bool,
}

pub struct Updater {
    phase: Phase,
    current: String,
    release: Option<Release>,
    percent: u32,
    manual: bool,
    error: Option<(During, String)>,
    dismissed: Option<String>,
    fake: Option<String>,
    last_check: Option<Instant>,
    pub next_check: Option<Instant>,
    /// Fake download ticks.
    fake_tick: Option<Instant>,
    /// When the install step began (the app quits 1.2 s later).
    install_at: Option<Instant>,
    installer: Option<PathBuf>,
    tx: Sender<Msg>,
    waker: Waker,
}

/// "1.10.0" > "1.9.2"; anything after a "-" (pre-release tags) is ignored.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split('-').next().unwrap_or("").split('.').map(|n| n.trim().parse().unwrap_or(0)).collect() };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

/// Whether this copy was installed (and so may replace itself).
pub fn installed() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    if cfg!(windows) {
        exe.parent().is_some_and(|d| d.join(crate::platform::UNINSTALLER).exists())
    } else if cfg!(target_os = "macos") {
        exe.to_string_lossy().contains(".app/Contents/MacOS/")
    } else {
        std::env::var_os("APPIMAGE").is_some()
    }
}

impl Updater {
    pub fn new(tx: Sender<Msg>, waker: Waker) -> Self {
        let fake = std::env::var("QUOTA_OVERLAY_FAKE_UPDATE").ok().filter(|v| !v.is_empty());
        let manual = cfg!(target_os = "macos");
        let phase = if fake.is_none() && !installed() { Phase::Unsupported } else { Phase::Idle };
        let first = if phase == Phase::Unsupported {
            None
        } else {
            Some(Instant::now() + if fake.is_some() { Duration::from_secs(2) } else { CHECK_DELAY })
        };
        Self {
            phase,
            current: env!("CARGO_PKG_VERSION").to_string(),
            release: None,
            percent: 0,
            manual,
            error: None,
            dismissed: None,
            fake,
            last_check: None,
            next_check: first,
            fake_tick: None,
            install_at: None,
            installer: None,
            tx,
            waker,
        }
    }

    pub fn view(&self, skip_version: Option<&str>) -> View {
        let version = self.release.as_ref().map(|r| r.version.clone());
        View {
            phase: self.phase,
            current: self.current.clone(),
            skipped: version.is_some() && version.as_deref() == skip_version,
            dismissed: version.is_some() && version == self.dismissed,
            version,
            percent: self.percent,
            manual: self.manual,
            error: self.error.clone(),
        }
    }

    fn send(&self, m: UpMsg) {
        let _ = self.tx.send(Msg::Update(m));
        (self.waker)();
    }

    pub fn check(&mut self) {
        if matches!(self.phase, Phase::Unsupported | Phase::Checking | Phase::Downloading | Phase::Installing) {
            return;
        }
        self.last_check = Some(Instant::now());
        // A re-check while an update is on offer leaves the toast alone instead of making it blink.
        if self.phase != Phase::Available {
            self.phase = Phase::Checking;
            self.error = None;
        }
        let tx = self.tx.clone();
        let waker = self.waker.clone();
        let fake = self.fake.clone();
        let manual = self.manual;
        let current = self.current.clone();
        std::thread::spawn(move || {
            let r = if let Some(v) = fake {
                std::thread::sleep(Duration::from_millis(800));
                Ok(Some(Release { version: v, file: String::new(), sha512: String::new(), size: None }))
            } else if manual {
                latest_tag().map(|v| newer(&v, &current).then(|| Release { version: v, file: String::new(), sha512: String::new(), size: None }))
            } else {
                latest_yml().map(|r| newer(&r.version, &current).then_some(r))
            };
            let _ = tx.send(Msg::Update(UpMsg::Checked(r)));
            waker();
        });
    }

    pub fn check_if_stale(&mut self, max_age: Duration) {
        if self.last_check.is_none_or(|t| t.elapsed() >= max_age) {
            self.check();
        }
    }

    pub fn download(&mut self, open_url: &mut dyn FnMut(&str)) {
        let Some(rel) = self.release.clone() else { return };
        if matches!(self.phase, Phase::Downloading | Phase::Installing) {
            return;
        }
        if self.manual {
            open_url(RELEASES_URL);
            return;
        }
        self.phase = Phase::Downloading;
        self.percent = 0;
        self.error = None;
        if self.fake.is_some() {
            self.fake_tick = Some(Instant::now());
            return;
        }
        let tx = self.tx.clone();
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let r = download(&rel, &mut |p| {
                let _ = tx.send(Msg::Update(UpMsg::Progress(p)));
                waker();
            });
            let _ = tx.send(Msg::Update(UpMsg::Downloaded(r)));
            waker();
        });
    }

    pub fn skip(&mut self, set_skipped: &mut dyn FnMut(String)) {
        if let Some(r) = &self.release {
            set_skipped(r.version.clone());
        }
    }

    pub fn dismiss(&mut self) {
        self.dismissed = self.release.as_ref().map(|r| r.version.clone());
        if self.phase == Phase::Error {
            self.phase = if self.release.is_some() { Phase::Available } else { Phase::Idle };
            self.error = None;
        }
    }

    pub fn on_msg(&mut self, m: UpMsg) {
        match m {
            UpMsg::Checked(Ok(Some(rel))) => {
                self.release = Some(rel);
                self.percent = 0;
                self.phase = Phase::Available;
            }
            UpMsg::Checked(Ok(None)) => {
                if self.phase == Phase::Checking || self.phase == Phase::Available {
                    self.phase = Phase::Latest;
                }
            }
            UpMsg::Checked(Err(e)) => {
                if self.phase != Phase::Available {
                    self.phase = Phase::Error;
                    self.error = Some((During::Check, e));
                }
            }
            UpMsg::Progress(p) => {
                if self.phase == Phase::Downloading {
                    self.percent = p;
                }
            }
            UpMsg::Downloaded(Ok(path)) => {
                self.installer = Some(path);
                self.install();
            }
            UpMsg::Downloaded(Err(e)) => {
                self.phase = Phase::Error;
                self.error = Some((During::Download, e));
            }
        }
    }

    fn install(&mut self) {
        self.phase = Phase::Installing;
        self.percent = 100;
        self.install_at = Some(Instant::now());
    }

    /// Timer work: scheduled checks, the fake download, and the install step. Returns true to quit the app now
    /// (the installer was started).
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.next_check.is_some_and(|t| now >= t) {
            self.next_check = Some(now + CHECK_EVERY);
            self.check();
        }
        if let Some(t) = self.fake_tick {
            if now.duration_since(t) >= Duration::from_millis(180) {
                if self.percent >= 100 {
                    self.fake_tick = None;
                    self.install();
                } else {
                    self.percent = (self.percent + 7).min(100);
                    self.fake_tick = Some(now);
                }
            }
        }
        if let Some(t) = self.install_at {
            if now.duration_since(t) >= Duration::from_millis(1200) {
                self.install_at = None;
                if self.fake.is_some() {
                    self.current = self.release.take().map(|r| r.version).unwrap_or_default();
                    self.phase = Phase::Latest;
                    return false;
                }
                if let Some(path) = self.installer.take() {
                    match run_installer(&path) {
                        Ok(()) => return true,
                        Err(e) => {
                            self.phase = Phase::Error;
                            self.error = Some((During::Download, e));
                        }
                    }
                }
            }
        }
        false
    }

    /// The next moment `tick` has work.
    pub fn next_due(&self) -> Option<Instant> {
        let mut t = self.next_check;
        let mut min = |x: Option<Instant>| {
            if let Some(x) = x {
                t = Some(t.map_or(x, |t| t.min(x)));
            }
        };
        min(self.fake_tick.map(|t| t + Duration::from_millis(180)));
        min(self.install_at.map(|t| t + Duration::from_millis(1200)));
        t
    }
}

fn feed_name() -> &'static str {
    if cfg!(windows) {
        "latest.yml"
    } else {
        "latest-linux.yml"
    }
}

/// Reads `version`, and the first `files` entry (`url`, `sha512`, `size`) of an electron-builder update feed.
pub fn parse_feed(yml: &str) -> Option<Release> {
    let mut version = None;
    let (mut file, mut sha, mut size) = (None, None, None);
    let (mut path, mut top_sha) = (None, None);
    let mut in_files = false;
    let mut first_done = false;
    let unquote = |v: &str| v.trim().trim_matches('\'').trim_matches('"').to_string();
    for line in yml.lines() {
        let indented = line.starts_with(' ') || line.starts_with('-');
        let l = line.trim();
        if !indented {
            in_files = l.starts_with("files:");
            if let Some(v) = l.strip_prefix("version:") {
                version = Some(unquote(v));
            } else if let Some(v) = l.strip_prefix("path:") {
                path = Some(unquote(v));
            } else if let Some(v) = l.strip_prefix("sha512:") {
                top_sha = Some(unquote(v));
            }
            continue;
        }
        if !in_files || first_done {
            continue;
        }
        let l = l.trim_start_matches("- ").trim();
        if line.trim_start().starts_with("- ") && file.is_some() {
            first_done = true;
            continue;
        }
        if let Some(v) = l.strip_prefix("url:") {
            file = Some(unquote(v));
        } else if let Some(v) = l.strip_prefix("sha512:") {
            sha = Some(unquote(v));
        } else if let Some(v) = l.strip_prefix("size:") {
            size = unquote(v).parse().ok();
        }
    }
    Some(Release { version: version?, file: file.or(path)?, sha512: sha.or(top_sha)?, size })
}

fn latest_yml() -> Result<Release, String> {
    let url = format!("https://github.com/{REPO}/releases/latest/download/{}", feed_name());
    let r = crate::http::get(&url, &[]).map_err(|e| e.to_string())?;
    if !r.ok() {
        return Err(format!("GitHub {}", r.status));
    }
    parse_feed(&r.text()).ok_or_else(|| format!("Bad {}", feed_name()))
}

fn latest_tag() -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let r = crate::http::get(&url, &[("Accept", "application/vnd.github+json")]).map_err(|e| e.to_string())?;
    if !r.ok() {
        return Err(format!("GitHub {}", r.status));
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    Ok(v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string())
}

fn cache_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
    } else {
        std::env::temp_dir()
    };
    base.join("ai-quota-overlay-updater").join("pending")
}

fn download(rel: &Release, progress: &mut dyn FnMut(u32)) -> Result<PathBuf, String> {
    let dir = cache_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = rel.file.rsplit('/').next().unwrap_or(&rel.file).to_string();
    let dest = dir.join(&name);
    let url = if rel.file.starts_with("https://") {
        rel.file.clone()
    } else {
        format!("https://github.com/{REPO}/releases/download/v{}/{}", rel.version, rel.file)
    };
    let mut last = u32::MAX;
    crate::http::download(&url, &dest, &mut |done, total| {
        let total = total.or(rel.size).unwrap_or(0);
        if total > 0 {
            let p = ((done as f64 / total as f64) * 100.0).floor().min(100.0) as u32;
            if p != last {
                last = p;
                progress(p);
            }
        }
    })
    .map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&dest).map_err(|e| e.to_string())?;
    if let Some(size) = rel.size {
        if bytes.len() as u64 != size {
            return Err(format!("size mismatch ({} != {size})", bytes.len()));
        }
    }
    let sha = base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&bytes));
    if sha != rel.sha512 {
        let _ = std::fs::remove_file(&dest);
        return Err("sha512 checksum mismatch".into());
    }
    Ok(dest)
}

/// Starts the installer silently; it waits for this app to exit, installs and starts the new version.
fn run_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new(path).args(["/S", "--updated", "--force-run"]).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        // AppImage: replace the running file (allowed on Linux) and start the new one.
        let target = std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or("not an AppImage")?;
        let tmp = target.with_extension("new");
        std::fs::copy(path, &tmp).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        }
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        std::process::Command::new(&target).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("1.10.0", "1.9.2"));
        assert!(newer("2.0.0", "1.2.0"));
        assert!(!newer("1.2.0", "1.2.0"));
        assert!(!newer("1.2.0-beta", "1.2.0"));
    }

    #[test]
    fn electron_builder_feed() {
        let yml = "version: 1.2.0\nfiles:\n  - url: AiQuotaOverlay-1.2.0-win-x64-setup.exe\n    sha512: abc==\n    size: 94093517\npath: AiQuotaOverlay-1.2.0-win-x64-setup.exe\nsha512: abc==\nreleaseDate: '2026-10-10T08:00:00.000Z'\n";
        let r = parse_feed(yml).unwrap();
        assert_eq!(r.version, "1.2.0");
        assert_eq!(r.file, "AiQuotaOverlay-1.2.0-win-x64-setup.exe");
        assert_eq!(r.sha512, "abc==");
        assert_eq!(r.size, Some(94093517));
        let two = "version: 2.0.0\nfiles:\n  - url: a.AppImage\n    sha512: one\n    size: 1\n  - url: b.deb\n    sha512: two\n    size: 2\npath: a.AppImage\nsha512: one\n";
        let r = parse_feed(two).unwrap();
        assert_eq!((r.file.as_str(), r.sha512.as_str(), r.size), ("a.AppImage", "one", Some(1)));
    }
}
