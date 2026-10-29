//! The app (port of src/main.js and the pages' event handling): windows, layout and anchoring, drag, polling,
//! back-off, the internet check, settings, tray, shortcut and updates. Platform code (src/platform) owns the real
//! windows and calls `App::event`; the app answers through the `Host` trait.
//!
//! Quota: checked every POLL while the card or the app window is on screen, every IDLE_POLL while both are hidden.
//! Checks never overlap, manual refreshes closer than MIN_REFRESH_GAP reuse the last result, and a service that
//! answers "too many requests" is left alone for BACKOFF (doubling up to MAX_BACKOFF, longer when the server's
//! Retry-After asks for it), so checking often never turns into hammering anyone's API. A double-click tries such a
//! service again at once, unless the server's own wait still holds. While a check fails (other than an expired
//! login) the service keeps showing its last good numbers, from this run or from quota.json, up to STALE_MAX old.
//! All of this is per account: a service's own login on this machine and any accounts signed in from the app
//! window (`quota::login`), each with its own id.
use crate::anim::Fades;
use crate::i18n::{self, Lang};
use crate::model::{aid, now_ms, short_who, Aid, NetState, QErr, Quota, Retry, Snapshot, Svc};
use crate::quota::login;
use crate::settings::{CardPos, Settings};
use std::collections::{HashMap, HashSet};
use crate::ui::app_window::{self, AppWin, Action as AppAction};
use crate::ui::card::{self, CardFx, CardIn, Target};
use crate::ui::gfx::Canvas;
use crate::ui::panel::{self, PanelFx, PanelIn};
use crate::ui::text::Fonts;
use crate::ui::toast::{self, Btn, Kind, ToastFx, ToastIn};
use crate::updater::{self, Phase, UpMsg, Updater};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

const POLL: Duration = Duration::from_secs(60);
const IDLE_POLL: Duration = Duration::from_secs(5 * 60);
/// Claude's usage answers "too many requests" after a few checks a minute apart, and Copilot's quota is monthly:
/// those are checked this often (a refresh by hand still checks them at once).
const SLOW_POLL: Duration = Duration::from_secs(5 * 60);

/// How often an account of `s` is checked while the overlay is on screen.
fn every(s: Svc) -> Duration {
    match s {
        Svc::Claude | Svc::Copilot => SLOW_POLL,
        Svc::Codex | Svc::Cursor => POLL,
    }
}
const MIN_REFRESH_GAP: Duration = Duration::from_secs(10);
const BACKOFF_MS: i64 = 2 * 60 * 1000;
const MAX_BACKOFF_MS: i64 = 15 * 60 * 1000;
/// A Retry-After longer than this is taken as this.
const MAX_SERVER_WAIT_MS: i64 = 60 * 60 * 1000;
const STALE_MAX_MS: i64 = 12 * 60 * 60 * 1000;
/// Internet check: online when any of these answers within NET_TIMEOUT. Any HTTP status counts, only a failed
/// connection doesn't, so a captive portal or a blocked host still reads as online as long as one gets through.
const NET_EVERY: Duration = Duration::from_secs(3);
const NET_TIMEOUT: Duration = Duration::from_millis(2500);
const NET_URLS: [&str; 2] = ["https://www.gstatic.com/generate_204", "https://1.1.1.1/cdn-cgi/trace"];
const MARGIN: f64 = 16.0;
/// The card and the panel sit 2 CSS px inside their windows (room for the panel's outline) and 4 CSS px apart on
/// screen; the overlay is drawn at the zoom of the chosen size (CSS px × zoom = DIP, `Settings::zoom`).
const PAD_CSS: f64 = 2.0;
const GAP_CSS: f64 = 4.0;
/// Ring labels and reset times count down.
const RELABEL: Duration = Duration::from_secs(30);
const FRAME: Duration = Duration::from_millis(16);

pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WinKind {
    Card,
    Panel,
    Toast,
    App,
}

/// A rectangle in DIP (device-independent px, the coordinate space Electron used).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectD {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectD {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn intersects(&self, o: &RectD) -> bool {
        self.x < o.x + o.w && self.x + self.w > o.x && self.y < o.y + o.h && self.y + self.h > o.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub bounds: RectD,
    pub work: RectD,
    /// Device px per DIP.
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn3 {
    Left,
    Right,
    #[allow(dead_code)] // macOS / Linux
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mouse {
    Move,
    Down(Btn3),
    Up(Btn3),
    Dbl(Btn3),
    Leave,
    /// Wheel notches, positive = away from the user (scroll up).
    Wheel(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Other,
}

/// Not every platform sends every event (no tray menu on Windows, no display events from winit).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    /// Position in DIP relative to the window's top-left.
    Mouse { w: WinKind, m: Mouse, x: f64, y: f64 },
    Key { w: WinKind, key: Key },
    /// The window lost focus (the panel closes).
    Blur(WinKind),
    /// The app window was closed by the system (Alt+F4, taskbar).
    Close(WinKind),
    /// The app window's client size changed (DIP) or its scale did.
    Resized(WinKind),
    DisplaysChanged,
    Resume,
    TrayClick,
    TrayOpen,
    TrayQuit,
    Hotkey,
    SecondInstance,
    Timer,
    Wake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Default,
    Pointer,
    Grab,
    Grabbing,
}

/// What the platform layer provides.
pub trait Host {
    fn displays(&self) -> Vec<Display>;
    fn primary(&self) -> Display;
    /// Cursor position in DIP.
    fn cursor(&self) -> (f64, f64);
    /// Device px per DIP and the device-px size a window placed at `r` will have.
    fn device_size(&self, w: WinKind, r: RectD) -> (f64, u32, u32);
    /// Places a window at `r` (DIP) and shows `pm` (exactly `device_size` big) at opacity `alpha`.
    fn present(&mut self, w: WinKind, r: RectD, pm: &Pixmap, alpha: f64, activate: bool);
    fn hide(&mut self, w: WinKind);
    fn set_cursor(&mut self, c: CursorKind);
    fn set_tooltip(&mut self, w: WinKind, text: Option<&str>);
    /// Opens (or brings up) the normal app window at `r` (DIP).
    fn app_open(&mut self, r: RectD, title: &str);
    fn app_close(&mut self);
    fn app_is_open(&self) -> bool;
    /// Client size in DIP and device px per DIP of the open app window.
    fn app_size(&self) -> Option<(f64, f64, f64)>;
    fn app_present(&mut self, pm: &Pixmap);
    /// Shows `pm` again where only the device px box (x, y, w, h) changed since the last present.
    fn app_present_rect(&mut self, pm: &Pixmap, r: (u32, u32, u32, u32)) {
        let _ = r;
        self.app_present(pm);
    }
    fn app_title(&mut self, title: &str);
    /// Starts moving the app window with the mouse (from the custom title bar).
    fn app_drag(&mut self);
    fn tray_tooltip(&mut self, text: &str);
    fn tray_menu(&mut self, open: &str, quit: &str);
    /// Launch at startup: None where it isn't supported (Linux).
    fn login_item(&self) -> Option<bool>;
    fn set_login_item(&mut self, on: bool);
    fn open_url(&mut self, url: &str);
    /// Puts `text` on the clipboard.
    fn copy_text(&mut self, text: &str);
    fn quit(&mut self);
    fn relaunch(&mut self);
    /// The next `Ev::Timer` comes after `after` (replacing any earlier request).
    fn set_timer(&mut self, after: Duration);
}

pub enum Msg {
    Quota(Snapshot),
    Net(bool, u64),
    Update(UpMsg),
    /// News from sign-in number `n`.
    Login(u64, Svc, login::Event),
}

/// Checks every account except the ids given.
pub type Fetcher = fn(&HashSet<String>) -> Snapshot;

/// A service left alone after "too many requests": until when (unix ms), the current back-off, and the server's
/// own Retry-After (0 = none), which a manual retry doesn't skip.
#[derive(Clone, Copy, Debug)]
struct Cool {
    until: i64,
    ms: i64,
    server: i64,
}

/// An error the user has to fix (log in again): shown as is, never hidden behind old numbers.
fn needs_login(e: &QErr) -> bool {
    e.code.ends_with("Expired") || e.code.ends_with("ExpiredKeychain")
}

/// The line under the app window's Services heading after a sign-in: what happened, and whether it went wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub bad: bool,
}

struct Drag {
    from_card: (f64, f64),
    from_cursor: (f64, f64),
    moving: bool,
    target: Option<Target>,
}

pub struct App {
    pub settings: Settings,
    fonts: Fonts,
    fetcher: Fetcher,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    waker: Waker,
    pub updater: Updater,

    last: Option<Snapshot>,
    last_fetch: Option<Instant>,
    refreshing: bool,
    /// Check again as soon as the check under way is back (an account was just signed in).
    refresh_again: bool,
    /// When each account was last checked; the ones left out of the check under way as not due yet (`every`) keep
    /// their numbers; the next check takes every account (a refresh by hand, waking up).
    checked: HashMap<String, Instant>,
    resting: HashSet<String>,
    check_all: bool,
    /// By account id.
    cooling: HashMap<String, Cool>,
    /// Each account's last good numbers (`as_of` = when fetched), also kept in quota.json.
    good: crate::cache::Good,
    /// The sign-in under way (one at a time) and its number; the line it left in the app window.
    login: Option<login::Pending>,
    login_n: u64,
    /// The code to type on the sign-in page, once it came (GitHub).
    login_code: Option<String>,
    note: Option<Note>,
    net_busy: bool,
    net: Option<NetState>,
    next_net: Instant,
    resume_at: Option<Instant>,
    next_relabel: Instant,

    card: (f64, f64),
    card_size: (f64, f64),
    panel_size: (f64, f64),
    toast_size: (f64, f64),
    selected: Option<(Aid, i32)>,
    anchor_right: bool,
    anchor_bottom: bool,
    drag: Option<Drag>,
    visible: bool,
    panel_shown: bool,
    toast_shown: bool,

    // animation
    card_hover: Fades<Target>,
    card_over: Fades<u8>,
    panel_hover: Fades<u8>,
    toast_hover: Fades<Btn>,
    panel_pop: Option<Instant>,
    toast_pop: Option<Instant>,
    toast_t0: Instant,
    toast_bar: f32,
    last_frame: Instant,
    /// When the app window was last painted (changed parts are painted at most once a frame).
    app_painted: Instant,
    started: Instant,
    hover_card: Option<Target>,
    hover_panel: bool,
    hover_toast: Option<Btn>,

    card_layout: Option<card::CardLayout>,
    panel_layout: Option<panel::PanelLayout>,
    toast_layout: Option<toast::ToastLayout>,
    dirty_card: bool,
    dirty_panel: bool,
    dirty_toast: bool,

    app_win: Option<AppWin>,
    /// The app window as last painted (partial repaints draw into it) and its device px per DIP.
    app_pm: Option<(Pixmap, f64)>,
    /// Whether the app starts with the system (read when the app window opens, not every frame), and whether the
    /// last change to it didn't take.
    startup: Option<bool>,
    startup_failed: bool,
    /// A change that is saved a moment later (the opacity slider moves many times a second).
    save_at: Option<Instant>,
    quitting: bool,
}

impl App {
    pub fn new(fetcher: Fetcher, waker: Waker) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let settings = Settings::load();
        let updater = Updater::new(tx.clone(), waker.clone());
        let now = Instant::now();
        Self {
            settings,
            fonts: Fonts::load(),
            fetcher,
            tx,
            rx,
            waker,
            updater,
            last: None,
            last_fetch: None,
            refreshing: false,
            refresh_again: false,
            checked: HashMap::new(),
            resting: HashSet::new(),
            check_all: false,
            cooling: HashMap::new(),
            good: crate::cache::load(),
            login: None,
            login_n: 0,
            login_code: None,
            note: None,
            net_busy: false,
            net: None,
            next_net: now,
            resume_at: None,
            next_relabel: now + RELABEL,
            card: (0.0, 0.0),
            card_size: (192.0, 91.0),
            panel_size: (0.0, 0.0),
            toast_size: (0.0, 0.0),
            selected: None,
            anchor_right: true,
            anchor_bottom: true,
            drag: None,
            visible: true,
            panel_shown: false,
            toast_shown: false,
            card_hover: Fades::new(0.12),
            card_over: Fades::new(0.15),
            panel_hover: Fades::new(0.12),
            toast_hover: Fades::new(0.12),
            panel_pop: None,
            toast_pop: None,
            toast_t0: now,
            toast_bar: 0.0,
            last_frame: now,
            app_painted: now,
            started: now,
            hover_card: None,
            hover_panel: false,
            hover_toast: None,
            card_layout: None,
            panel_layout: None,
            toast_layout: None,
            dirty_card: true,
            dirty_panel: false,
            dirty_toast: true,
            app_win: None,
            app_pm: None,
            startup: None,
            startup_failed: false,
            save_at: None,
            quitting: false,
        }
    }


    fn lang(&self) -> Lang {
        Lang::from_key(&self.settings.lang).unwrap_or(Lang::En)
    }

    // ---------- start ----------

    pub fn start(&mut self, h: &mut dyn Host) {
        self.card = self.initial_card(h);
        self.update_anchor(h);
        self.refresh(true);
        self.apply_network();
        if self.lang() == Lang::En || self.lang() == Lang::Th {
            let m = &i18n::t(self.lang()).menu;
            h.tray_menu(m.settings, m.quit);
        }
        h.tray_tooltip(crate::settings::APP_NAME);
        if std::env::var_os("QUOTA_OVERLAY_SETTINGS").is_some() {
            self.open_settings(h);
        }
        self.flush(h);
    }

    fn default_card(&self, h: &dyn Host) -> (f64, f64) {
        let a = h.primary().work;
        (a.x + a.w - self.card_size.0 - MARGIN, a.y + a.h - self.card_size.1 - MARGIN)
    }

    /// Back where it was left, at its size then, so the corner it's anchored to stays put once the real size comes in.
    fn initial_card(&mut self, h: &dyn Host) -> (f64, f64) {
        let Some(saved) = self.settings.card else { return self.default_card(h) };
        let size = match (saved.w, saved.h) {
            (Some(w), Some(hh)) if w > 0.0 => (w, hh),
            _ => self.card_size,
        };
        let r = RectD::new(saved.x, saved.y, size.0, size.1);
        if !h.displays().iter().any(|d| d.work.intersects(&r)) {
            return self.default_card(h);
        }
        self.card_size = size;
        (saved.x, saved.y)
    }

    fn display_near(&self, h: &dyn Host, x: f64, y: f64) -> Display {
        let ds = h.displays();
        let dist = |d: &Display| {
            let b = d.bounds;
            let dx = if x < b.x { b.x - x } else if x >= b.x + b.w { x - (b.x + b.w) } else { 0.0 };
            let dy = if y < b.y { b.y - y } else if y >= b.y + b.h { y - (b.y + b.h) } else { 0.0 };
            dx * dx + dy * dy
        };
        ds.iter().cloned().min_by(|a, b| dist(a).total_cmp(&dist(b))).unwrap_or_else(|| h.primary())
    }

    /// Moves the card back inside the work area of the display it is on (after it grew).
    fn keep_on_screen(&mut self, h: &dyn Host) {
        let (cx, cy) = (self.card.0 + self.card_size.0 / 2.0, self.card.1 + self.card_size.1 / 2.0);
        let a = self.display_near(h, cx.round(), cy.round()).work;
        self.card.0 = self.card.0.min(a.x + a.w - self.card_size.0).max(a.x);
        self.card.1 = self.card.1.min(a.y + a.h - self.card_size.1).max(a.y);
    }

    /// The panel opens toward the middle of the display the card is on.
    fn update_anchor(&mut self, h: &dyn Host) {
        let cx = self.card.0 + self.card_size.0 / 2.0;
        let cy = self.card.1 + self.card_size.1 / 2.0;
        let a = self.display_near(h, cx.round(), cy.round()).work;
        self.anchor_right = cx > a.x + a.w / 2.0;
        self.anchor_bottom = cy > a.y + a.h / 2.0;
    }

    /// The overlay's zoom (CSS px → DIP) and the padding around the card and panel inside their windows (DIP).
    fn zoom(&self) -> f64 {
        self.settings.zoom()
    }

    fn pad(&self) -> f64 {
        PAD_CSS * self.zoom()
    }

    /// Window bounds for a box at `pos` of `size`, with the padding to spare around it.
    fn around(&self, pos: (f64, f64), size: (f64, f64)) -> RectD {
        let pad = self.pad();
        RectD::new((pos.0 - pad).round(), (pos.1 - pad).round(), (size.0 + 2.0 * pad).ceil(), (size.1 + 2.0 * pad).ceil())
    }

    /// Above/below the card when horizontal, beside it when vertical, aligned to the card edge facing the screen
    /// border. The detail panel and the update toast both open there (the toast steps aside while a panel is open).
    fn popup_pos(&self, size: (f64, f64)) -> (f64, f64) {
        let (c, cs) = (self.card, self.card_size);
        #[allow(non_snake_case)]
        let GAP = GAP_CSS * self.zoom();
        if self.settings.vertical {
            (
                if self.anchor_right { c.0 - GAP - size.0 } else { c.0 + cs.0 + GAP },
                if self.anchor_bottom { c.1 + cs.1 - size.1 } else { c.1 },
            )
        } else {
            (
                if self.anchor_right { c.0 + cs.0 - size.0 } else { c.0 },
                if self.anchor_bottom { c.1 - GAP - size.1 } else { c.1 + cs.1 + GAP },
            )
        }
    }

    // ---------- events ----------

    pub fn event(&mut self, h: &mut dyn Host, ev: Ev) {
        match ev {
            Ev::Mouse { w: WinKind::Card, m, x, y } => self.card_mouse(h, m, x, y),
            Ev::Mouse { w: WinKind::Panel, m, x, y } => self.panel_mouse(h, m, x, y),
            Ev::Mouse { w: WinKind::Toast, m, x, y } => self.toast_mouse(h, m, x, y),
            Ev::Mouse { w: WinKind::App, m, x, y } => self.app_mouse(h, m, x, y),
            Ev::Key { w: WinKind::App, key } => {
                if key == Key::Escape {
                    self.close_settings(h);
                }
            }
            Ev::Key { .. } => {}
            Ev::Blur(WinKind::Panel) => {
                if self.selected.is_some() {
                    self.select(None);
                }
            }
            Ev::Blur(_) => {}
            Ev::Close(WinKind::App) => self.close_settings(h),
            Ev::Close(_) => {}
            Ev::Resized(WinKind::App) => {
                if let Some(a) = &mut self.app_win {
                    a.dirty = true;
                }
            }
            Ev::Resized(_) => {}
            Ev::DisplaysChanged => {
                self.update_anchor(h);
                self.dirty_card = true;
                self.dirty_panel = self.selected.is_some();
                self.dirty_toast = true;
            }
            // After sleep everything is stale: check it all again (each check still keeps its own guards).
            Ev::Resume => self.resume_at = Some(Instant::now() + Duration::from_secs(5)),
            Ev::TrayClick | Ev::TrayOpen => self.open_settings(h),
            Ev::TrayQuit => self.quit(h),
            Ev::Hotkey => self.toggle_visible(h),
            // Starting the app again (Start menu, desktop shortcut) while it runs brings up the overlay and the app window.
            Ev::SecondInstance => {
                if !self.visible {
                    self.toggle_visible(h);
                }
                self.open_settings(h);
            }
            Ev::Timer | Ev::Wake => {}
        }
        self.drain(h);
        self.tick(h);
        self.flush(h);
    }

    fn drain(&mut self, h: &mut dyn Host) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Quota(s) => self.on_quota(h, s),
                Msg::Net(online, ms) => self.on_net(online, ms),
                Msg::Update(u) => {
                    self.updater.on_msg(u);
                    self.on_update_change();
                }
                Msg::Login(n, _, login::Event::Code(code)) => {
                    // Ready to paste on the page that is already open.
                    if n == self.login_n && self.login.is_some() {
                        h.copy_text(&code);
                        self.login_code = Some(code);
                        if let Some(a) = &mut self.app_win {
                            a.dirty = true;
                        }
                    }
                }
                Msg::Login(n, svc, login::Event::Done(o)) => self.on_login(n, svc, o),
            }
        }
    }

    fn tick(&mut self, h: &mut dyn Host) {
        let now = Instant::now();
        // A pointer that left without the system saying so (the card moved or hid under a still pointer) must not
        // leave the card lit.
        if self.hover_card.is_some() && self.drag.is_none() {
            let (x, y) = h.cursor();
            if !self.visible || !self.around(self.card, self.card_size).contains(x, y) {
                self.set_card_hover(h, None);
            }
        }
        // Polling: due once POLL (on screen) or IDLE_POLL (all hidden) has passed since the last check.
        let gap = if self.on_screen(h) { POLL } else { IDLE_POLL };
        if self.last_fetch.is_none_or(|t| now.duration_since(t) + Duration::from_secs(1) >= gap) {
            self.refresh(true);
        }
        if self.settings.network && self.visible && now >= self.next_net {
            self.check_network();
        }
        if now >= self.next_relabel {
            self.next_relabel = now + RELABEL;
            self.dirty_card = true;
            self.dirty_panel = self.selected.is_some();
            if let Some(a) = &mut self.app_win {
                a.dirty = true;
            }
        }
        if self.resume_at.is_some_and(|t| now >= t) {
            self.resume_at = None;
            self.check_all = true;
            self.refresh(true);
            self.check_network();
            self.updater.check_if_stale(Duration::ZERO);
            self.on_update_change();
        }
        let before = self.updater.view(None).phase;
        let percent = self.updater.view(None).percent;
        if self.updater.tick(now) {
            self.save();
            h.quit();
            self.quitting = true;
            return;
        }
        let v = self.updater.view(None);
        if v.phase != before || v.percent != percent {
            self.on_update_change();
        }
    }

    fn on_screen(&self, h: &dyn Host) -> bool {
        self.visible || h.app_is_open()
    }

    // ---------- quota ----------

    /// force: skip the MIN_REFRESH_GAP guard (timer, a service switched on, back online, wake from sleep).
    pub fn refresh(&mut self, force: bool) {
        if self.refreshing {
            return;
        }
        if !force && self.last.is_some() && self.last_fetch.is_some_and(|t| t.elapsed() < MIN_REFRESH_GAP) {
            // Nothing new to fetch; the views show the last result (the app window's refresh icon stops).
            if let Some(a) = &mut self.app_win {
                a.refresh_spin = false;
                a.dirty = true;
            }
            return;
        }
        self.refreshing = true;
        let at = Instant::now();
        self.last_fetch = Some(at);
        let now = now_ms();
        let mut skip: HashSet<String> = self.settings.services.iter().filter(|(_, on)| !**on).map(|(id, _)| id.clone()).collect();
        skip.extend(self.cooling.iter().filter(|(_, c)| c.until > now).map(|(id, _)| id.clone()));
        // Accounts checked less often (`every`) sit this one out until they are due.
        self.resting.clear();
        if !std::mem::take(&mut self.check_all) {
            for (a, _) in self.last.iter().flat_map(|l| &l.accts) {
                let due = self.checked.get(&a.id).is_none_or(|t| at.duration_since(*t) + Duration::from_secs(2) >= every(a.svc));
                if !due && !skip.contains(&a.id) {
                    self.resting.insert(a.id.clone());
                }
            }
        }
        skip.extend(self.resting.iter().cloned());
        let fetch = self.fetcher;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let snap = fetch(&skip);
            let _ = tx.send(Msg::Quota(snap));
            waker();
        });
    }

    /// Checks now, or right after the check under way (which may have started before what changed).
    fn refresh_soon(&mut self) {
        if self.refreshing {
            self.refresh_again = true;
        } else {
            self.refresh(true);
        }
    }

    /// A double-click on the card or "Refresh now": also tries accounts that are waiting out a "too many requests",
    /// except while the server's own Retry-After holds.
    fn retry_now(&mut self) {
        self.check_all = true;
        let now = now_ms();
        for c in self.cooling.values_mut() {
            if c.server <= now && c.until > now {
                c.until = now - 1;
            }
        }
        self.refresh(false);
    }

    fn retry_of(&self, id: &str, now: i64) -> Option<Retry> {
        self.cooling.get(id).map(|c| Retry { at: c.until, manual: c.server <= now })
    }

    /// What to show for an account whose check failed with `err`: its last good numbers (if not too old and the
    /// login is fine) carrying the error, else the error alone.
    fn failed(&self, id: &str, err: QErr, now: i64) -> Quota {
        let retry = self.retry_of(id, now);
        let good = if needs_login(&err) { None } else { self.good.get(id).and_then(|g| crate::cache::aged(g, now, STALE_MAX_MS)) };
        match good {
            Some(g) => Quota { error: Some(err), retry, ..g },
            None => Quota { error: Some(err), retry, ..Quota::default() },
        }
    }

    fn on_quota(&mut self, h: &mut dyn Host, mut fresh: Snapshot) {
        // In the order the user has now (a check that started before a drag and drop came back in the old one).
        let order: Vec<String> = crate::quota::store::list().into_iter().map(|s| s.id).collect();
        fresh.accts.sort_by_key(|(a, _)| order.iter().position(|id| *id == a.id).unwrap_or(usize::MAX));
        let now = now_ms();
        let started = now - self.last_fetch.map(|t| t.elapsed().as_millis() as i64).unwrap_or(0);
        let fetched_at = fresh.fetched_at;
        let mut store = false;
        let checked_at = self.last_fetch.unwrap_or_else(Instant::now);
        for (a, q) in fresh.accts.iter_mut() {
            let id = a.id.as_str();
            if !self.settings.on(id) {
                self.cooling.remove(id);
                continue;
            }
            if self.resting.contains(id) {
                // Not due this time: as it was.
                if let Some(prev) = self.last.as_ref().and_then(|l| l.get(id)) {
                    *q = prev.clone();
                }
                continue;
            }
            if !q.disabled {
                self.checked.insert(id.to_string(), checked_at);
            }
            if self.cooling.get(id).is_some_and(|c| c.until > started) {
                // Not checked this time: keep the last error, with the numbers aged and the next try updated.
                let err = self.last.as_ref().and_then(|l| l.get(id)).and_then(|q| q.error.clone());
                *q = self.failed(id, err.unwrap_or_else(|| QErr::rate_limited(None)), now);
                continue;
            }
            if q.ok || q.absent {
                self.cooling.remove(id);
                if q.ok {
                    self.good.insert(id.to_string(), Quota { as_of: Some(fetched_at), ..q.clone() });
                    store = true;
                }
                continue;
            }
            let err = q.error.clone().unwrap_or_default();
            if q.rate_limited() {
                let ms = (self.cooling.get(id).map(|c| c.ms).unwrap_or(BACKOFF_MS / 2) * 2).min(MAX_BACKOFF_MS);
                let server = err.retry_at.map_or(0, |t| t.min(started + MAX_SERVER_WAIT_MS));
                self.cooling.insert(id.to_string(), Cool { until: (started + ms).max(server), ms, server });
            } else {
                self.cooling.remove(id);
            }
            *q = self.failed(id, err, now);
        }
        if store {
            crate::cache::save(&self.good);
        }
        self.last = Some(fresh);
        self.refreshing = false;
        if std::mem::take(&mut self.refresh_again) {
            self.refresh(true);
        }
        self.remember_windows();
        self.dirty_card = true;
        self.dirty_panel = self.selected.is_some();
        if let Some(a) = &mut self.app_win {
            a.refresh_spin = false;
            a.dirty = true;
        }
        self.update_tooltip(h);
        if let Ok(path) = std::env::var("QUOTA_OVERLAY_SNAPSHOT") {
            self.flush(h);
            self.snapshot(h, &path);
        }
    }

    /// Remember each account's windows, so the app window can still list them while it's switched off or offline.
    fn remember_windows(&mut self) {
        let Some(last) = &self.last else { return };
        let mut changed = false;
        for (a, q) in &last.accts {
            if !q.ok || q.windows.is_empty() {
                continue;
            }
            let seen: Vec<_> = q.windows.iter().map(|w| w.seen()).collect();
            if self.settings.seen.get(&a.id) == Some(&seen) {
                continue;
            }
            self.settings.seen.insert(a.id.clone(), seen);
            changed = true;
        }
        if changed {
            self.save();
        }
    }

    fn update_tooltip(&mut self, h: &mut dyn Host) {
        let Some(last) = &self.last else {
            h.tray_tooltip(crate::settings::APP_NAME);
            return;
        };
        let lang = self.lang();
        let mut lines = Vec::new();
        for (a, q) in &last.accts {
            if q.absent {
                continue;
            }
            let name = if last.count(a.svc) > 1 && !a.who.is_empty() { format!("{} ({})", a.svc.name(), short_who(&a.who)) } else { a.svc.name().to_string() };
            if !q.ok {
                lines.push(format!("{name}: {}", q.error.as_ref().map(|e| i18n::error_text(e, lang)).unwrap_or_default()));
                continue;
            }
            let hidden = self.settings.hidden_of(&a.id);
            let parts: Vec<String> = q
                .windows
                .iter()
                .filter(|w| !hidden.contains(&w.id))
                .map(|w| format!("{} {}%", i18n::window_label(w, lang, true), crate::ui::js_round(100.0 - w.used())))
                .collect();
            let mut line = format!("{name} {}", parts.join(" · "));
            if let Some(at) = q.as_of.filter(|_| q.stale()) {
                line += &format!(" ({})", crate::fmt::time(at, lang));
            }
            lines.push(line);
        }
        let mut text = lines.join("\n");
        if text.is_empty() {
            text = i18n::t(lang).menu.not_logged_in.to_string();
        }
        // Tray tooltips hold 127 characters.
        let cut = String::from_utf16_lossy(&text.encode_utf16().take(127).collect::<Vec<u16>>());
        h.tray_tooltip(&cut);
    }

    /// Debug aid: save a PNG of the card after the first fetch, then exit. QUOTA_OVERLAY_SNAPSHOT_APP=<png> also
    /// saves the app window (at 2×).
    fn snapshot(&mut self, h: &mut dyn Host, path: &str) {
        if let Some(pm) = self.render_card(h) {
            let _ = pm.save_png(path);
        }
        if let Ok(p) = std::env::var("QUOTA_OVERLAY_SNAPSHOT_APP") {
            let (w, hh) = (app_window::SIZE.0 as f32, app_window::SIZE.1 as f32);
            if let Some(mut pm) = Pixmap::new((w * 2.0) as u32, (hh * 2.0) as u32) {
                let ctx = self.app_ctx();
                {
                    let mut c = Canvas::new(&mut pm, 2.0, 0.0, 0.0);
                    AppWin::new().draw(&mut c, &mut self.fonts, &ctx, w, hh);
                }
                let _ = pm.save_png(p);
            }
        }
        self.quit(h);
    }

    // ---------- signing in ----------

    /// Signs in to another account of `svc` in the browser (giving up on any sign-in still under way).
    fn sign_in(&mut self, h: &mut dyn Host, svc: Svc) {
        self.login = None;
        self.login_code = None;
        self.login_n += 1;
        let n = self.login_n;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        match login::start(svc, self.lang(), move |e| {
            let _ = tx.send(Msg::Login(n, svc, e));
            waker();
        }) {
            Ok(p) => {
                h.open_url(&p.url);
                self.login = Some(p);
                self.note = None;
            }
            Err(e) => self.note = Some(Note { text: i18n::login_failed(self.lang(), &e), bad: true }),
        }
    }

    fn on_login(&mut self, n: u64, svc: Svc, o: login::Outcome) {
        let current = n == self.login_n;
        if current {
            self.login = None;
            self.login_code = None;
        }
        match o {
            // Stored whichever sign-in it was: check it.
            Ok(id) => {
                let who = crate::quota::store::get(&id).map(|s| s.email).unwrap_or_default();
                self.note = Some(Note { text: i18n::signed_in(self.lang(), svc.name(), &who), bad: false });
                self.cooling.remove(&id);
                if self.settings.services.remove(&id).is_some() {
                    self.save();
                }
                self.refresh_soon();
            }
            Err(Some(e)) if current => self.note = Some(Note { text: i18n::login_failed(self.lang(), &e), bad: true }),
            Err(_) => {}
        }
        if let Some(a) = &mut self.app_win {
            a.dirty = true;
        }
    }

    /// Forgets an account: its tokens, settings and numbers.
    fn sign_out(&mut self, h: &mut dyn Host, id: &str) {
        let _ = crate::quota::store::remove(id);
        self.settings.forget(id);
        if let Some(l) = &mut self.last {
            l.accts.retain(|(a, _)| a.id != id);
        }
        self.save();
        if self.good.remove(id).is_some() {
            crate::cache::save(&self.good);
        }
        self.cooling.remove(id);
        self.checked.remove(id);
        if self.selected.is_some_and(|(x, _)| x == aid(id)) {
            self.select(None);
        }
        self.note = None;
        self.dirty_card = true;
        self.update_tooltip(h);
    }

    // ---------- internet ----------

    /// ms is the fastest answer. Coming back online refetches the quota, which failed while offline.
    fn check_network(&mut self) {
        self.next_net = Instant::now() + NET_EVERY;
        if self.net_busy || !self.visible {
            return;
        }
        self.net_busy = true;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let start = Instant::now();
            let (dtx, drx) = std::sync::mpsc::channel();
            for url in NET_URLS {
                let dtx = dtx.clone();
                std::thread::spawn(move || {
                    let _ = dtx.send(crate::http::head_ok(url, NET_TIMEOUT));
                });
            }
            drop(dtx);
            let mut online = false;
            while let Ok(ok) = drx.recv_timeout(NET_TIMEOUT + Duration::from_millis(200)) {
                if ok {
                    online = true;
                    break;
                }
            }
            let _ = tx.send(Msg::Net(online, start.elapsed().as_millis() as u64));
            waker();
        });
    }

    fn on_net(&mut self, online: bool, ms: u64) {
        self.net_busy = false;
        if !self.settings.network {
            return; // switched off while the check was out
        }
        let was_offline = self.net.is_some_and(|n| !n.online);
        self.net = Some(NetState { online, ms: online.then_some(ms) });
        self.dirty_card = true;
        if let Some(a) = &mut self.app_win {
            a.damage_net();
        }
        if online && was_offline {
            self.check_all = true;
            self.refresh(true);
        }
    }

    /// The internet check only runs while it's shown on the card (app window → Internet status, off by default).
    fn apply_network(&mut self) {
        if self.settings.network {
            self.next_net = Instant::now();
            self.check_network();
        } else {
            self.net = None;
            self.dirty_card = true;
        }
    }

    // ---------- selection, visibility ----------

    /// Opening a ring's panel: lay it out, move the window there, then reveal it, so it never shows at its old spot.
    fn select(&mut self, next: Option<(Aid, i32)>) {
        self.selected = next;
        self.dirty_card = true;
        self.dirty_toast = true;
        if next.is_some() {
            self.panel_pop = Some(Instant::now());
            self.dirty_panel = true;
            self.panel_hover.snap();
            self.hover_panel = false;
        } else {
            self.dirty_panel = true;
        }
    }

    fn toggle_visible(&mut self, h: &mut dyn Host) {
        self.visible = !self.visible;
        if !self.visible {
            self.select(None);
            h.hide(WinKind::Card);
            // Hidden under the pointer (the shortcut): it comes back unlit.
            self.set_card_hover(h, None);
            self.card_hover.snap();
            self.card_over.snap();
        }
        self.dirty_card = true;
        self.dirty_toast = true;
        if let Some(a) = &mut self.app_win {
            a.dirty = true;
        }
    }

    fn reset_position(&mut self, h: &mut dyn Host) {
        self.settings.card = None;
        self.save();
        self.card = self.default_card(h);
        self.update_anchor(h);
        self.dirty_card = true;
        self.dirty_panel = self.selected.is_some();
        self.dirty_toast = true;
    }

    fn save(&self) {
        self.settings.save();
    }

    fn quit(&mut self, h: &mut dyn Host) {
        self.save_at = None;
        self.save();
        self.quitting = true;
        h.quit();
    }

    fn restart(&mut self, h: &mut dyn Host) {
        self.save_at = None;
        self.save();
        self.quitting = true;
        h.relaunch();
    }

    // ---------- card mouse ----------

    fn card_mouse(&mut self, h: &mut dyn Host, m: Mouse, x: f64, y: f64) {
        let (cx, cy) = ((x - self.pad()) / self.zoom(), (y - self.pad()) / self.zoom());
        let hit = self.card_layout.as_ref().and_then(|l| l.hit(cx as f32, cy as f32));
        match m {
            Mouse::Move => {
                if let Some(d) = &mut self.drag {
                    let p = h.cursor();
                    if !d.moving {
                        if ((p.0 - d.from_cursor.0).powi(2) + (p.1 - d.from_cursor.1).powi(2)).sqrt() < 4.0 {
                            return;
                        }
                        d.moving = true;
                        // Drag is measured from the real cursor position, so zoom and DPI don't skew it.
                        d.from_card = self.card;
                        d.from_cursor = p;
                        h.set_cursor(CursorKind::Grabbing);
                    }
                    self.card = (d.from_card.0 + p.0 - d.from_cursor.0, d.from_card.1 + p.1 - d.from_cursor.1);
                    self.place_all(h);
                    return;
                }
                self.set_card_hover(h, hit);
            }
            Mouse::Leave => {
                if self.drag.is_none() {
                    self.set_card_hover(h, None);
                }
            }
            Mouse::Down(Btn3::Left) => {
                self.drag = Some(Drag { from_card: self.card, from_cursor: h.cursor(), moving: false, target: hit });
            }
            Mouse::Up(Btn3::Left) => {
                let Some(d) = self.drag.take() else { return };
                if d.moving {
                    self.settings.card = Some(CardPos { x: self.card.0, y: self.card.1, w: Some(self.card_size.0), h: Some(self.card_size.1) });
                    self.save();
                    self.update_anchor(h);
                    self.dirty_panel = self.selected.is_some();
                    self.dirty_toast = true;
                    self.place_all(h);
                    self.set_card_hover(h, hit);
                } else if matches!(d.target, Some(Target::More | Target::Prompt)) {
                    self.open_settings(h);
                } else {
                    let next = match d.target {
                        Some(Target::Ring(s, i)) => Some((s, i)),
                        _ => None,
                    };
                    let same = next.is_some() && next == self.selected;
                    self.select(if same { None } else { next });
                }
            }
            Mouse::Dbl(Btn3::Left) => {
                if !matches!(hit, Some(Target::Ring(..) | Target::More | Target::Prompt)) {
                    self.retry_now();
                }
            }
            // Right-click opens the app window, where all the settings and actions are.
            Mouse::Up(Btn3::Right) => self.open_settings(h),
            _ => {}
        }
    }

    fn set_card_hover(&mut self, h: &mut dyn Host, hit: Option<Target>) {
        if hit != self.hover_card {
            self.hover_card = hit;
            let title = hit.and_then(|t| self.card_layout.as_ref().and_then(|l| l.title(t, self.lang(), self.net)));
            h.set_tooltip(WinKind::Card, title);
        }
        h.set_cursor(match (self.drag.as_ref().is_some_and(|d| d.moving), hit) {
            (true, _) => CursorKind::Grabbing,
            (_, Some(Target::Ring(..) | Target::More | Target::Prompt)) => CursorKind::Pointer,
            (_, Some(_)) => CursorKind::Grab,
            _ => CursorKind::Default,
        });
        let on: Vec<Target> = hit.into_iter().filter(|t| *t != Target::Card).collect();
        self.card_hover.target(&on);
        self.card_over.target(if hit.is_some() { &[0] } else { &[] });
        self.dirty_card = true;
    }

    // ---------- panel mouse ----------

    fn panel_mouse(&mut self, h: &mut dyn Host, m: Mouse, x: f64, y: f64) {
        let (px, py) = ((x - self.pad()) / self.zoom(), (y - self.pad()) / self.zoom());
        let on_close = self.panel_layout.as_ref().is_some_and(|l| l.close.contains(px as f32, py as f32));
        match m {
            Mouse::Move | Mouse::Leave => {
                let over = on_close && m == Mouse::Move;
                if over != self.hover_panel {
                    self.hover_panel = over;
                    self.panel_hover.target(if over { &[0] } else { &[] });
                    self.dirty_panel = true;
                    h.set_tooltip(WinKind::Panel, over.then_some(i18n::t(self.lang()).close));
                }
                h.set_cursor(if over { CursorKind::Pointer } else { CursorKind::Default });
            }
            Mouse::Up(Btn3::Left) if on_close => self.select(None),
            _ => {}
        }
    }

    // ---------- toast mouse ----------

    fn toast_mouse(&mut self, h: &mut dyn Host, m: Mouse, x: f64, y: f64) {
        let (tx, ty) = ((x - self.pad()) / self.zoom(), (y - self.pad()) / self.zoom());
        let hit = self.toast_layout.as_ref().and_then(|l| l.hit(tx as f32, ty as f32));
        match m {
            Mouse::Move | Mouse::Leave => {
                let hit = if m == Mouse::Leave { None } else { hit };
                if hit != self.hover_toast {
                    self.hover_toast = hit;
                    self.toast_hover.target(&hit.into_iter().collect::<Vec<_>>());
                    self.dirty_toast = true;
                    h.set_tooltip(WinKind::Toast, (hit == Some(Btn::Close)).then_some(i18n::t(self.lang()).u.later));
                }
                h.set_cursor(if hit.is_some() { CursorKind::Pointer } else { CursorKind::Default });
            }
            Mouse::Up(Btn3::Left) => match hit {
                Some(Btn::Close) => self.update_action(h, "dismiss"),
                Some(Btn::Primary) => self.update_action(h, "download"),
                Some(Btn::Skip) => self.update_action(h, "skip"),
                None => {}
            },
            _ => {}
        }
    }

    /// From the toast or the app window: check | download | skip | dismiss.
    pub fn update_action(&mut self, h: &mut dyn Host, a: &str) {
        match a {
            "check" => self.updater.check(),
            "download" => self.updater.download(&mut |u| h.open_url(u)),
            "skip" => {
                let mut v = None;
                self.updater.skip(&mut |s| v = Some(s));
                if let Some(v) = v {
                    self.settings.skip_version = Some(v);
                    self.save();
                }
            }
            "dismiss" => self.updater.dismiss(),
            _ => {}
        }
        self.on_update_change();
    }

    fn on_update_change(&mut self) {
        self.dirty_toast = true;
        if let Some(a) = &mut self.app_win {
            a.dirty = true;
        }
    }

    /// The toast shows while an update is on offer or under way, unless it was skipped or closed, the card is
    /// hidden, or a detail panel is open in its spot. A failed check stays out of the way (the app window shows it).
    fn toast_kind(&self) -> Option<Kind> {
        let u = self.updater.view(self.settings.skip_version.as_deref());
        if !self.visible || self.selected.is_some() {
            return None;
        }
        match u.phase {
            Phase::Available if !u.skipped && !u.dismissed => Some(Kind::Available),
            Phase::Downloading => Some(Kind::Downloading),
            Phase::Installing => Some(Kind::Installing),
            Phase::Error if u.error.as_ref().is_some_and(|e| e.0 == updater::During::Download) => Some(Kind::Error),
            _ => None,
        }
    }

    // ---------- app window ----------

    /// A normal window of its own, centred on the display the card is on.
    pub fn open_settings(&mut self, h: &mut dyn Host) {
        self.select(None);
        self.startup = h.login_item();
        let title = format!("Ai Quota Overlay — {}", i18n::t(self.lang()).s.title);
        if h.app_is_open() {
            h.app_open(RectD::default(), &title);
        } else {
            let a = self.display_near(h, self.card.0.round(), self.card.1.round()).work;
            let (w0, h0) = crate::ui::app_window::SIZE;
            let w = w0.min(a.w - 2.0 * MARGIN).min((a.h - 2.0 * MARGIN) * w0 / h0).round();
            let hh = (w * h0 / w0).round();
            h.app_open(RectD::new((a.x + (a.w - w) / 2.0).round(), (a.y + (a.h - hh) / 2.0).round(), w, hh), &title);
            self.app_win = Some(AppWin::new());
        }
        if let Some(a) = &mut self.app_win {
            a.dirty = true;
        }
        self.updater.check_if_stale(updater::STALE);
        self.on_update_change();
    }

    fn close_settings(&mut self, h: &mut dyn Host) {
        h.app_close();
        self.app_pm = None;
        self.app_win = None;
    }

    fn app_mouse(&mut self, h: &mut dyn Host, m: Mouse, x: f64, y: f64) {
        let Some(mut aw) = self.app_win.take() else { return };
        let ctx = self.app_ctx();
        let actions = aw.mouse(&mut self.fonts, &ctx, m, x as f32, y as f32);
        h.set_cursor(aw.cursor());
        self.app_win = Some(aw);
        for a in actions {
            self.app_action(h, a);
        }
    }

    fn app_ctx(&self) -> app_window::Ctx {
        app_window::Ctx {
            lang: self.lang(),
            settings: self.settings.clone(),
            visible: self.visible,
            startup: self.startup,
            startup_failed: self.startup_failed,
            data: self.last.clone(),
            net: if self.settings.network { self.net } else { None },
            update: self.updater.view(self.settings.skip_version.as_deref()),
            version: env!("CARGO_PKG_VERSION").to_string(),
            login: self.login.as_ref().map(|p| p.svc),
            login_code: self.login_code.clone(),
            note: self.note.clone(),
        }
    }

    /// Every change, from the app window or the shortcut, goes through here, is saved and shown everywhere.
    fn app_action(&mut self, h: &mut dyn Host, a: AppAction) {
        match a {
            AppAction::Close => {
                self.close_settings(h);
                return;
            }
            AppAction::DragWindow => {
                h.app_drag();
                return;
            }
            AppAction::Lang(l) => {
                self.settings.lang = l.key().to_string();
                h.app_title(&format!("Ai Quota Overlay — {}", i18n::t(l).s.title));
                let m = &i18n::t(l).menu;
                h.tray_menu(m.settings, m.quit);
            }
            AppAction::Visible(on) => {
                if on != self.visible {
                    self.toggle_visible(h);
                }
                return;
            }
            AppAction::Vertical(v) => {
                self.settings.vertical = v;
                self.select(None);
            }
            AppAction::Size(k) => {
                if self.settings.size == k {
                    return;
                }
                self.settings.size = k.to_string();
                self.select(None);
                // The corner the card is anchored to stays put; then keep it on its display and remember it.
                self.layout_card();
                self.keep_on_screen(h);
                if self.settings.card.is_some() {
                    self.settings.card = Some(CardPos { x: self.card.0, y: self.card.1, w: Some(self.card_size.0), h: Some(self.card_size.1) });
                }
                self.update_anchor(h);
            }
            AppAction::Opacity(v) => {
                // Only the overlay's alpha and the slider change: the app window repaints just the slider.
                self.settings.opacity = (v.clamp(0.3, 1.0) * 100.0).round() / 100.0;
                self.dirty_card = true;
                self.dirty_panel = self.selected.is_some();
                self.dirty_toast = true;
                self.save_at = Some(Instant::now() + Duration::from_millis(500));
                return;
            }
            AppAction::Network(on) => {
                self.settings.network = on;
                self.apply_network();
            }
            AppAction::Startup(on) => {
                h.set_login_item(on);
                self.startup = h.login_item();
                // Read back: a blocked write leaves it as it was, which the app window then says.
                self.startup_failed = self.startup != Some(on);
            }
            AppAction::Service(id, on) => {
                if on {
                    self.settings.services.remove(&id);
                } else {
                    self.settings.services.insert(id.clone(), false);
                }
                if self.selected.is_some_and(|(x, _)| x == aid(&id)) {
                    self.select(None);
                }
                self.refresh(true);
            }
            AppAction::Window(id, win, on) => {
                let hidden = self.settings.hidden.entry(id).or_default();
                hidden.retain(|x| *x != win);
                if !on {
                    hidden.push(win);
                }
            }
            AppAction::SignIn(svc) => {
                self.sign_in(h, svc);
                if let Some(a) = &mut self.app_win {
                    a.dirty = true;
                }
                return;
            }
            AppAction::CancelSignIn => {
                self.login = None;
                self.login_code = None;
                if let Some(a) = &mut self.app_win {
                    a.dirty = true;
                }
                return;
            }
            AppAction::CopyLink => {
                // The code where there is one (the page needs it), else the page's address.
                if let Some(p) = &self.login {
                    h.copy_text(self.login_code.as_deref().unwrap_or(&p.url));
                }
                return;
            }
            AppAction::SignOut(id) => {
                self.sign_out(h, &id);
            }
            AppAction::MoveTo(id, to) => {
                let _ = crate::quota::store::move_to(&id, to);
                // The card and the list follow at once, not at the next check.
                if let Some(l) = &mut self.last {
                    if let Some(i) = l.accts.iter().position(|(a, _)| a.id == id) {
                        let item = l.accts.remove(i);
                        l.accts.insert(to.min(l.accts.len()), item);
                    }
                }
            }
            AppAction::Refresh => {
                self.retry_now();
                return;
            }
            AppAction::Snap => {
                self.reset_position(h);
                return;
            }
            AppAction::Restart => return self.restart(h),
            AppAction::Quit => return self.quit(h),
            AppAction::Update(u) => {
                self.update_action(h, u);
                return;
            }

        }
        self.save();
        self.dirty_card = true;
        self.dirty_toast = true;
        if let Some(a) = &mut self.app_win {
            a.dirty = true;
        }
        self.update_tooltip(h);
    }

    // ---------- drawing ----------

    fn card_in<'a>(settings: &'a Settings, last: &'a Option<Snapshot>, net: Option<NetState>, selected: Option<(Aid, i32)>) -> CardIn<'a> {
        CardIn {
            lang: Lang::from_key(&settings.lang).unwrap_or(Lang::En),
            vertical: settings.vertical,
            network_on: settings.network,
            settings,
            data: last.as_ref(),
            net,
            selected,
            now: now_ms(),
        }
    }

    /// Lays the card out; when its size changes (new data, rotation) the anchored corner stays where it is.
    fn layout_card(&mut self) {
        let inp = Self::card_in(&self.settings, &self.last, self.net, self.selected);
        let l = card::layout(&mut self.fonts, &inp);
        // A selection whose ring went away closes.
        if let Some((s, i)) = self.selected {
            if !l.rings().contains(&Target::Ring(s, i)) && self.last.is_some() {
                self.selected = None;
                self.dirty_panel = true;
                self.dirty_toast = true;
            }
        }
        let size = (((l.w as f64) * self.zoom()).ceil(), ((l.h as f64) * self.zoom()).ceil());
        if size != self.card_size {
            if self.anchor_right {
                self.card.0 += self.card_size.0 - size.0;
            }
            if self.anchor_bottom {
                self.card.1 += self.card_size.1 - size.1;
            }
            self.card_size = size;
            self.dirty_panel = self.selected.is_some();
            self.dirty_toast = true;
        }
        self.card_layout = Some(l);
    }

    fn render_card(&mut self, h: &dyn Host) -> Option<Pixmap> {
        self.layout_card();
        let l = self.card_layout.take()?;
        let r = self.around(self.card, self.card_size);
        let (scale, pw, ph) = h.device_size(WinKind::Card, r);
        let zoom = self.zoom();
        let mut pm = Pixmap::new(pw.max(1), ph.max(1))?;
        let fx = CardFx { hover: self.card_hover.map(), card_hover: self.card_over.get(0), alarm_s: self.started.elapsed().as_secs_f32() };
        {
            let inp = Self::card_in(&self.settings, &self.last, self.net, self.selected);
            let mut c = Canvas::new(&mut pm, (scale * zoom) as f32, PAD_CSS as f32, PAD_CSS as f32);
            card::draw(&mut c, &mut self.fonts, &l, &inp, &fx);
        }
        self.card_layout = Some(l);
        Some(pm)
    }

    fn place_all(&mut self, h: &mut dyn Host) {
        self.dirty_card = true;
        self.dirty_panel = self.selected.is_some();
        self.dirty_toast = true;
        self.flush(h);
    }

    fn alpha(&self) -> f64 {
        self.settings.opacity
    }

    /// Redraws whatever changed and schedules the next timer.
    pub fn flush(&mut self, h: &mut dyn Host) {
        if self.quitting {
            return;
        }
        let now = Instant::now();
        let gap = now.duration_since(self.last_frame);
        let dt = gap.as_secs_f32().min(0.1);
        self.last_frame = now;
        let mut painted: Option<&'static str> = None;
        // A changed part of the app window waiting for its frame.
        let mut app_due: Option<Instant> = None;
        let mut animating = false;
        if self.card_hover.step(dt) | self.card_over.step(dt) {
            animating = true;
            self.dirty_card = true;
        }
        if self.panel_hover.step(dt) {
            animating = true;
            self.dirty_panel = true;
        }
        if self.toast_hover.step(dt) {
            animating = true;
            self.dirty_toast = true;
        }
        let offline = self.settings.network && self.net.is_some_and(|n| !n.online) && self.visible;
        if offline {
            animating = true;
            self.dirty_card = true;
        }

        // Card.
        if self.visible && self.dirty_card {
            self.dirty_card = false;
            if let Some(pm) = self.render_card(h) {
                let r = self.around(self.card, self.card_size);
                h.present(WinKind::Card, r, &pm, self.alpha(), false);
            }
        }

        // Panel.
        if self.dirty_panel {
            self.dirty_panel = false;
            animating |= self.draw_panel(h);
        } else if self.panel_pop.is_some() {
            animating |= self.draw_panel(h);
        }

        // Toast.
        let kind = self.toast_kind();
        if kind.is_some() && !self.toast_shown {
            self.toast_pop = Some(now);
            self.toast_t0 = now;
        }
        if let Some(k) = kind {
            if matches!(k, Kind::Downloading | Kind::Installing) {
                animating = true;
                self.dirty_toast = true;
            }
            let target = self.updater.view(None).percent as f32;
            if (self.toast_bar - target).abs() > 0.1 {
                // transition: width .25s ease-out
                self.toast_bar += (target - self.toast_bar) * (dt / 0.25).min(1.0) * 2.0;
                if (self.toast_bar - target).abs() < 0.5 {
                    self.toast_bar = target;
                }
                animating = true;
                self.dirty_toast = true;
            }
        }
        if self.toast_pop.is_some() {
            self.dirty_toast = true;
        }
        if self.dirty_toast {
            self.dirty_toast = false;
            animating |= self.draw_toast(h, kind);
        }

        // App window.
        if let Some(mut aw) = self.app_win.take() {
            if !h.app_is_open() {
                // Closed by the platform.
            } else {
                let ctx = self.app_ctx();
                if aw.step(dt) {
                    animating = true;
                }
                // A changed part is painted at most once a frame: the mouse can send several moves per frame (a
                // card being dragged), and painting each of them only falls behind. What changed meanwhile adds up.
                let frame_due = now.duration_since(self.app_painted) >= FRAME - Duration::from_millis(2);
                if aw.damage.is_some() && !aw.dirty && !frame_due {
                    app_due = Some(self.app_painted + FRAME);
                } else if aw.dirty || aw.damage.is_some() {
                    self.app_painted = now;
                    if let Some((w, hh, scale)) = h.app_size() {
                        let pw = (w * scale).round().max(1.0) as u32;
                        let ph = (hh * scale).round().max(1.0) as u32;
                        let same = self.app_pm.as_ref().is_some_and(|(p, s)| p.width() == pw && p.height() == ph && *s == scale);
                        let damage = aw.damage.take();
                        if aw.dirty || !same {
                            painted = Some("full");
                            aw.dirty = false;
                            if let Some(mut pm) = Pixmap::new(pw, ph) {
                                {
                                    let mut c = Canvas::new(&mut pm, scale as f32, 0.0, 0.0);
                                    aw.draw(&mut c, &mut self.fonts, &ctx, w as f32, hh as f32);
                                }
                                h.app_present(&pm);
                                self.app_pm = Some((pm, scale));
                            }
                        } else if let (Some(d), Some((pm, _))) = (damage, self.app_pm.as_mut()) {
                            // Paint only the changed box: the whole layout runs (it is cheap) but every shape
                            // outside the small canvas is skipped, then the box is copied in and shown.
                            let s = scale as f32;
                            let x0 = ((d.x * s).floor() as i32).clamp(0, pw as i32);
                            let y0 = ((d.y * s).floor() as i32).clamp(0, ph as i32);
                            let x1 = (((d.x + d.w) * s).ceil() as i32).clamp(0, pw as i32);
                            let y1 = (((d.y + d.h) * s).ceil() as i32).clamp(0, ph as i32);
                            if x1 > x0 && y1 > y0 {
                                painted = Some("part");
                                let (bw, bh) = ((x1 - x0) as u32, (y1 - y0) as u32);
                                // A few px of margin: anti-aliasing right at a canvas edge comes out a shade
                                // different, so the edge rows are painted but not kept.
                                const M: u32 = 4;
                                if let Some(mut part) = Pixmap::new(bw + 2 * M, bh + 2 * M) {
                                    {
                                        let mut c = Canvas::new(&mut part, s, 0.0, 0.0);
                                        c.base = c.base.post_translate((M as i32 - x0) as f32, (M as i32 - y0) as f32);
                                        aw.draw_part(&mut c, &mut self.fonts, &ctx, w as f32, hh as f32);
                                    }
                                    copy_into(pm, &part, (M, M, bw, bh), x0 as u32, y0 as u32);
                                    h.app_present_rect(pm, (x0 as u32, y0 as u32, bw, bh));
                                }
                            }
                        }
                    }
                }
                self.app_win = Some(aw);
            }
        }

        if let Some(kind) = painted {
            log_frame(gap, kind, now.elapsed());
        }
        if self.save_at.is_some_and(|t| now >= t) {
            self.save_at = None;
            self.save();
        }

        // Next wake-up.
        let mut next = if animating { FRAME } else { Duration::from_secs(5) };
        let mut min_at = |t: Instant| {
            let d = t.saturating_duration_since(Instant::now());
            if d < next {
                next = d;
            }
        };
        min_at(self.next_relabel);
        if let Some(t) = app_due {
            min_at(t);
        }
        if self.settings.network && self.visible {
            min_at(self.next_net);
        }
        if let Some(t) = self.resume_at {
            min_at(t);
        }
        if let Some(t) = self.save_at {
            min_at(t);
        }
        if let Some(t) = self.updater.next_due() {
            min_at(t);
        }
        if let Some(t) = self.last_fetch {
            let gap = if self.on_screen(h) { POLL } else { IDLE_POLL };
            min_at(t + gap);
        }
        h.set_timer(next.max(Duration::from_millis(1)));
    }

    /// Draws or hides the panel; true while its pop animation runs.
    fn draw_panel(&mut self, h: &mut dyn Host) -> bool {
        let Some((x, index)) = self.selected else {
            if self.panel_shown {
                self.panel_shown = false;
                h.hide(WinKind::Panel);
            }
            self.panel_pop = None;
            return false;
        };
        let Some((acct, q)) = self.last.as_ref().and_then(|l| l.by_aid(x)).cloned() else { return false };
        let several = self.last.as_ref().is_some_and(|l| l.count(acct.svc) > 1);
        let lang = self.lang();
        let inp = PanelIn { lang, svc: acct.svc, who: (several && !acct.who.is_empty()).then_some(acct.who.as_str()), q: &q, index, now: now_ms() };
        let l = panel::layout(&mut self.fonts, &inp);
        let size = ((panel::W as f64 * self.zoom()).ceil(), (l.h as f64 * self.zoom()).ceil());
        self.panel_size = size;
        let r = self.around(self.popup_pos(size), size);
        let (scale, pw, ph) = h.device_size(WinKind::Panel, r);
        let zoom = self.zoom();
        let Some(mut pm) = Pixmap::new(pw.max(1), ph.max(1)) else { return false };
        // @keyframes pop .14s ease-out: from opacity 0, scale .97.
        let (k, animating) = match self.panel_pop {
            Some(t) => {
                let p = (t.elapsed().as_secs_f32() / 0.14).min(1.0);
                if p >= 1.0 {
                    self.panel_pop = None;
                }
                (crate::ui::ease_out(p), p < 1.0)
            }
            None => (1.0, false),
        };
        {
            let fx = PanelFx { close_hover: self.panel_hover.get(0) };
            let mut c = Canvas::new(&mut pm, (scale * zoom) as f32, PAD_CSS as f32, PAD_CSS as f32);
            let (cx, cy) = (panel::W / 2.0, l.h / 2.0);
            let fonts = &mut self.fonts;
            if k < 1.0 {
                c.layer(k, crate::ui::gfx::Filter::NONE, Some((0.97 + 0.03 * k, cx, cy)), 0.0, |c| panel::draw(c, fonts, &l, &inp, &fx));
            } else {
                panel::draw(&mut c, fonts, &l, &inp, &fx);
            }
        }
        let activate = !self.panel_shown;
        self.panel_shown = true;
        h.present(WinKind::Panel, r, &pm, self.alpha(), activate);
        self.panel_layout = Some(l);
        animating
    }

    /// Draws or hides the update toast; true while it animates.
    fn draw_toast(&mut self, h: &mut dyn Host, kind: Option<Kind>) -> bool {
        let Some(kind) = kind else {
            if self.toast_shown {
                self.toast_shown = false;
                h.hide(WinKind::Toast);
            }
            self.toast_pop = None;
            return false;
        };
        let v = self.updater.view(self.settings.skip_version.as_deref());
        let error = v.error.as_ref().map(|e| e.1.clone()).unwrap_or_default();
        let version = v.version.clone().unwrap_or_default();
        let inp = ToastIn {
            lang: self.lang(),
            kind,
            current: &v.current,
            version: &version,
            percent: v.percent,
            manual: v.manual,
            error: &error,
        };
        let l = toast::layout(&mut self.fonts, &inp);
        let size = ((toast::W as f64 * self.zoom()).ceil(), (l.h as f64 * self.zoom()).ceil());
        self.toast_size = size;
        let r = self.around(self.popup_pos(size), size);
        let (scale, pw, ph) = h.device_size(WinKind::Toast, r);
        let zoom = self.zoom();
        let Some(mut pm) = Pixmap::new(pw.max(1), ph.max(1)) else { return false };
        // @keyframes pop .18s ease-out: from opacity 0, 4 px lower, scale .98.
        let (k, animating) = match self.toast_pop {
            Some(t) => {
                let p = (t.elapsed().as_secs_f32() / 0.18).min(1.0);
                if p >= 1.0 {
                    self.toast_pop = None;
                }
                (crate::ui::ease_out(p), p < 1.0)
            }
            None => (1.0, false),
        };
        {
            let fx = ToastFx {
                hover_close: self.toast_hover.get(Btn::Close),
                hover_primary: self.toast_hover.get(Btn::Primary),
                hover_skip: self.toast_hover.get(Btn::Skip),
                t: self.toast_t0.elapsed().as_secs_f32(),
                bar: self.toast_bar,
            };
            let mut c = Canvas::new(&mut pm, (scale * zoom) as f32, PAD_CSS as f32, PAD_CSS as f32);
            let fonts = &mut self.fonts;
            let (cx, cy) = (toast::W / 2.0, l.h / 2.0);
            if k < 1.0 {
                c.layer(k, crate::ui::gfx::Filter::NONE, Some((0.98 + 0.02 * k, cx, cy)), 4.0 * (1.0 - k), |c| toast::draw(c, fonts, &l, &inp, &fx));
            } else {
                toast::draw(&mut c, fonts, &l, &inp, &fx);
            }
        }
        self.toast_shown = true;
        h.present(WinKind::Toast, r, &pm, self.alpha(), false);
        self.toast_layout = Some(l);
        animating
    }
}

/// Copies the box (x, y, w, h) of `part` into `pm` at (to_x, to_y) device px, replacing what was there.
fn copy_into(pm: &mut Pixmap, part: &Pixmap, (x, y, w, h): (u32, u32, u32, u32), to_x: u32, to_y: u32) {
    let (pw, sw) = (pm.width() as usize, part.width() as usize);
    let rows = h.min(pm.height().saturating_sub(to_y)).min(part.height().saturating_sub(y)) as usize;
    let cols = w.min(pm.width().saturating_sub(to_x)).min(part.width().saturating_sub(x)) as usize;
    let src = part.data();
    let dst = pm.data_mut();
    for r in 0..rows {
        let d = ((to_y as usize + r) * pw + to_x as usize) * 4;
        let s0 = ((y as usize + r) * sw + x as usize) * 4;
        dst[d..d + cols * 4].copy_from_slice(&src[s0..s0 + cols * 4]);
    }
}

/// Dev aid: QUOTA_OVERLAY_LOG_FRAMES=<file> appends "<ms since the last frame> <full|part> <ms this frame took>" for
/// every frame that repainted the app window, to check animation smoothness.
fn log_frame(gap: Duration, kind: &str, took: Duration) {
    use std::io::Write;
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    let Some(path) = PATH.get_or_init(|| std::env::var_os("QUOTA_OVERLAY_LOG_FRAMES").map(Into::into)) else { return };
    let line = format!("{:.1} {kind} {:.2}
", gap.as_secs_f64() * 1000.0, took.as_secs_f64() * 1000.0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
