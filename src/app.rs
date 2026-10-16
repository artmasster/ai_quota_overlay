//! The app (port of src/main.js and the pages' event handling): windows, layout and anchoring, drag, polling,
//! back-off, the internet check, settings, tray, shortcut and updates. Platform code (src/platform) owns the real
//! windows and calls `App::event`; the app answers through the `Host` trait.
//!
//! Quota: checked every POLL while the card or the app window is on screen, every IDLE_POLL while both are hidden.
//! Checks never overlap, manual refreshes closer than MIN_REFRESH_GAP reuse the last result, and a service that
//! answers "too many requests" is left alone for BACKOFF (doubling up to MAX_BACKOFF) while it keeps showing its
//! last good numbers, so checking often never turns into hammering anyone's API.
use crate::anim::Fades;
use crate::i18n::{self, Lang};
use crate::model::{now_ms, NetState, Snapshot, Svc, SERVICES};
use crate::settings::{CardPos, Settings};
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
const MIN_REFRESH_GAP: Duration = Duration::from_secs(10);
const BACKOFF_MS: i64 = 2 * 60 * 1000;
const MAX_BACKOFF_MS: i64 = 15 * 60 * 1000;
/// Internet check: online when any of these answers within NET_TIMEOUT. Any HTTP status counts, only a failed
/// connection doesn't, so a captive portal or a blocked host still reads as online as long as one gets through.
const NET_EVERY: Duration = Duration::from_secs(3);
const NET_TIMEOUT: Duration = Duration::from_millis(2500);
const NET_URLS: [&str; 2] = ["https://www.gstatic.com/generate_204", "https://1.1.1.1/cdn-cgi/trace"];
const MARGIN: f64 = 16.0;
/// The whole overlay is drawn at this zoom: CSS px × SCALE = DIP.
pub const SCALE: f64 = 1.2;
/// The card and the panel sit PAD inside their windows (room for the panel's outline) and GAP apart on screen.
const PAD: f64 = 2.0 * SCALE;
const GAP: f64 = 4.0 * SCALE;
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
    #[allow(dead_code)] // macOS / Linux
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
    fn app_title(&mut self, title: &str);
    /// Starts moving the app window with the mouse (from the custom title bar).
    fn app_drag(&mut self);
    fn tray_tooltip(&mut self, text: &str);
    fn tray_menu(&mut self, open: &str, quit: &str);
    /// Launch at startup: None where it isn't supported (Linux).
    fn login_item(&self) -> Option<bool>;
    fn set_login_item(&mut self, on: bool);
    fn open_url(&mut self, url: &str);
    fn quit(&mut self);
    fn relaunch(&mut self);
    /// The next `Ev::Timer` comes after `after` (replacing any earlier request).
    fn set_timer(&mut self, after: Duration);
}

pub enum Msg {
    Quota(Snapshot),
    Net(bool, u64),
    Update(UpMsg),
}

pub type Fetcher = fn([bool; 3]) -> Snapshot;

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
    cooling: [Option<(i64, i64)>; 3],
    net_busy: bool,
    net: Option<NetState>,
    next_net: Instant,
    resume_at: Option<Instant>,
    next_relabel: Instant,

    card: (f64, f64),
    card_size: (f64, f64),
    panel_size: (f64, f64),
    toast_size: (f64, f64),
    selected: Option<(Svc, i32)>,
    anchor_right: bool,
    anchor_bottom: bool,
    drag: Option<Drag>,
    visible: bool,
    panel_shown: bool,
    toast_shown: bool,
    loading: bool,

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
            cooling: [None; 3],
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
            loading: false,
            card_hover: Fades::new(0.12),
            card_over: Fades::new(0.15),
            panel_hover: Fades::new(0.12),
            toast_hover: Fades::new(0.12),
            panel_pop: None,
            toast_pop: None,
            toast_t0: now,
            toast_bar: 0.0,
            last_frame: now,
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

    /// The panel opens toward the middle of the display the card is on.
    fn update_anchor(&mut self, h: &dyn Host) {
        let cx = self.card.0 + self.card_size.0 / 2.0;
        let cy = self.card.1 + self.card_size.1 / 2.0;
        let a = self.display_near(h, cx.round(), cy.round()).work;
        self.anchor_right = cx > a.x + a.w / 2.0;
        self.anchor_bottom = cy > a.y + a.h / 2.0;
    }

    /// Window bounds for a box at `pos` of `size`, with PAD to spare around it.
    fn around(pos: (f64, f64), size: (f64, f64)) -> RectD {
        RectD::new((pos.0 - PAD).round(), (pos.1 - PAD).round(), (size.0 + 2.0 * PAD).ceil(), (size.1 + 2.0 * PAD).ceil())
    }

    /// Above/below the card when horizontal, beside it when vertical, aligned to the card edge facing the screen
    /// border. The detail panel and the update toast both open there (the toast steps aside while a panel is open).
    fn popup_pos(&self, size: (f64, f64)) -> (f64, f64) {
        let (c, cs) = (self.card, self.card_size);
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
            }
        }
    }

    fn tick(&mut self, h: &mut dyn Host) {
        let now = Instant::now();
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
        self.loading = true;
        self.dirty_card = true;
        self.last_fetch = Some(Instant::now());
        let now = now_ms();
        let mut enabled = self.settings.services;
        for s in SERVICES {
            if self.cooling[s.index()].is_some_and(|(until, _)| until > now) {
                enabled[s.index()] = false;
            }
        }
        let fetch = self.fetcher;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let snap = fetch(enabled);
            let _ = tx.send(Msg::Quota(snap));
            waker();
        });
    }

    fn on_quota(&mut self, h: &mut dyn Host, mut fresh: Snapshot) {
        let now = now_ms();
        let started = now - self.last_fetch.map(|t| t.elapsed().as_millis() as i64).unwrap_or(0);
        for s in SERVICES {
            let i = s.index();
            let cooling = self.cooling[i].is_some_and(|(until, _)| until > started);
            if !self.settings.services[i] {
                self.cooling[i] = None;
            } else if cooling {
                if let Some(prev) = self.last.as_ref().and_then(|l| l.svc[i].clone()) {
                    fresh.svc[i] = Some(prev);
                }
            } else if fresh.svc[i].as_ref().is_some_and(|q| q.rate_limited()) {
                let ms = (self.cooling[i].map(|(_, ms)| ms).unwrap_or(BACKOFF_MS / 2) * 2).min(MAX_BACKOFF_MS);
                self.cooling[i] = Some((started + ms, ms));
                if let Some(prev) = self.last.as_ref().and_then(|l| l.svc[i].clone()).filter(|q| q.ok) {
                    fresh.svc[i] = Some(prev);
                }
            } else {
                self.cooling[i] = None;
            }
        }
        self.last = Some(fresh);
        self.refreshing = false;
        self.loading = false;
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

    /// Remember each service's windows, so the app window can still list them while it's switched off or offline.
    fn remember_windows(&mut self) {
        let Some(last) = &self.last else { return };
        let mut changed = false;
        for s in SERVICES {
            let Some(q) = last.get(s) else { continue };
            if !q.ok || q.windows.is_empty() {
                continue;
            }
            let seen: Vec<_> = q.windows.iter().map(|w| w.seen()).collect();
            if self.settings.seen[s.index()].as_ref() == Some(&seen) {
                continue;
            }
            self.settings.seen[s.index()] = Some(seen);
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
        for s in SERVICES {
            let Some(q) = last.get(s) else { continue };
            if q.absent {
                continue;
            }
            if !q.ok {
                lines.push(format!("{}: {}", s.name(), q.error.as_ref().map(|e| i18n::error_text(e, lang)).unwrap_or_default()));
                continue;
            }
            let hidden = &self.settings.hidden[s.index()];
            let parts: Vec<String> = q
                .windows
                .iter()
                .filter(|w| !hidden.contains(&w.id))
                .map(|w| format!("{} {}%", i18n::window_label(w, lang, true), crate::ui::js_round(100.0 - w.used())))
                .collect();
            lines.push(format!("{} {}", s.name(), parts.join(" · ")));
        }
        let mut text = lines.join("\n");
        if text.is_empty() {
            text = i18n::t(lang).menu.not_logged_in.to_string();
        }
        // Tray tooltips hold 127 characters.
        let cut = String::from_utf16_lossy(&text.encode_utf16().take(127).collect::<Vec<u16>>());
        h.tray_tooltip(&cut);
    }

    /// Debug aid: save a PNG of the card after the first fetch, then exit.
    fn snapshot(&mut self, h: &mut dyn Host, path: &str) {
        if let Some(pm) = self.render_card(h) {
            let _ = pm.save_png(path);
        }
        self.quit(h);
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
            a.dirty = true;
        }
        if online && was_offline {
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
    fn select(&mut self, next: Option<(Svc, i32)>) {
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
        self.save();
        self.quitting = true;
        h.quit();
    }

    fn restart(&mut self, h: &mut dyn Host) {
        self.save();
        self.quitting = true;
        h.relaunch();
    }

    // ---------- card mouse ----------

    fn card_mouse(&mut self, h: &mut dyn Host, m: Mouse, x: f64, y: f64) {
        let (cx, cy) = ((x - PAD) / SCALE, (y - PAD) / SCALE);
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
                } else if d.target == Some(Target::More) {
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
                if !matches!(hit, Some(Target::Ring(..)) | Some(Target::More)) {
                    self.refresh(false);
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
            (_, Some(Target::Ring(..)) | Some(Target::More)) => CursorKind::Pointer,
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
        let (px, py) = ((x - PAD) / SCALE, (y - PAD) / SCALE);
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
        let (tx, ty) = ((x - PAD) / SCALE, (y - PAD) / SCALE);
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
        self.app_win = None;
    }

    fn app_mouse(&mut self, h: &mut dyn Host, m: Mouse, x: f64, y: f64) {
        let Some(mut aw) = self.app_win.take() else { return };
        let ctx = self.app_ctx(h);
        let actions = aw.mouse(&mut self.fonts, &ctx, m, x as f32, y as f32);
        h.set_cursor(aw.cursor());
        self.app_win = Some(aw);
        for a in actions {
            self.app_action(h, a);
        }
    }

    fn app_ctx(&self, h: &dyn Host) -> app_window::Ctx {
        app_window::Ctx {
            lang: self.lang(),
            settings: self.settings.clone(),
            visible: self.visible,
            startup: h.login_item(),
            data: self.last.clone(),
            net: if self.settings.network { self.net } else { None },
            update: self.updater.view(self.settings.skip_version.as_deref()),
            version: env!("CARGO_PKG_VERSION").to_string(),
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
            AppAction::Opacity(v) => {
                self.settings.opacity = (v.clamp(0.3, 1.0) * 100.0).round() / 100.0;
                self.dirty_card = true;
                self.dirty_panel = self.selected.is_some();
                self.dirty_toast = true;
            }
            AppAction::Network(on) => {
                self.settings.network = on;
                self.apply_network();
            }
            AppAction::Startup(on) => {
                h.set_login_item(on);
            }
            AppAction::Service(s, on) => {
                self.settings.services[s.index()] = on;
                if self.selected.is_some_and(|(x, _)| x == s) {
                    self.select(None);
                }
                self.refresh(true);
            }
            AppAction::Window(s, id, on) => {
                let hidden = &mut self.settings.hidden[s.index()];
                hidden.retain(|x| *x != id);
                if !on {
                    hidden.push(id);
                }
            }
            AppAction::Refresh => {
                self.refresh(false);
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

    fn card_in<'a>(settings: &'a Settings, last: &'a Option<Snapshot>, net: Option<NetState>, selected: Option<(Svc, i32)>) -> CardIn<'a> {
        CardIn {
            lang: Lang::from_key(&settings.lang).unwrap_or(Lang::En),
            vertical: settings.vertical,
            network_on: settings.network,
            services_on: settings.services,
            hidden: [&settings.hidden[0], &settings.hidden[1], &settings.hidden[2]],
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
        let size = (((l.w as f64) * SCALE).ceil(), ((l.h as f64) * SCALE).ceil());
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
        let r = Self::around(self.card, self.card_size);
        let (scale, pw, ph) = h.device_size(WinKind::Card, r);
        let mut pm = Pixmap::new(pw.max(1), ph.max(1))?;
        let fx = CardFx {
            hover: self.card_hover.map(),
            card_hover: self.card_over.get(0),
            loading: self.loading,
            alarm_s: self.started.elapsed().as_secs_f32(),
        };
        {
            let inp = Self::card_in(&self.settings, &self.last, self.net, self.selected);
            let mut c = Canvas::new(&mut pm, (scale * SCALE) as f32, 2.0, 2.0);
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
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
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
                let r = Self::around(self.card, self.card_size);
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
                let ctx = self.app_ctx(h);
                if aw.step(dt) {
                    animating = true;
                }
                if aw.dirty {
                    aw.dirty = false;
                    if let Some((w, hh, scale)) = h.app_size() {
                        let pw = (w * scale).round().max(1.0) as u32;
                        let ph = (hh * scale).round().max(1.0) as u32;
                        if let Some(mut pm) = Pixmap::new(pw, ph) {
                            {
                                let mut c = Canvas::new(&mut pm, scale as f32, 0.0, 0.0);
                                aw.draw(&mut c, &mut self.fonts, &ctx, w as f32, hh as f32);
                            }
                            h.app_present(&pm);
                        }
                    }
                }
                self.app_win = Some(aw);
            }
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
        if self.settings.network && self.visible {
            min_at(self.next_net);
        }
        if let Some(t) = self.resume_at {
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
        let Some((svc, index)) = self.selected else {
            if self.panel_shown {
                self.panel_shown = false;
                h.hide(WinKind::Panel);
            }
            self.panel_pop = None;
            return false;
        };
        let Some(q) = self.last.as_ref().and_then(|l| l.get(svc)).cloned() else { return false };
        let lang = self.lang();
        let inp = PanelIn { lang, svc, q: &q, index, now: now_ms() };
        let l = panel::layout(&mut self.fonts, &inp);
        let size = ((panel::W as f64 * SCALE).ceil(), (l.h as f64 * SCALE).ceil());
        self.panel_size = size;
        let r = Self::around(self.popup_pos(size), size);
        let (scale, pw, ph) = h.device_size(WinKind::Panel, r);
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
            let mut c = Canvas::new(&mut pm, (scale * SCALE) as f32, 2.0, 2.0);
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
        let size = ((toast::W as f64 * SCALE).ceil(), (l.h as f64 * SCALE).ceil());
        self.toast_size = size;
        let r = Self::around(self.popup_pos(size), size);
        let (scale, pw, ph) = h.device_size(WinKind::Toast, r);
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
            let mut c = Canvas::new(&mut pm, (scale * SCALE) as f32, 2.0, 2.0);
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
