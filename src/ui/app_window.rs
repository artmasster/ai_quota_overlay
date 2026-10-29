//! The app window (src/settings.html): custom dark title bar, General / Internet status / Services / Updates,
//! the action buttons and the footer. Laid out and painted in one pass; the pass records hit rectangles.
//! Services lists accounts: each service's own login on this machine and the ones signed in here, with buttons
//! to sign in to more and to sign out.
use super::gfx::{linear, Canvas, Fill, Filter, Rgba, R};
use super::svg::Icon;
use super::text::{Fonts, Style};
use super::{icons, js_round, level_color, svc_color, ACCENT, ACCENT2, BAD, FAINT, MUTED, OK, TEXT, WARN};
use crate::anim::Fades;
use crate::app::{Btn3, CursorKind, Mouse, Note};
use crate::i18n::{self, Lang, LANGS};
use crate::model::{Acct, NetState, Quota, Snapshot, Svc, Win, SERVICES};
use crate::settings::Settings;
use crate::updater::{During, Phase, View};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

const BG: Rgba = Rgba::hex(0x111318);
const SURFACE: Rgba = Rgba::hex(0x171a21);
const SURFACE2: Rgba = Rgba::hex(0x1d2029);
const LINE: Rgba = Rgba::rgba(255, 255, 255, 0.07);
const LINE2: Rgba = Rgba::rgba(255, 255, 255, 0.12);
const TITLE_H: f32 = 46.0;
/// Space above a section heading (the first one sits a little lower than this below the title bar).
const GAP: f32 = 14.0;
/// The window's size in DIP: 16:9, not resizable, everything visible without scrolling. (On a work area too
/// small for it the window shrinks, keeping 16:9, and the content scrolls.)
pub const SIZE: (f64, f64) = (1120.0, 630.0);
const GUTTER: f32 = 10.0;
/// A sign-out click waits this long for the second, confirming one; "Copied" shows this long.
const SURE_FOR: Duration = Duration::from_secs(4);
const COPIED_FOR: Duration = Duration::from_millis(1500);

#[derive(Clone)]
pub struct Ctx {
    pub lang: Lang,
    pub settings: Settings,
    pub visible: bool,
    pub startup: Option<bool>,
    /// Switching "launch at startup" didn't take.
    pub startup_failed: bool,
    pub data: Option<Snapshot>,
    pub net: Option<NetState>,
    pub update: View,
    pub version: String,
    /// The service a sign-in is under way for, and the code to type on its page (GitHub).
    pub login: Option<Svc>,
    pub login_code: Option<String>,
    pub note: Option<Note>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Close,
    TitleBar,
    Visible,
    Lang(Lang),
    Layout(bool),
    Size(&'static str),
    Slider,
    Startup,
    Network,
    /// An account's switch, by id.
    Acct(String),
    /// Anywhere else on an account's card: it can be dragged to another place in the list.
    Card(String),
    /// Account id and window id.
    Chip(String, String),
    /// "+ Claude" by the Services heading.
    Add(Svc),
    /// The button of an account with no login, or one that ran out: service and account id.
    SignIn(Svc, String),
    /// Sign out of an account signed in here, or take a service's own login out of this app.
    SignOut(String),
    CancelLogin,
    CopyLink,
    Upd,
    Refresh,
    Snap,
    Restart,
    Quit,
    Thumb,
}

impl Hit {
    fn pointer(&self) -> bool {
        !matches!(self, Hit::TitleBar | Hit::Thumb | Hit::Card(_))
    }
}

/// An account card held with the mouse (it moves once the pointer has gone a few px).
struct CardDrag {
    id: String,
    /// Pointer y when pressed, now, and its distance below the card's top.
    from_y: f32,
    y: f32,
    grab_dy: f32,
    moving: bool,
}

/// Space between account cards.
const CARD_GAP: f32 = 10.0;
/// The held card's shadow, and the room around it in its picture.
const LIFT_SHADOW: Rgba = Rgba::rgba(0, 0, 0, 0.55);
const LIFT_PAD: f32 = 40.0;

/// A segmented control's selected pill, sliding toward the chosen option: its left edge (from the control's) and
/// width now and where they go, and the control's box (repainted while it moves).
struct Pill {
    x: f32,
    w: f32,
    tx: f32,
    tw: f32,
    area: R,
}

/// Pictures for painting the account list while cards move (device px at `scale`): the window without the cards,
/// each card, and the held card lifted with its shadow.
struct Pics {
    scale: f32,
    window: Pixmap,
    cards: HashMap<String, Pixmap>,
    lifted: Option<(String, Pixmap)>,
}

pub enum Action {
    Close,
    DragWindow,
    Lang(Lang),
    Visible(bool),
    Vertical(bool),
    Size(&'static str),
    Opacity(f64),
    Network(bool),
    Startup(bool),
    /// Account id, on.
    Service(String, bool),
    /// Account id, window id, shown.
    Window(String, String, bool),
    SignIn(Svc),
    SignOut(String),
    /// Account id and its new place in the list.
    MoveTo(String, usize),
    CancelSignIn,
    CopyLink,
    Refresh,
    Snap,
    Restart,
    Quit,
    Update(&'static str),
}

/// Switches by key, for the knob animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Sw {
    Visible,
    Startup,
    Network,
    /// An account's switch, by the hit key of its `Hit::Acct`.
    Acct(u64),
}

impl Sw {
    fn key(self) -> u64 {
        match self {
            Sw::Visible => hkey(&Hit::Visible),
            Sw::Startup => hkey(&Hit::Startup),
            Sw::Network => hkey(&Hit::Network),
            Sw::Acct(k) => k,
        }
    }
}

/// What the Services section lists: the accounts signed in here, by service (none before the first check).
fn account_rows(ctx: &Ctx) -> Vec<(Acct, Quota)> {
    ctx.data.as_ref().map(|d| d.accts.clone()).unwrap_or_default()
}

pub struct AppWin {
    /// Everything needs painting again.
    pub dirty: bool,
    /// Only this part (DIP) changed: a hover fade, a switch knob, the slider, a spinner, the ping.
    pub damage: Option<R>,
    pub refresh_spin: bool,
    scroll: f32,
    content_h: f32,
    view_h: f32,
    hits: Vec<(R, Hit)>,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    hovers: Fades<u64>,
    switches: HashMap<Sw, (f32, f32)>,
    slider_drag: bool,
    slider_track: R,
    /// Opacity shown while dragging (the setting follows it).
    slider_value: Option<f64>,
    thumb_drag: Option<(f32, f32)>,
    thumb: R,
    t0: Instant,
    spin_from: Option<Instant>,
    last_w: f32,
    /// Hit boxes by key from the last paint, to know what a hover or a switch covers.
    rects: HashMap<u64, R>,
    /// Where the spinning icons, the opacity slider and the internet status row were painted.
    spin_rects: Vec<R>,
    slider_rect: R,
    net_rect: R,
    /// The account whose sign-out was clicked once, and when.
    sure: Option<(String, Instant)>,
    copied: Option<Instant>,
    /// Drag and drop of account cards: the card held, each card's shown top (relative to the list, sliding toward
    /// its place), where the held card would go, the list's box, whether anything still slides, the frame's dt.
    card_drag: Option<CardDrag>,
    slide: HashMap<String, f32>,
    drop_at: Option<usize>,
    list_rect: R,
    sliding: bool,
    dt: f32,
    /// The list as last laid out (ids and card heights), its pictures while cards move, and whether a paint leaves
    /// the cards out (taking the window picture).
    list: Vec<(String, f32)>,
    pics: Option<Pics>,
    cards_hidden: bool,
    /// Segmented controls' pills, by the hit key of each control's first option.
    pills: HashMap<u64, Pill>,
}

fn hkey(h: &Hit) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut s = std::collections::hash_map::DefaultHasher::new();
    h.hash(&mut s);
    s.finish()
}

impl AppWin {
    pub fn new() -> Self {
        Self {
            dirty: true,
            refresh_spin: false,
            scroll: 0.0,
            content_h: 0.0,
            view_h: 0.0,
            hits: Vec::new(),
            hover: None,
            pressed: None,
            hovers: Fades::new(0.12),
            switches: HashMap::new(),
            slider_drag: false,
            slider_track: R::default(),
            slider_value: None,
            thumb_drag: None,
            thumb: R::default(),
            t0: Instant::now(),
            spin_from: None,
            last_w: 0.0,
            damage: None,
            rects: HashMap::new(),
            spin_rects: Vec::new(),
            slider_rect: R::default(),
            net_rect: R::default(),
            sure: None,
            copied: None,
            card_drag: None,
            slide: HashMap::new(),
            drop_at: None,
            list_rect: R::default(),
            sliding: false,
            dt: 0.0,
            list: Vec::new(),
            pics: None,
            cards_hidden: false,
            pills: HashMap::new(),
        }
    }

    /// Height of everything below the title bar (for full-length screenshots).
    pub fn content_height(&self) -> f32 {
        self.content_h
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| h.clone())
    }

    pub fn cursor(&self) -> CursorKind {
        if self.card_drag.as_ref().is_some_and(|d| d.moving) {
            return CursorKind::Grabbing;
        }
        match &self.hover {
            Some(Hit::Card(_)) => CursorKind::Grab,
            Some(h) if h.pointer() => CursorKind::Pointer,
            _ => CursorKind::Default,
        }
    }

    /// Where account `id`'s card was painted last (screenshots drive a drag with it).
    pub fn card_box(&self, id: &str) -> Option<R> {
        self.rects.get(&hkey(&Hit::Card(id.to_string()))).copied()
    }

    /// The account list's box with room for the held card's overhang and shadow.
    fn damage_list(&mut self) {
        self.damage(self.list_rect.grow(48.0));
    }

    /// Advances animations; true while anything moves.
    pub fn step(&mut self, dt: f32) -> bool {
        // Added up until the cards are next placed (a paint can wait for its frame across several steps).
        self.dt += dt;
        let dragging = self.card_drag.as_ref().is_some_and(|d| d.moving);
        if self.sliding || dragging {
            self.damage_list();
        }
        let fading = self.hovers.moving();
        let mut moving = self.hovers.step(dt);
        for k in fading {
            self.damage_key(k);
        }
        let mut knobs = Vec::new();
        for (k, (v, t)) in self.switches.iter_mut() {
            if v != t {
                let d = dt / 0.18;
                *v = if *v < *t { (*v + d).min(*t) } else { (*v - d).max(*t) };
                knobs.push(*k);
            }
        }
        for k in knobs {
            moving = true;
            self.damage_key(k.key());
        }
        // Segmented controls' pills glide to the chosen option (eased: quick, then settling).
        let k = 1.0 - (-dt.clamp(0.0, 0.1) * 18.0).exp();
        let mut areas = Vec::new();
        for p in self.pills.values_mut() {
            if (p.x - p.tx).abs() > 0.25 || (p.w - p.tw).abs() > 0.25 {
                p.x += (p.tx - p.x) * k;
                p.w += (p.tw - p.w) * k;
                areas.push(p.area);
            } else if p.x != p.tx || p.w != p.tw {
                (p.x, p.w) = (p.tx, p.tw);
                areas.push(p.area);
            }
        }
        for a in areas {
            moving = true;
            self.damage(a.grow(4.0));
        }
        // The sign-out confirmation and "Copied" go back after a while.
        if self.sure.as_ref().is_some_and(|(_, t)| t.elapsed() >= SURE_FOR) {
            self.sure = None;
            self.dirty = true;
        }
        if self.copied.is_some_and(|t| t.elapsed() >= COPIED_FOR) {
            self.copied = None;
            self.dirty = true;
        }
        moving |= self.sure.is_some() || self.copied.is_some() || self.sliding || dragging;
        if self.refresh_spin || self.spin_from.is_some() {
            moving = true;
            for r in self.spin_rects.clone() {
                self.damage(r.grow(2.0));
            }
        }
        moving
    }

    /// Marks part of the window (DIP) for repainting.
    pub fn damage(&mut self, r: R) {
        self.damage = Some(self.damage.map_or(r, |d| d.union(r)));
    }

    fn damage_key(&mut self, k: u64) {
        // A little extra for shadows, glows and a pressed button's 1 px shift.
        if let Some(r) = self.rects.get(&k).copied() {
            self.damage(r.grow(6.0));
        }
    }

    fn damage_hit(&mut self, h: &Option<Hit>) {
        if let Some(h) = h {
            self.damage_key(hkey(h));
        }
    }

    /// The internet status row (its ping changes every few seconds).
    pub fn damage_net(&mut self) {
        if self.net_rect.w > 0.0 {
            self.damage(self.net_rect.grow(2.0));
        } else {
            self.dirty = true;
        }
    }

    fn switch_pos(&mut self, k: Sw, on: bool) -> f32 {
        let t = if on { 1.0 } else { 0.0 };
        let e = self.switches.entry(k).or_insert((t, t));
        e.1 = t;
        // transition: transform .18s cubic-bezier(.3, 1.4, .5, 1) (a little overshoot). The curve runs on how far the
        // knob has come toward where it is going (0 → 1 either way); off is measured back from the right.
        let done = if e.1 >= 0.5 { e.0 } else { 1.0 - e.0 };
        let eased = super::bezier(0.3, 1.4, 0.5, 1.0, done);
        if e.1 >= 0.5 {
            eased
        } else {
            1.0 - eased
        }
    }

    pub fn mouse(&mut self, f: &mut Fonts, ctx: &Ctx, m: Mouse, x: f32, y: f32) -> Vec<Action> {
        let _ = f;
        let mut out = Vec::new();
        match m {
            Mouse::Move => {
                if self.slider_drag {
                    out.push(Action::Opacity(self.slider_at(x)));
                    self.damage(self.slider_rect);
                    return out;
                }
                if let Some(d) = &mut self.card_drag {
                    d.y = y;
                    if !d.moving && (y - d.from_y).abs() > 4.0 {
                        d.moving = true;
                        let old = self.hover.take();
                        self.hovers.target(&[]);
                        self.damage_hit(&old);
                    }
                    if self.card_drag.as_ref().is_some_and(|d| d.moving) {
                        self.damage_list();
                        return out;
                    }
                }
                if let Some((y0, s0)) = self.thumb_drag {
                    let track = self.view_h;
                    let ratio = if track > 0.0 { self.content_h / track } else { 1.0 };
                    self.set_scroll(s0 + (y - y0) * ratio);
                    return out;
                }
                let h = self.hit_at(x, y);
                if h != self.hover {
                    let old = std::mem::replace(&mut self.hover, h.clone());
                    let on: Vec<u64> = h.iter().map(hkey).collect();
                    self.hovers.target(&on);
                    self.damage_hit(&old);
                    self.damage_hit(&h);
                }
            }
            Mouse::Leave => {
                if !self.slider_drag && self.thumb_drag.is_none() && self.card_drag.is_none() {
                    let old = self.hover.take();
                    self.hovers.target(&[]);
                    self.damage_hit(&old);
                }
            }
            Mouse::Down(Btn3::Left) => {
                let h = self.hit_at(x, y);
                self.pressed = h.clone();
                match h {
                    Some(Hit::TitleBar) => out.push(Action::DragWindow),
                    Some(Hit::Slider) => {
                        self.slider_drag = true;
                        out.push(Action::Opacity(self.slider_at(x)));
                    }
                    Some(Hit::Thumb) => self.thumb_drag = Some((y, self.scroll)),
                    Some(Hit::Card(ref id)) => {
                        let top = self.rects.get(&hkey(&Hit::Card(id.clone()))).map_or(y, |r| r.y);
                        self.card_drag = Some(CardDrag { id: id.clone(), from_y: y, y, grab_dy: y - top, moving: false });
                    }
                    _ => {}
                }
                self.damage_hit(&h);
                if self.slider_drag {
                    self.damage(self.slider_rect);
                }
            }
            Mouse::Up(Btn3::Left) => {
                // A card let go: it takes the place it was over, sliding there from where it is.
                if let Some(d) = self.card_drag.take() {
                    if d.moving {
                        if let Some(to) = self.drop_at.take() {
                            out.push(Action::MoveTo(d.id, to));
                        }
                        self.pressed = None;
                        self.sliding = true;
                        self.damage_list();
                        return out;
                    }
                }
                let was_slider = self.slider_drag;
                self.slider_drag = false;
                self.slider_value = None;
                self.thumb_drag = None;
                let pressed = self.pressed.take();
                let h = self.hit_at(x, y);
                self.damage_hit(&pressed);
                self.damage_hit(&h);
                if was_slider {
                    self.damage(self.slider_rect);
                }
                if was_slider || h.is_none() || h != pressed {
                    return out;
                }
                let s = &ctx.settings;
                match h.unwrap() {
                    Hit::Close => out.push(Action::Close),
                    Hit::Visible => out.push(Action::Visible(!ctx.visible)),
                    Hit::Lang(l) => out.push(Action::Lang(l)),
                    Hit::Layout(v) => out.push(Action::Vertical(v)),
                    Hit::Size(k) => out.push(Action::Size(k)),
                    Hit::Startup => out.push(Action::Startup(!ctx.startup.unwrap_or(false))),
                    Hit::Network => out.push(Action::Network(!s.network)),
                    Hit::Acct(id) => {
                        let on = s.on(&id);
                        out.push(Action::Service(id, !on));
                    }
                    Hit::Chip(id, win) => {
                        let shown = !s.hidden_of(&id).contains(&win);
                        out.push(Action::Window(id, win, !shown));
                    }
                    Hit::Add(svc) | Hit::SignIn(svc, _) => out.push(Action::SignIn(svc)),
                    Hit::SignOut(id) => {
                        if self.sure.as_ref().is_some_and(|(x, t)| *x == id && t.elapsed() < SURE_FOR) {
                            self.sure = None;
                            out.push(Action::SignOut(id));
                        } else {
                            self.sure = Some((id, Instant::now()));
                        }
                        self.dirty = true;
                    }
                    Hit::CancelLogin => out.push(Action::CancelSignIn),
                    Hit::CopyLink => {
                        self.copied = Some(Instant::now());
                        self.dirty = true;
                        out.push(Action::CopyLink);
                    }
                    Hit::Upd => {
                        if let Some(a) = upd_action(&ctx.update).0 {
                            out.push(Action::Update(a));
                        }
                    }
                    Hit::Refresh => {
                        self.refresh_spin = true;
                        self.spin_from = Some(Instant::now());
                        out.push(Action::Refresh);
                    }
                    Hit::Snap => out.push(Action::Snap),
                    Hit::Restart => out.push(Action::Restart),
                    Hit::Quit => out.push(Action::Quit),
                    Hit::TitleBar | Hit::Slider | Hit::Thumb | Hit::Card(_) => {}
                }
            }
            Mouse::Wheel(n) => {
                // Chromium scrolls 100 px per notch... of which 40 lines? It scrolls about 53 px per notch at 100 %.
                self.set_scroll(self.scroll - n * 100.0);
            }
            _ => {}
        }
        out
    }

    fn set_scroll(&mut self, s: f32) {
        let max = (self.content_h - self.view_h).max(0.0);
        let s = s.clamp(0.0, max);
        if s != self.scroll {
            self.scroll = s;
            self.dirty = true;
        }
    }

    fn slider_at(&mut self, x: f32) -> f64 {
        // The thumb's centre runs from 8 px to 140 − 8 px of the track; values 50–100 in steps of 1.
        let t = self.slider_track;
        let p = ((x - t.x - 8.0) / (t.w - 16.0)).clamp(0.0, 1.0);
        let v = (50.0 + p * 50.0).round() as f64 / 100.0;
        self.slider_value = Some(v);
        v
    }

    fn hov(&self, h: &Hit) -> f32 {
        self.hovers.get(hkey(h))
    }

    /// Lays out and paints the whole window, `w`×`h` DIP.
    pub fn draw(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, w: f32, h: f32) {
        if w != self.last_w {
            self.last_w = w;
        }
        self.view_h = h - TITLE_H;
        self.hits.clear();
        self.spin_rects.clear();
        c.pm.fill(BG.sk());
        let content_h = self.draw_main(c, f, ctx, w);
        self.content_h = content_h;
        let max = (content_h - self.view_h).max(0.0);
        if self.scroll > max {
            // The window grew or content shrank: clamp and draw again.
            self.scroll = max;
            self.hits.clear();
            c.pm.fill(BG.sk());
            self.draw_main(c, f, ctx, w);
        }
        self.draw_scrollbar(c, w, h);
        self.draw_titlebar(c, f, ctx, w);
        self.rects = self.hits.iter().map(|(r, h)| (hkey(h), *r)).collect();
        if self.refresh_spin && self.spin_from.is_none() {
            self.spin_from = Some(Instant::now());
        }
        if !self.refresh_spin {
            self.spin_from = None;
        }
    }

    fn draw_titlebar(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, w: f32) {
        let s = i18n::t(ctx.lang);
        let bar = R::new(0.0, 0.0, w, TITLE_H);
        c.fill_rr(bar, 0.0, BG);
        c.fill_rr(bar, 0.0, linear(180.0, bar, &[(0.0, Rgba::rgba(255, 255, 255, 0.025)), (1.0, Rgba::TRANSPARENT)]));
        c.fill_rr(R::new(0.0, TITLE_H - 1.0, w, 1.0), 0.0, LINE);
        self.hits.push((bar, Hit::TitleBar));
        let mid = (TITLE_H - 1.0) / 2.0;
        draw_app_icon(c, 16.0, mid - 10.0, 20.0);
        let ts = Style::new(13.0, 600, TEXT, 1.4);
        let x = 16.0 + 20.0 + 10.0;
        let tw = f.draw(c, x, mid - ts.lh / 2.0, s.s.title, &ts, None, 0.0).w;
        let space = f.width(" ", &ts);
        f.draw(c, x + tw + space, mid - ts.lh / 2.0, "· Ai Quota Overlay", &ts.weight(400).color(MUTED), None, 0.0);
        let btn = R::new(w - 8.0 - 32.0, mid - 15.0, 32.0, 30.0);
        let hv = self.hov(&Hit::Close);
        if hv > 0.0 {
            c.fill_rr(btn, 8.0, Rgba::rgba(248, 113, 113, 0.16 * hv));
        }
        icons::CLOSE.draw(c, btn.cx() - 6.0, btn.cy() - 6.0, 12.0, 12.0, MUTED.lerp(Rgba::WHITE, hv));
        self.hits.push((btn, Hit::Close));
    }

    fn draw_scrollbar(&mut self, c: &mut Canvas, w: f32, h: f32) {
        if self.content_h <= self.view_h + 0.5 {
            return;
        }
        let track_h = self.view_h;
        let thumb_h = (track_h * track_h / self.content_h).max(20.0);
        let max = self.content_h - self.view_h;
        let y = TITLE_H + (track_h - thumb_h) * (self.scroll / max);
        let thumb = R::new(w - GUTTER, y, GUTTER, thumb_h);
        self.thumb = thumb;
        let hv = self.hov(&Hit::Thumb).max(if self.thumb_drag.is_some() { 1.0 } else { 0.0 });
        // 10 px wide with a 3 px border in the page colour: a 4 px bar.
        c.fill_rr(thumb.inset(3.0, 3.0, 3.0, 3.0), 2.0, Rgba::rgba(255, 255, 255, 0.1 + 0.08 * hv));
        self.hits.push((thumb, Hit::Thumb));
        let _ = h;
    }

    /// Paints `main` scrolled by `self.scroll`, in two columns: General, Internet status and Updates on the left;
    /// Services and the action buttons on the right; the footer across the bottom. Returns the content height.
    fn draw_main(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, w: f32) -> f32 {
        let s = i18n::t(ctx.lang);
        let col_gap = 20.0;
        let cw = ((w - 32.0 - col_gap) / 2.0).floor();
        let top = TITLE_H - self.scroll;
        let y0 = top + 6.0;

        // ---------- left column ----------
        let x0 = 16.0;
        let mut y = y0;
        let lang = ctx.lang;

        // ---------- General ----------
        y += GAP;
        y = self.sec_head(c, f, x0, y, s.s.general);
        let rows_y = y;
        y += 1.0; // .group border
        let mut rows: Vec<f32> = Vec::new();
        // Show overlay
        let ry = y;
        let visible = ctx.visible;
        let pos = self.switch_pos(Sw::Visible, visible);
        y += self.row(c, f, x0, ry, cw, &icons::EYE, s.s.show_overlay, Some((s.s.show_overlay_hint, false)), &mut |me, c, f, right, mid| {
            let _ = f;
            me.switch(c, right - 38.0, mid, pos, visible, ACCENT, Hit::Visible);
            (38.0, 22.0)
        });
        rows.push(y);
        y += 1.0;
        // Language
        y += self.row(c, f, x0, y, cw, &icons::GLOBE, s.s.language, None, &mut |me, c, f, right, mid| {
            let items: Vec<(Option<&Icon>, &str, Hit, bool)> = LANGS.iter().map(|(l, n)| (None, *n, Hit::Lang(*l), *l == lang)).collect();
            me.seg(c, f, right, mid, &items)
        });
        rows.push(y);
        y += 1.0;
        // Layout
        let vertical = ctx.settings.vertical;
        y += self.row(c, f, x0, y, cw, &icons::LAYOUT, s.s.layout, None, &mut |me, c, f, right, mid| {
            let items = [
                (Some(&icons::SEG_H), s.s.horizontal, Hit::Layout(false), !vertical),
                (Some(&icons::SEG_V), s.s.vertical, Hit::Layout(true), vertical),
            ];
            me.seg(c, f, right, mid, &items)
        });
        rows.push(y);
        y += 1.0;
        // Size
        let size = ctx.settings.size.as_str();
        y += self.row(c, f, x0, y, cw, &icons::SIZE, s.s.size, None, &mut |me, c, f, right, mid| {
            let labels = [s.s.small, s.s.medium, s.s.large];
            let items: Vec<(Option<&Icon>, &str, Hit, bool)> = crate::settings::SIZES
                .iter()
                .zip(labels)
                .map(|((k, _), label)| (None, label, Hit::Size(k), *k == size))
                .collect();
            me.seg(c, f, right, mid, &items)
        });
        rows.push(y);
        y += 1.0;
        // Opacity
        let value = self.slider_value.unwrap_or(ctx.settings.opacity);
        y += self.row(c, f, x0, y, cw, &icons::OPACITY, s.s.opacity, None, &mut |me, c, f, right, mid| {
            me.slider(c, f, right, mid, value);
            (140.0 + 10.0 + 38.0, 16.8)
        });
        // Launch at startup (Windows and macOS)
        if let Some(on) = ctx.startup {
            rows.push(y);
            y += 1.0;
            y += 1.0;
            let pos = self.switch_pos(Sw::Startup, on);
            // The last change didn't take (the system or an antivirus kept the startup entry as it was).
            let startup_hint = if ctx.startup_failed { (s.s.startup_blocked, true) } else { (s.s.startup_hint, false) };
            y += self.row(c, f, x0, y, cw, &icons::POWER, s.s.startup, Some(startup_hint), &mut |me, c, _f, right, mid| {
                me.switch(c, right - 38.0, mid, pos, on, ACCENT, Hit::Startup);
                (38.0, 22.0)
            });
        }
        // Internet status (its own section before the size option; folded in so everything still fits)
        rows.push(y);
        y += 1.0;
        let net_on = ctx.settings.network;
        let pos = self.switch_pos(Sw::Network, net_on);
        let net = ctx.net;
        let net_y = y;
        y += self.row(c, f, x0, y, cw, &icons::WIFI_ROW, s.s.network_show, Some((s.s.network_hint, false)), &mut |me, c, f, right, mid| {
            me.switch(c, right - 38.0, mid, pos, net_on, ACCENT, Hit::Network);
            let mut used = 38.0;
            if let (true, Some(n)) = (net_on, net) {
                let text = if n.online { format!("{} ms", n.ms.unwrap_or(0)) } else { i18n::t(lang).offline.to_string() };
                let st = Style::new(11.5, 600, if n.online { OK } else { BAD }, 1.4).tab();
                let tw = f.width(&text, &st);
                let pw = 9.0 + 7.0 + 6.0 + tw + 9.0;
                let ph = st.lh + 6.0;
                let px = right - 38.0 - 14.0 - pw;
                let col = if n.online { OK } else { BAD };
                c.fill_rr(R::new(px, mid - ph / 2.0, pw, ph), 20.0, col.pct(if n.online { 0.13 } else { 0.14 }));
                c.glow_circle(px + 9.0 + 3.5, mid, 3.5, 0.0, 6.0, 0.0, col);
                c.fill_circle(px + 9.0 + 3.5, mid, 3.5, col);
                f.draw(c, px + 9.0 + 7.0 + 6.0, mid - st.lh / 2.0, &text, &st, None, 0.0);
                used += 14.0 + pw;
            }
            (used, 22.0)
        });
        self.net_rect = R::new(x0, net_y, cw, y - net_y);
        y += 1.0;
        self.group_frame(c, x0, rows_y, cw, y - rows_y, &rows);

        let left_end = y;

        // ---------- right column ----------
        let x0 = 16.0 + cw + col_gap;
        let mut y = y0;

        // ---------- Services ----------
        y += GAP;
        y = self.sec_head(c, f, x0, y, s.s.services);
        let hint = Style::new(11.5, 400, FAINT, 1.4);
        y -= 4.0;
        y += f.draw(c, x0 + 4.0, y, s.s.services_hint, &hint, Some(cw - 8.0), 0.0).h;
        y += 8.0;
        y += self.add_buttons(c, f, ctx, x0, y, cw) + 10.0;
        y += self.login_line(c, f, ctx, x0, y, cw);
        let rows = account_rows(ctx);
        if rows.is_empty() && ctx.data.is_some() {
            y += self.no_accounts(c, f, ctx, x0, y, cw);
        }
        y += self.account_list(c, f, ctx, &rows, x0, y, cw);

        // ---------- Updates ----------
        y += GAP;
        y = self.sec_head(c, f, x0, y, s.u.section);
        let gy = y;
        y += 1.0;
        y += self.update_row(c, f, ctx, x0, y, cw);
        y += 1.0;
        self.group_frame(c, x0, gy, cw, y - gy, &[]);
        let right_end = y;

        // ---------- actions ---------- (one row under the left column, level with the right column's end when
        // there is room; under the left column when many accounts make the right one run past the window)
        let x0 = 16.0;
        let bw = (cw - 3.0 * 8.0) / 4.0;
        let bh = Style::new(12.5, 500, TEXT, 1.4).lh + 18.0 + 2.0;
        let right_col_end = right_end;
        let fits = right_col_end <= top + self.view_h - 50.0;
        let y = if fits { (left_end + 16.0).max(right_end - bh) } else { left_end + 16.0 };
        let spin = self.spin_from.map(|t| (t.elapsed().as_secs_f32() % 0.8) / 0.8 * 360.0).unwrap_or(0.0);
        let buttons = [
            (&icons::REFRESH, s.s.refresh, Hit::Refresh, false, if self.refresh_spin { spin } else { 0.0 }),
            (&icons::SNAP, s.s.snap, Hit::Snap, false, 0.0),
            (&icons::RESTART, s.s.restart, Hit::Restart, false, 0.0),
            (&icons::QUIT, s.s.quit, Hit::Quit, true, 0.0),
        ];
        for (i, (icon, text, hit, danger, rot)) in buttons.into_iter().enumerate() {
            let bx = x0 + i as f32 * (bw + 8.0);
            if hit == Hit::Refresh && self.refresh_spin {
                self.spin_rects.push(R::new(bx, y, bw, bh));
            }
            self.button(c, f, bx, y, bw, icon, text, hit, danger, rot);
        }
        let left_end = y + bh;
        let right_end = right_col_end.max(y);

        // ---------- footer ---------- (kept at the bottom of the window when everything fits)
        let x0 = 16.0;
        let cw = w - 32.0;
        let fs = Style::new(11.5, 400, FAINT, 1.4);
        let kbd = Style::new(10.5, 400, MUTED, 1.4).mono();
        let kh = kbd.lh + 2.0 + 3.0;
        let line_h = fs.lh.max(kh);
        let y = (left_end.max(right_end) + 14.0).max(top + self.view_h - 14.0 - line_h);
        let mid = y + line_h / 2.0;
        let mut x = x0 + 4.0;
        let ver = format!("v{}", ctx.version);
        x += f.draw(c, x, mid - fs.lh / 2.0, &ver, &fs, None, 0.0).w;
        if let Some(d) = &ctx.data {
            if d.fetched_at > 0 {
                let t = format!(" · {}", i18n::updated(ctx.lang, &crate::fmt::time(d.fetched_at, ctx.lang)));
                f.draw(c, x + 8.0, mid - fs.lh / 2.0, &t, &fs, None, 0.0);
            }
        }
        // Right side: Ctrl Alt Q  Show / hide
        let label = s.s.shortcut;
        let lw = f.width(label, &fs);
        let nb = f.width("\u{a0}", &fs);
        let keys = ["Ctrl", "Alt", "Q"];
        let kws: Vec<f32> = keys.iter().map(|k| f.width(k, &kbd) + 10.0 + 2.0).collect();
        let total = kws.iter().sum::<f32>() + 4.0 * (keys.len() as f32 - 1.0) + nb + lw;
        let mut kx = x0 + cw - 4.0 - total;
        for (k, kw) in keys.iter().zip(&kws) {
            let r = R::new(kx, mid - kh / 2.0, *kw, kh);
            c.fill_rr(r, 4.0, SURFACE2);
            c.border_sides(r, [4.0; 4], [1.0, 1.0, 2.0, 1.0], LINE2);
            f.draw(c, kx + 6.0, r.y + 1.0, k, &kbd, None, 0.0);
            kx += kw + 4.0;
        }
        f.draw(c, kx - 4.0 + nb, mid - fs.lh / 2.0, label, &fs, None, 0.0);
        y + line_h + 14.0 - top
    }

    fn sec_head(&mut self, c: &mut Canvas, f: &mut Fonts, x0: f32, y: f32, text: &str) -> f32 {
        let st = Style::new(11.0, 700, MUTED, 1.4).ls_em(0.08);
        f.draw(c, x0 + 4.0, y, &text.to_uppercase(), &st, None, 0.0);
        y + st.lh + 8.0
    }

    /// The rounded frame of a `.group` (drawn after its rows: border and the lines between rows).
    fn group_frame(&mut self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, dividers: &[f32]) {
        let r = R::new(x, y, w, h);
        // overflow: hidden with border-radius 14: the rows' square corners are painted over in the page colour.
        let mut pb = tiny_skia::PathBuilder::new();
        if let (Some(sq), Some(round)) = (super::gfx::rr(r, 0.0), super::gfx::rr(r, 14.0)) {
            pb.push_path(&sq);
            pb.push_path(&round);
        }
        if let Some(p) = pb.finish() {
            c.fill_path_eo(&p, BG);
        }
        c.border(r, 14.0, 1.0, LINE);
        for d in dividers {
            c.fill_rr(R::new(x + 1.0, *d, w - 2.0, 1.0), 0.0, SURFACE);
            c.fill_rr(R::new(x + 1.0, *d, w - 2.0, 1.0), 0.0, LINE);
        }
    }

    /// One `.row` of a group: icon, title (+ hint), and controls drawn by `right` (given the right edge and the
    /// row's middle, it returns the controls' width). Returns the row height. The group's background is drawn here.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        c: &mut Canvas,
        f: &mut Fonts,
        x: f32,
        y: f32,
        w: f32,
        icon: &Icon,
        title: &str,
        hint: Option<(&str, bool)>,
        right: &mut dyn FnMut(&mut Self, &mut Canvas, &mut Fonts, f32, f32) -> (f32, f32),
    ) -> f32 {
        let ts = Style::new(13.0, 500, TEXT, 1.4);
        let hs = Style::new(11.5, 400, MUTED, 1.4);
        // Measure the controls first (dry run on a scratch canvas) to know the text width.
        let (ctrl_w, ctrl_h) = {
            let mut scratch = Pixmap::new(1, 1).unwrap();
            let mut sc = Canvas::new(&mut scratch, c.scale, 0.0, -10000.0);
            let n = self.hits.len();
            let v = right(self, &mut sc, f, x + w - 14.0, y);
            self.hits.truncate(n);
            v
        };
        let text_w = (w - 28.0 - 30.0 - 14.0 - 14.0 - ctrl_w).max(10.0);
        let th = ts.lh;
        let hh = hint.map(|(h, _)| 1.0 + f.lay(h, &hs, Some(text_w)).h).unwrap_or(0.0);
        let content = (th + hh).max(30.0).max(ctrl_h);
        let rh = (content + 24.0).max(52.0);
        c.fill_rr(R::new(x, y, w, rh), 0.0, SURFACE);
        let mid = y + rh / 2.0;
        // Background inside the rounded group frame is clipped by drawing the corners in BG afterwards (group_frame
        // draws the border); rows at the top/bottom get their corners masked here.
        let icon_r = R::new(x + 14.0, mid - 15.0, 30.0, 30.0);
        c.fill_rr(icon_r, 9.0, SURFACE2);
        icon.draw(c, icon_r.cx() - 8.0, icon_r.cy() - 8.0, 16.0, 16.0, MUTED);
        let tx = x + 14.0 + 30.0 + 14.0;
        let ty = mid - (th + hh) / 2.0;
        f.draw(c, tx, ty, title, &ts, None, 0.0);
        if let Some((h, warn)) = hint {
            f.draw(c, tx, ty + th + 1.0, h, &hs.color(if warn { WARN } else { MUTED }), Some(text_w), 0.0);
        }
        right(self, c, f, x + w - 14.0, mid);
        rh
    }

    #[allow(clippy::too_many_arguments)]
    fn switch(&mut self, c: &mut Canvas, x: f32, mid: f32, pos: f32, on: bool, color: Rgba, hit: Hit) {
        let r = R::new(x, mid - 11.0, 38.0, 22.0);
        let off = Rgba::rgba(255, 255, 255, 0.14);
        let bg = off.lerp(color, pos.clamp(0.0, 1.0));
        let _ = on;
        c.fill_rr(r, 11.0, bg);
        let kx = r.x + 3.0 + 16.0 * pos;
        let knob = R::new(kx, r.y + 3.0, 16.0, 16.0);
        c.shadow(knob, 8.0, 0.0, 1.0, 3.0, 0.0, Rgba::rgba(0, 0, 0, 0.4));
        c.fill_circle(knob.cx(), knob.cy(), 8.0, Rgba::WHITE);
        self.hits.push((r, hit));
    }

    /// Segmented control ending at `right`; returns its width.
    fn seg(&mut self, c: &mut Canvas, f: &mut Fonts, right: f32, mid: f32, items: &[(Option<&Icon>, &str, Hit, bool)]) -> (f32, f32) {
        let st = Style::new(12.5, 500, MUTED, 1.4);
        let widths: Vec<f32> = items.iter().map(|(ic, t, _, _)| 24.0 + f.width(t, &st) + if ic.is_some() { 14.0 + 6.0 } else { 0.0 }).collect();
        let bh = st.lh + 10.0;
        let w = widths.iter().sum::<f32>() + 2.0 * (items.len() as f32 - 1.0) + 6.0 + 2.0;
        let h = bh + 6.0 + 2.0;
        let r = R::new(right - w, mid - h / 2.0, w, h);
        c.fill_rr(r, 10.0, BG);
        c.border(r, 10.0, 1.0, LINE);
        let mut boxes = Vec::with_capacity(items.len());
        let mut x = r.x + 1.0 + 3.0;
        for bw in &widths {
            boxes.push(R::new(x, r.y + 4.0, *bw, bh));
            x += bw + 2.0;
        }
        // The selected pill slides to the chosen option and takes its width (`step`); new, it starts there.
        let (tx, tw) = items.iter().zip(&boxes).find(|(it, _)| it.3).map_or((0.0, 0.0), |(_, b)| (b.x - r.x, b.w));
        let pill = self.pills.entry(hkey(&items[0].2)).or_insert(Pill { x: tx, w: tw, tx, tw, area: r });
        (pill.tx, pill.tw, pill.area) = (tx, tw, r);
        let p = R::new(r.x + pill.x, r.y + 4.0, pill.w, bh);
        if p.w > 0.0 {
            c.shadow(p, 7.0, 0.0, 1.0, 3.0, 0.0, Rgba::rgba(0, 0, 0, 0.35));
            c.fill_rr(p, 7.0, SURFACE2);
            c.shadow_rrect(p, [7.0; 4], 0.0, 1.0, 0.0, 0.0, Rgba::rgba(255, 255, 255, 0.06), true);
        }
        for ((ic, text, hit, _), b) in items.iter().zip(boxes) {
            // Bright under the pill, fading out as it slides away.
            let under = 1.0 - ((p.cx() - b.cx()).abs() / b.w.max(1.0)).min(1.0);
            let hv = self.hov(hit);
            let color = MUTED.lerp(TEXT, under.max(hv));
            let mut tx = b.x + 12.0;
            if let Some(icon) = ic {
                let (iw, ih) = (icon.vb.0, icon.vb.1);
                icon.draw(c, tx, b.cy() - ih / 2.0, iw, ih, color);
                tx += 14.0 + 6.0;
            }
            f.draw(c, tx, b.cy() - st.lh / 2.0, text, &st.color(color), None, 0.0);
            self.hits.push((b, hit.clone()));
        }
        (w, h)
    }

    fn slider(&mut self, c: &mut Canvas, f: &mut Fonts, right: f32, mid: f32, value: f64) {
        let ow = 38.0;
        let track = R::new(right - ow - 10.0 - 140.0, mid - 3.0, 140.0, 6.0);
        self.slider_track = track;
        // The thumb's glow reaches past the track ends; the value text sits at the right.
        self.slider_rect = R::new(track.x - 14.0, mid - 16.0, right - track.x + 16.0, 32.0);
        let v = (value * 100.0).round().clamp(50.0, 100.0) as f32;
        let p = (v - 50.0) / 50.0;
        // linear-gradient(90deg, accent p, track p): a hard stop.
        let px = track.x + track.w * p;
        c.fill_rr(track, 3.0, Rgba::rgba(255, 255, 255, 0.12));
        c.clipped(track, 3.0, |c| c.fill_rr(R::new(track.x, track.y, px - track.x, track.h), 0.0, ACCENT));
        let tcx = track.x + 8.0 + p * (track.w - 16.0);
        c.glow_circle(tcx, mid, 8.0, 1.0, 3.0, 0.0, Rgba::rgba(0, 0, 0, 0.5));
        c.glow_circle(tcx, mid, 8.0, 0.0, 0.0, 4.0, ACCENT.pct(0.25));
        c.fill_circle(tcx, mid, 8.0, Rgba::WHITE);
        self.hits.push((R::new(track.x, mid - 10.0, track.w, 20.0), Hit::Slider));
        let st = Style::new(12.0, 400, MUTED, 1.4).tab();
        let text = format!("{v}%");
        let tw = f.width(&text, &st);
        f.draw(c, right - tw, mid - st.lh / 2.0, &text, &st, None, 0.0);
    }

    /// "+ Claude  + Codex  + Cursor …" (sign in to an account), wrapping across `w`. Returns the height.
    fn add_buttons(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, x: f32, y: f32, w: f32) -> f32 {
        let st = Style::new(12.0, 600, TEXT, 1.4);
        let h = st.lh + 10.0;
        let (mut bx, mut by) = (x, y);
        for svc in SERVICES {
            let label = format!("+ {}", svc.name());
            let bw = f.width(&label, &st) + 24.0;
            if bx > x && bx + bw > x + w {
                bx = x;
                by += h + 6.0;
            }
            let r = R::new(bx, by, bw, h);
            let hit = Hit::Add(svc);
            let hv = self.hov(&hit).max(if ctx.login == Some(svc) { 1.0 } else { 0.0 });
            let col = svc_color(svc);
            c.fill_rr(r, h / 2.0, col.pct(0.10 + 0.10 * hv));
            c.border(r, h / 2.0, 1.0, col.pct(0.35 + 0.3 * hv));
            f.draw(c, r.x + 12.0, r.cy() - st.lh / 2.0, &label, &st.color(col.lerp(TEXT, 0.3 + 0.5 * hv)), None, 0.0);
            self.hits.push((r, hit));
            bx += bw + 8.0;
        }
        by + h - y
    }

    /// The account cards in order, with drag and drop: the held card follows the pointer above the others, which
    /// slide out of its way, and once let go it slides into its place. Returns the height. (While cards move, frames
    /// are painted from pictures instead, see `draw_part`.)
    #[allow(clippy::too_many_arguments)]
    fn account_list(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, rows: &[(Acct, Quota)], x: f32, y: f32, w: f32) -> f32 {
        if rows.is_empty() {
            self.list = Vec::new();
            self.list_rect = R::default();
            self.sliding = false;
            return 0.0;
        }
        // Heights first, off-canvas (a card keeps its size while it moves).
        let heights: Vec<f32> = rows
            .iter()
            .map(|(a, q)| {
                let mut scratch = Pixmap::new(1, 1).unwrap();
                let mut sc = Canvas::new(&mut scratch, c.scale, 0.0, -10000.0);
                let n = self.hits.len();
                let h = self.account(&mut sc, f, ctx, a, q, x, 0.0, w);
                self.hits.truncate(n);
                h
            })
            .collect();
        let total = heights.iter().sum::<f32>() + CARD_GAP * (rows.len() - 1) as f32;
        self.list = rows.iter().map(|(a, _)| a.id.clone()).zip(heights.iter().copied()).collect();
        self.list_rect = R::new(x, y, w, total);
        if self.cards_hidden {
            // Painting the window behind the cards (for `draw_part`).
            return total;
        }
        let (order, tops, held) = self.arrange();
        // The others, then the held one on top with a lifted shadow.
        for &i in order.iter().filter(|i| held != Some(**i)) {
            let (a, q) = &rows[i];
            self.account(c, f, ctx, a, q, x, y + tops[i], w);
        }
        if let Some(hi) = held {
            let (a, q) = &rows[hi];
            let top = y + tops[hi];
            c.shadow(R::new(x, top, w, heights[hi]), 14.0, 0.0, 10.0, 28.0, 0.0, LIFT_SHADOW);
            self.account(c, f, ctx, a, q, x, top, w);
        }
        total
    }

    /// Where the cards of `self.list` go this frame: the held one where the pointer has it (and where it would be
    /// dropped: before the first other card whose middle is below its own), the others sliding toward their places,
    /// eased per frame. Returns the painting order, each card's top relative to the list, and the held card.
    fn arrange(&mut self) -> (Vec<usize>, Vec<f32>, Option<usize>) {
        let n = self.list.len();
        let total = self.list_rect.h;
        let held: Option<(usize, f32)> = self
            .card_drag
            .as_ref()
            .filter(|d| d.moving)
            .and_then(|d| self.list.iter().position(|(id, _)| *id == d.id).map(|i| (i, d.y - d.grab_dy - self.list_rect.y)));
        let mut order: Vec<usize> = (0..n).filter(|i| held.is_none_or(|(h, _)| h != *i)).collect();
        let mut held_top = 0.0;
        if let Some((hi, top)) = held {
            held_top = top.clamp(-24.0, total - self.list[hi].1 + 24.0);
            let mid = held_top + self.list[hi].1 / 2.0;
            let mut acc = 0.0;
            let mut at = order.len();
            for (k, &i) in order.iter().enumerate() {
                if mid < acc + self.list[i].1 / 2.0 {
                    at = k;
                    break;
                }
                acc += self.list[i].1 + CARD_GAP;
            }
            order.insert(at, hi);
            self.drop_at = Some(at);
        }
        let k = 1.0 - (-std::mem::take(&mut self.dt).clamp(0.0, 0.1) * 16.0).exp();
        let mut sliding = false;
        let mut tops = vec![0.0f32; n];
        let mut acc = 0.0;
        for &i in &order {
            let id = &self.list[i].0;
            let shown = if held.is_some_and(|(h, _)| h == i) {
                held_top
            } else {
                let cur = self.slide.get(id).copied().unwrap_or(acc);
                let next = if (acc - cur).abs() < 0.5 { acc } else { cur + (acc - cur) * k };
                sliding |= next != acc;
                next
            };
            self.slide.insert(id.clone(), shown);
            tops[i] = shown;
            acc += self.list[i].1 + CARD_GAP;
        }
        let list = &self.list;
        self.slide.retain(|id, _| list.iter().any(|(x, _)| x == id));
        self.sliding = sliding;
        (order, tops, held.map(|(h, _)| h))
    }

    /// Paints the part of the window that changed. While account cards are dragged or slide, that is only the list:
    /// it is put together from pictures taken when the move began (the window without the cards, and each card),
    /// rather than laying out and painting every card again each frame.
    pub fn draw_part(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, w: f32, h: f32) {
        let moving = self.sliding || self.card_drag.as_ref().is_some_and(|d| d.moving);
        let rows = account_rows(ctx);
        let same = rows.len() == self.list.len() && rows.iter().all(|(a, _)| self.list.iter().any(|(id, _)| *id == a.id));
        if !moving || !same {
            self.pics = None;
            return self.draw(c, f, ctx, w, h);
        }
        if self.pics.as_ref().is_none_or(|p| p.scale != c.scale) {
            self.pics = Some(self.take_pics(f, ctx, &rows, w, h, c.scale));
        }
        let Some(p) = self.pics.take() else { return };
        let (order, tops, held) = self.arrange();
        // The window and the cards resting on it are opaque: copied straight in. The held card is blended (its
        // shadow), on top.
        copy_pic(c, &p.window, 0.0, 0.0);
        let (lx, ly) = (self.list_rect.x, self.list_rect.y);
        for &i in order.iter().filter(|i| held != Some(**i)) {
            if let Some(pic) = p.cards.get(&self.list[i].0) {
                copy_pic(c, pic, lx, ly + tops[i]);
            }
        }
        if let (Some(i), Some((id, pic))) = (held, p.lifted.as_ref()) {
            if self.list[i].0 == *id {
                let inv = 1.0 / p.scale;
                let paint = PixmapPaint { quality: FilterQuality::Nearest, ..PixmapPaint::default() };
                let (dx, dy) = (lx - LIFT_PAD, ly + tops[i] - LIFT_PAD);
                c.pm.draw_pixmap(0, 0, pic.as_ref(), &paint, c.base.pre_translate(dx, dy).pre_scale(inv, inv), None);
            }
        }
        self.pics = Some(p);
        // Settled: one full paint brings back the cards' hit boxes (the window picture was taken without them).
        if !self.sliding && self.card_drag.as_ref().is_none_or(|d| !d.moving) {
            self.pics = None;
            self.dirty = true;
        }
    }

    /// The pictures `draw_part` is put together from, at `scale` device px per DIP.
    #[allow(clippy::too_many_arguments)]
    fn take_pics(&mut self, f: &mut Fonts, ctx: &Ctx, rows: &[(Acct, Quota)], w: f32, h: f32, scale: f32) -> Pics {
        let px = |v: f32| (v * scale).ceil().max(1.0) as u32;
        // The window without the cards. (Painting resets the hit boxes; the cards' come back with the next full paint.)
        let mut window = Pixmap::new(px(w), px(h)).unwrap();
        self.cards_hidden = true;
        {
            let mut c = Canvas::new(&mut window, scale, 0.0, 0.0);
            self.draw(&mut c, f, ctx, w, h);
        }
        self.cards_hidden = false;
        let (lx, lw) = (self.list_rect.x, self.list_rect.w);
        let held = self.card_drag.as_ref().filter(|d| d.moving).map(|d| d.id.clone());
        let mut cards = HashMap::new();
        let mut lifted = None;
        for ((a, q), (_, ch)) in rows.iter().zip(self.list.clone()) {
            let n = self.hits.len();
            // On the window's colour, so it can be copied in without blending.
            let mut pic = Pixmap::new(px(lw), px(ch)).unwrap();
            pic.fill(BG.sk());
            {
                let mut c = Canvas::new(&mut pic, scale, -lx, 0.0);
                self.account(&mut c, f, ctx, a, q, lx, 0.0, lw);
            }
            if held.as_deref() == Some(a.id.as_str()) {
                let mut pic = Pixmap::new(px(lw + 2.0 * LIFT_PAD), px(ch + 2.0 * LIFT_PAD)).unwrap();
                {
                    let mut c = Canvas::new(&mut pic, scale, -lx + LIFT_PAD, LIFT_PAD);
                    c.shadow(R::new(lx, 0.0, lw, ch), 14.0, 0.0, 10.0, 28.0, 0.0, LIFT_SHADOW);
                    self.account(&mut c, f, ctx, a, q, lx, 0.0, lw);
                }
                lifted = Some((a.id.clone(), pic));
            }
            self.hits.truncate(n);
            cards.insert(a.id.clone(), pic);
        }
        Pics { scale, window, cards, lifted }
    }

    /// In place of the accounts while there are none. Returns the height.
    fn no_accounts(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, x: f32, y: f32, w: f32) -> f32 {
        let s = i18n::t(ctx.lang);
        let ts = Style::new(13.0, 500, TEXT, 1.4);
        let hs = Style::new(11.5, 400, MUTED, 1.4);
        let tw = w - 2.0 * 18.0;
        let th = f.lay(s.s.no_accounts, &ts, Some(tw)).h;
        let hh = f.lay(s.s.no_accounts_hint, &hs, Some(tw)).h;
        let h = 18.0 + th + 3.0 + hh + 18.0;
        let r = R::new(x, y, w, h);
        c.fill_rr(r, 14.0, SURFACE);
        c.border(r, 14.0, 1.0, LINE);
        f.draw(c, x + 18.0, y + 18.0, s.s.no_accounts, &ts, Some(tw), 0.0);
        f.draw(c, x + 18.0, y + 18.0 + th + 3.0, s.s.no_accounts_hint, &hs, Some(tw), 0.0);
        h
    }

    /// The sign-in under way (with Copy link and Cancel), or how the last one went. Returns the height it took.
    fn login_line(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, x: f32, y: f32, w: f32) -> f32 {
        let s = i18n::t(ctx.lang);
        if let Some(svc) = ctx.login {
            let col = svc_color(svc);
            let ts = Style::new(12.5, 600, TEXT, 1.4);
            let hs = Style::new(11.5, 400, MUTED, 1.4);
            let bs = Style::new(12.0, 500, TEXT, 1.4);
            // GitHub: a code to type on its page (copied for the user once it comes); the others: the page itself.
            let device = svc == Svc::Copilot;
            let copy = match (self.copied.is_some(), device) {
                (true, _) => s.s.copied,
                (false, true) => s.s.copy_code,
                (false, false) => s.s.copy_link,
            };
            let mut buttons = vec![(copy, Hit::CopyLink), (s.s.cancel, Hit::CancelLogin)];
            if device && ctx.login_code.is_none() {
                buttons.remove(0);
            }
            let bws: Vec<f32> = buttons.iter().map(|(l, _)| f.width(l, &bs) + 24.0).collect();
            let all_w = bws.iter().sum::<f32>() + 8.0 * (bws.len() as f32 - 1.0);
            let tx = x + 14.0 + 8.0 + 10.0;
            let text_w = (x + w - 14.0 - all_w - 14.0 - tx).max(40.0);
            let (title, hint) = match (device, &ctx.login_code) {
                (true, Some(code)) => (i18n::enter_code(ctx.lang, code), s.s.code_hint),
                (true, None) => (s.s.getting_code.to_string(), s.s.other_account),
                (false, _) => (i18n::waiting(ctx.lang, svc.name()), s.s.other_account),
            };
            let th = f.lay(&title, &ts, Some(text_w)).h;
            let hh = f.lay(hint, &hs, Some(text_w)).h;
            let bh = bs.lh + 12.0;
            let h = 12.0 + (th + 2.0 + hh).max(bh) + 12.0;
            let r = R::new(x, y, w, h);
            c.fill_rr(r, 14.0, col.pct(0.08));
            c.border(r, 14.0, 1.0, col.pct(0.4));
            c.glow_circle(x + 14.0 + 4.0, y + 12.0 + ts.lh / 2.0, 4.0, 0.0, 6.0, 0.0, col);
            c.fill_circle(x + 14.0 + 4.0, y + 12.0 + ts.lh / 2.0, 4.0, col);
            f.draw(c, tx, y + 12.0, &title, &ts, Some(text_w), 0.0);
            f.draw(c, tx, y + 12.0 + th + 2.0, hint, &hs, Some(text_w), 0.0);
            let mut bx = x + w - 14.0 - all_w;
            for ((label, hit), bw) in buttons.into_iter().zip(bws) {
                let b = R::new(bx, y + h / 2.0 - bh / 2.0, bw, bh);
                let hv = self.hov(&hit);
                c.fill_rr(b, 8.0, SURFACE2.lerp(Rgba::hex(0x252937), hv));
                c.border(b, 8.0, 1.0, LINE2);
                f.draw(c, b.x + 12.0, b.cy() - bs.lh / 2.0, label, &bs, None, 0.0);
                self.hits.push((b, hit));
                bx += bw + 8.0;
            }
            return h + 10.0;
        }
        if let Some(n) = &ctx.note {
            let st = Style::new(11.5, 600, if n.bad { WARN } else { OK }, 1.4);
            return f.draw(c, x + 4.0, y, &n.text, &st, Some(w - 8.0), 0.0).h + 10.0;
        }
        0.0
    }

    /// One account's `.svc` card; returns its height.
    #[allow(clippy::too_many_arguments)]
    fn account(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, a: &Acct, q: &Quota, x: f32, y: f32, w: f32) -> f32 {
        let s = i18n::t(ctx.lang);
        let svc = a.svc;
        let col = svc_color(svc);
        let on = ctx.settings.on(&a.id);
        // Just switched on (not checked yet) reads as checking.
        let r = (on && !q.disabled).then(|| q.clone());
        let hidden = ctx.settings.hidden_of(&a.id);

        // Body first (to know the height).
        let body_x = x + 1.0 + 14.0 + 43.0;
        let body_w = w - 2.0 - 28.0 - 43.0;
        let note = Style::new(11.5, 400, FAINT, 1.4);
        let chip_st = Style::new(12.0, 400, MUTED, 1.4);
        let pct_st = Style::new(11.0, 600, MUTED, 1.4).tab();
        let btn_st = Style::new(12.0, 600, Rgba::WHITE, 1.4);
        let btn_h = btn_st.lh + 10.0;
        let live = r.as_ref().filter(|q| q.ok && !q.windows.is_empty()).map(|q| q.windows.clone());
        let wins: Vec<Win> = live.clone().or_else(|| ctx.settings.seen.get(&a.id).cloned()).unwrap_or_default();
        enum Body {
            Note(String, bool),
            /// The windows, under a line about the latest check when it failed (a warning, or not when it only has
            /// to wait).
            Chips(Option<(String, bool)>),
            /// A login that ran out: why, and the button to sign in again.
            SignIn(String),
        }
        // Old numbers kept through "too many requests": nothing is wrong, the app just waits before asking again.
        let waiting = r.as_ref().filter(|q| q.ok && q.rate_limited());
        let expired = r.as_ref().and_then(|q| q.error.as_ref()).filter(|e| e.code == "appExpired");
        let body = if let Some(e) = expired {
            Body::SignIn(i18n::error_text(e, ctx.lang))
        } else if !wins.is_empty() {
            // Also when old numbers are shown because the latest check failed.
            let line = if let Some(q) = waiting {
                let when = q.retry.map(|rt| i18n::retry_at(ctx.lang, &crate::fmt::time(rt.at, ctx.lang), rt.manual));
                Some((when.unwrap_or_else(|| s.retry_hint.to_string()), false))
            } else {
                r.as_ref().and_then(|q| q.error.as_ref()).map(|e| (i18n::error_text(e, ctx.lang), true))
            };
            Body::Chips(if on { line } else { None })
        } else if let Some(e) = r.as_ref().filter(|q| on && !q.ok).and_then(|q| q.error.as_ref()) {
            Body::Note(i18n::error_text(e, ctx.lang), true)
        } else {
            Body::Note((if on { s.s.windows_pending } else { s.s.windows_off }).to_string(), false)
        };
        // Chip sizes and wrapping.
        let chip_h = chip_st.lh + 8.0 + 2.0;
        let mut chips: Vec<(R, Win, String, Option<(String, Rgba)>)> = Vec::new();
        let mut body_h;
        match &body {
            Body::Note(t, _) => body_h = f.lay(t, &note, Some(body_w)).h,
            Body::SignIn(t) => body_h = f.lay(t, &note, Some(body_w)).h + 8.0 + btn_h,
            Body::Chips(line) => {
                body_h = 0.0;
                if let Some((e, _)) = line {
                    body_h += f.lay(e, &note, Some(body_w)).h + 8.0;
                }
                let (mut cx, mut cy) = (0.0f32, body_h);
                for wdw in &wins {
                    let label = i18n::window_label(wdw, ctx.lang, false);
                    let pct = wdw.used_percent.map(|u| (format!("{}%", js_round(100.0 - u)), level_color(100.0 - u)));
                    let cw = 1.0 + 7.0 + 15.0 + 6.0 + f.width(&label, &chip_st) + pct.as_ref().map(|(t, _)| 6.0 + f.width(t, &pct_st)).unwrap_or(0.0) + 10.0 + 1.0;
                    if cx > 0.0 && cx + cw > body_w {
                        cx = 0.0;
                        cy += chip_h + 6.0;
                    }
                    chips.push((R::new(cx, cy, cw, chip_h), wdw.clone(), label, pct));
                    cx += cw + 6.0;
                }
                body_h = cy + chip_h;
            }
        }
        // .svc-head: the name + status block (36 px) is taller than the 32 px logo.
        // .svc-head: the name, then .status, an inline-flex box sitting on a 13 px / 1.4 line. Its baseline is its
        // first item's, the 6 px dot (an empty box: its bottom edge), so the line box comes out taller than the
        // status itself and the status sits lower in it.
        let strut = Style::new(13.0, 400, TEXT, 1.4);
        let strut_above = f.baseline(&strut);
        let strut_below = strut.lh - strut_above;
        let status_h = 11.5 * 1.4;
        let flex_above = 1.0 + (status_h + 6.0) / 2.0;
        let flex_below = 1.0 + status_h - flex_above;
        let status_line = strut_above.max(flex_above) + strut_below.max(flex_below);
        let status_top = strut_above.max(flex_above) - flex_above + 1.0;
        let head_h = (13.5f32 * 1.4 + status_line).max(32.0);
        let total = 1.0 + 12.0 + head_h + 11.0 + body_h + 12.0 + 1.0;
        let card = R::new(x, y, w, total);
        // The card itself is the drag handle (its buttons, switch and chips are hit first, being added later).
        let grab = Hit::Card(a.id.clone());
        let hv = self.hov(&grab);
        self.hits.push((card, grab));
        c.fill_rr(card, 14.0, SURFACE);
        let edge = if on { col.mix(0.28, LINE) } else { LINE };
        c.border(card, 14.0, 1.0, edge.lerp(LINE2.lerp(col, 0.35), hv));
        if on {
            let strip = R::new(x - 1.0 + 0.0, y + 14.0, 3.0, total - 28.0);
            c.shadow_rrect(strip, [0.0, 3.0, 3.0, 0.0], 0.0, 0.0, 10.0, 0.0, col.pct(0.6), false);
            c.fill_rrect(strip, [0.0, 3.0, 3.0, 0.0], col);
        }

        // Head.
        let hx = x + 1.0 + 14.0;
        let hy = y + 1.0 + 12.0;
        let mid = hy + head_h / 2.0;
        let logo = R::new(hx, mid - 16.0, 32.0, 32.0);
        let draw_logo = |c: &mut Canvas| {
            c.fill_rr(logo, 10.0, col.pct(0.16));
            c.shadow_rrect(logo, [10.0; 4], 0.0, 0.0, 0.0, 1.0, col.pct(0.25), true);
            icons::LOGO_TRACK.draw(c, logo.cx() - 9.0, logo.cy() - 9.0, 18.0, 18.0, col.fade(0.25));
            icons::LOGO_ARC.draw(c, logo.cx() - 9.0, logo.cy() - 9.0, 18.0, 18.0, col);
        };
        if on {
            draw_logo(c);
        } else {
            c.layer_in(logo, 0.55, Filter { grayscale: 0.9, brightness: 1.0 }, draw_logo);
        }
        let nx = hx + 32.0 + 11.0;
        let ns = Style::new(13.5, 600, TEXT, 1.4);
        let ss = Style::new(11.5, 400, MUTED, 1.4);
        let block = ns.lh + status_line;
        let ny = mid - block / 2.0;
        let nw = f.draw(c, nx, ny, svc.name(), &ns, None, 0.0).w;
        if let Some(q) = r.as_ref().filter(|q| q.ok && !q.plan.is_empty()) {
            let ps = Style::new(9.5, 700, col, 1.4).ls_em(0.06);
            let plan = q.plan.to_uppercase();
            let pw = f.width(&plan, &ps) + 12.0;
            let ph = ps.lh + 2.0;
            let pr = R::new(nx + nw + 7.0, ny + ns.lh / 2.0 - ph / 2.0, pw, ph);
            c.fill_rr(pr, 5.0, col.pct(0.15));
            f.draw(c, pr.x + 6.0, pr.y + 1.0, &plan, &ps, None, 0.0);
        }
        let (status, dot, scol) = if !on {
            (s.s.disabled, FAINT, MUTED)
        } else {
            match &r {
                None => (s.s.checking, FAINT, MUTED),
                Some(q) if q.absent => (s.s.absent, FAINT, MUTED),
                Some(q) if q.ok && q.rate_limited() => (s.s.waiting, FAINT, MUTED),
                Some(q) if !q.ok || q.error.is_some() => (s.s.failed, WARN, WARN),
                Some(_) => (s.s.connected, OK, MUTED),
            }
        };
        let sy = ny + ns.lh + status_top;
        if dot == OK {
            c.glow_circle(nx + 3.0, sy + ss.lh / 2.0, 3.0, 0.0, 5.0, 0.0, OK);
        }
        c.fill_circle(nx + 3.0, sy + ss.lh / 2.0, 3.0, dot);
        let status_w = f.draw(c, nx + 6.0 + 5.0, sy, status, &ss.color(scol), None, 0.0).w;

        // Right: sign out, then the switch.
        let sw_x = x + w - 1.0 - 14.0 - 38.0;
        let right;
        {
            let hit = Hit::SignOut(a.id.clone());
            let sure = self.sure.as_ref().is_some_and(|(id, _)| *id == a.id);
            let bs = Style::new(12.0, 500, if sure { BAD } else { MUTED }, 1.4);
            let label = if sure { s.s.sure } else { s.s.sign_out };
            let bw = f.width(label, &bs) + 22.0;
            let bh = bs.lh + 10.0;
            let b = R::new(sw_x - 12.0 - bw, mid - bh / 2.0, bw, bh);
            let hv = self.hov(&hit);
            if sure {
                c.fill_rr(b, 8.0, BAD.pct(0.12 + 0.06 * hv));
                c.border(b, 8.0, 1.0, BAD.pct(0.45));
            } else {
                c.fill_rr(b, 8.0, SURFACE.lerp(SURFACE2, hv));
                c.border(b, 8.0, 1.0, LINE2);
            }
            f.draw(c, b.x + 11.0, b.cy() - bs.lh / 2.0, label, &bs.color(if sure { BAD } else { MUTED.lerp(TEXT, hv) }), None, 0.0);
            self.hits.push((b, hit));
            right = b.x;
        }
        // Whose account it is, after the status.
        if !a.who.is_empty() {
            let wx = nx + 6.0 + 5.0 + status_w + 6.0;
            let line = f.ellipsize(&format!("· {}", a.who), &ss.color(FAINT), (right - 10.0 - wx).max(20.0));
            f.draw(c, wx, sy, &line, &ss.color(FAINT), None, 0.0);
        }
        let hit = Hit::Acct(a.id.clone());
        let pos = self.switch_pos(Sw::Acct(hkey(&hit)), on);
        self.switch(c, sw_x, mid, pos, on, col, hit);

        // Body.
        let by = hy + head_h + 11.0;
        match body {
            Body::Note(t, err) => {
                f.draw(c, body_x, by, &t, &note.color(if err { WARN } else { FAINT }), Some(body_w), 0.0);
            }
            Body::SignIn(t) => {
                let th = f.draw(c, body_x, by, &t, &note.color(WARN), Some(body_w), 0.0).h;
                let hit = Hit::SignIn(svc, a.id.clone());
                let hv = self.hov(&hit);
                let bw = f.width(s.s.sign_in, &btn_st) + 28.0;
                let b = R::new(body_x, by + th + 8.0, bw, btn_h);
                c.shadow(b, 8.0, 0.0, 2.0, 10.0, 0.0, col.pct(0.3));
                c.layer_in(b, 1.0, Filter { grayscale: 0.0, brightness: 1.0 + 0.12 * hv }, |c| c.fill_rr(b, 8.0, col));
                f.draw(c, b.x + 14.0, b.cy() - btn_st.lh / 2.0, s.s.sign_in, &btn_st, None, 0.0);
                self.hits.push((b, hit));
            }
            Body::Chips(line) => {
                if let Some((e, warn)) = line {
                    f.draw(c, body_x, by, &e, &note.color(if warn { WARN } else { FAINT }), Some(body_w), 0.0);
                }
                for (cr, wdw, label, pct) in chips {
                    let r = R::new(body_x + cr.x, by + cr.y, cr.w, cr.h);
                    let shown = !hidden.contains(&wdw.id);
                    let hit = Hit::Chip(a.id.clone(), wdw.id.clone());
                    let hv = if on { self.hov(&hit) } else { 0.0 };
                    let draw_chip = |c: &mut Canvas, f: &mut Fonts| {
                        if shown {
                            c.fill_rr(r, 9.0, col.pct(0.10));
                            c.border(r, 9.0, 1.0, col.pct(0.45));
                        } else {
                            if hv > 0.0 {
                                c.fill_rr(r, 9.0, Rgba::rgba(255, 255, 255, 0.05 * hv));
                            }
                            c.border(r, 9.0, 1.0, LINE2);
                        }
                        let bx = R::new(r.x + 1.0 + 7.0, r.cy() - 7.5, 15.0, 15.0);
                        if shown {
                            c.fill_rr(bx, 5.0, col);
                            icons::CHECK.draw(c, bx.cx() - 4.5, bx.cy() - 4.5, 9.0, 9.0, BG);
                        } else {
                            c.border(bx, 5.0, 1.5, FAINT);
                        }
                        let tc = if shown { TEXT } else { MUTED.lerp(TEXT, hv) };
                        let tx = bx.x + 15.0 + 6.0;
                        let lw = f.draw(c, tx, r.cy() - chip_st.lh / 2.0, &label, &chip_st.color(tc), None, 0.0).w;
                        if let Some((t, pc)) = &pct {
                            f.draw(c, tx + lw + 6.0, r.cy() - pct_st.lh / 2.0, t, &pct_st.color(*pc), None, 0.0);
                        }
                    };
                    if on {
                        draw_chip(c, f);
                        self.hits.push((r, hit));
                    } else {
                        c.layer_in(r, 0.45, Filter::NONE, |c| draw_chip(c, f));
                    }
                }
            }
        }
        total
    }

    /// The Updates row; returns its height.
    fn update_row(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, x: f32, y: f32, w: f32) -> f32 {
        let u = &ctx.update;
        let t = &i18n::t(ctx.lang).u;
        let l = ctx.lang;
        let mut title = i18n::current(l, &u.current);
        let mut hint = String::new();
        let mut err = false;
        let mut bar: Option<u32> = None;
        let version = u.version.clone().unwrap_or_default();
        match u.phase {
            Phase::Unsupported => hint = t.dev_only.into(),
            Phase::Checking => hint = t.checking.into(),
            Phase::Latest => title = i18n::up_to_date(l, &u.current),
            Phase::Available => {
                title = if u.skipped { i18n::skipped(l, &version) } else { i18n::ready(l, &version) };
                hint = if u.manual { t.manual_hint.to_string() } else { i18n::from_to(&u.current, &version) };
            }
            Phase::Downloading => {
                title = i18n::downloading(l, &version);
                hint = format!("{}%", u.percent);
                bar = Some(u.percent);
            }
            Phase::Installing => {
                title = i18n::installing(l, &version);
                hint = t.restart_note.into();
            }
            Phase::Error => {
                err = true;
                let (during, msg) = u.error.clone().unwrap_or((During::Check, String::new()));
                hint = format!("{} — {}", if during == During::Download { t.failed } else { t.check_failed }, msg);
            }
            Phase::Idle => {}
        }
        let (action, label) = upd_action(u);
        let label = label.map(|k| match k {
            "check" => t.check,
            "download" => t.download,
            "update" => t.update,
            "retry" => t.retry,
            _ => t.check,
        });
        let ts = Style::new(13.0, 500, TEXT, 1.4);
        let hs = Style::new(11.5, 400, if err { WARN } else { MUTED }, 1.4);
        let ms = Style::new(12.0, 600, TEXT, 1.4);
        let btn_w = label.map(|lb| f.width(lb, &ms) + 24.0 + if action == Some("download") { 0.0 } else { 2.0 }).unwrap_or(0.0);
        let text_w = (w - 28.0 - 30.0 - 14.0 - if btn_w > 0.0 { 14.0 + btn_w } else { 0.0 }).max(10.0);
        let hint_h = if hint.is_empty() { 0.0 } else { 1.0 + f.lay(&hint, &hs, Some(text_w)).h + if bar.is_some() { 6.0 + 4.0 } else { 0.0 } };
        let content = (ts.lh + hint_h).max(30.0);
        let rh = (content + 24.0).max(52.0);
        let row = R::new(x, y, w, rh);
        c.fill_rr(row, 14.0, SURFACE);
        let mid = y + rh / 2.0;
        let icon_r = R::new(x + 14.0, mid - 15.0, 30.0, 30.0);
        let fresh = matches!(u.phase, Phase::Available | Phase::Downloading | Phase::Installing);
        if fresh {
            c.fill_rr(icon_r, 9.0, linear(135.0, icon_r, &[(0.0, ACCENT), (1.0, ACCENT2)]));
        } else {
            c.fill_rr(icon_r, 9.0, SURFACE2);
        }
        let spin = matches!(u.phase, Phase::Checking | Phase::Installing);
        let rot = if spin { (self.t0.elapsed().as_secs_f32() % 1.0) * 360.0 } else { 0.0 };
        if spin {
            self.spin_from.get_or_insert(Instant::now());
            self.spin_rects.push(icon_r);
        }
        icons::UPDATE.draw_rot(c, icon_r.cx() - 8.0, icon_r.cy() - 8.0, 16.0, 16.0, if fresh { Rgba::WHITE } else { MUTED }, rot);
        let tx = x + 14.0 + 30.0 + 14.0;
        let ty = mid - content.min(ts.lh + hint_h) / 2.0;
        f.draw(c, tx, ty, &title, &ts, None, 0.0);
        if !hint.is_empty() {
            let lay = f.draw(c, tx, ty + ts.lh + 1.0, &hint, &hs, Some(text_w), 0.0);
            if let Some(p) = bar {
                let br = R::new(tx, ty + ts.lh + 1.0 + lay.h + 6.0, text_w, 4.0);
                c.fill_rr(br, 4.0, Rgba::rgba(255, 255, 255, 0.09));
                let bw = br.w * p as f32 / 100.0;
                if bw > 0.0 {
                    let fill = R::new(br.x, br.y, bw, 4.0);
                    c.fill_rr(fill, 4.0, linear(90.0, fill, &[(0.0, ACCENT), (1.0, ACCENT2)]));
                }
            }
        }
        if let Some(lb) = label {
            let bh = ms.lh + 12.0;
            let b = R::new(x + w - 14.0 - btn_w, mid - bh / 2.0, btn_w, bh);
            let hv = self.hov(&Hit::Upd);
            if action == Some("download") {
                c.shadow(b, 8.0, 0.0, 2.0, 10.0, 0.0, ACCENT.pct(0.35));
                let fill = linear(135.0, b, &[(0.0, ACCENT), (1.0, ACCENT2)]);
                c.layer_in(b, 1.0, Filter { grayscale: 0.0, brightness: 1.0 + 0.1 * hv }, |c| c.fill_rr(b, 8.0, fill));
                f.draw(c, b.x + 12.0, b.cy() - ms.lh / 2.0, lb, &ms.color(Rgba::WHITE), None, 0.0);
            } else {
                c.fill_rr(b, 8.0, SURFACE2.lerp(Rgba::hex(0x252937), hv));
                c.border(b, 8.0, 1.0, LINE2);
                f.draw(c, b.x + 13.0, b.cy() - ms.lh / 2.0, lb, &ms, None, 0.0);
            }
            self.hits.push((b, Hit::Upd));
        }
        rh
    }

    /// An action button; returns its height.
    #[allow(clippy::too_many_arguments)]
    fn button(&mut self, c: &mut Canvas, f: &mut Fonts, x: f32, y: f32, w: f32, icon: &Icon, text: &str, hit: Hit, danger: bool, rot: f32) -> f32 {
        let st = Style::new(12.5, 500, if danger { BAD } else { TEXT }, 1.4);
        let h = st.lh + 18.0 + 2.0;
        let hv = self.hov(&hit);
        let down = self.pressed.as_ref() == Some(&hit) && self.hover.as_ref() == Some(&hit);
        let r = R::new(x, y + if down { 1.0 } else { 0.0 }, w, h);
        let (bg, border) = if danger {
            (SURFACE.lerp(BAD.mix(0.12, SURFACE), hv), LINE2.lerp(BAD.pct(0.45), hv))
        } else {
            (SURFACE.lerp(SURFACE2, hv), LINE2.lerp(Rgba::rgba(255, 255, 255, 0.18), hv))
        };
        c.fill_rr(r, 10.0, bg);
        c.border(r, 10.0, 1.0, border);
        let tw = f.width(text, &st);
        let total = 14.0 + 7.0 + tw;
        let ix = r.cx() - total / 2.0;
        icon.draw_rot(c, ix, r.cy() - 7.0, 14.0, 14.0, if danger { BAD } else { MUTED }, rot);
        f.draw(c, ix + 14.0 + 7.0, r.cy() - st.lh / 2.0, text, &st, None, 0.0);
        self.hits.push((R::new(x, y, w, h), hit));
        h
    }
}

impl Default for AppWin {
    fn default() -> Self {
        Self::new()
    }
}

/// The Updates row's button: (action, label key).
fn upd_action(u: &View) -> (Option<&'static str>, Option<&'static str>) {
    match u.phase {
        Phase::Unsupported | Phase::Checking | Phase::Downloading | Phase::Installing => (None, None),
        Phase::Available => (Some("download"), Some(if u.manual { "download" } else { "update" })),
        Phase::Error => {
            if u.error.as_ref().is_some_and(|e| e.0 == During::Download) {
                (Some("download"), Some("retry"))
            } else {
                (Some("check"), Some("check"))
            }
        }
        _ => (Some("check"), Some("check")),
    }
}

/// Copies opaque picture `pic` (device px) into the canvas with its top-left at (x, y) DIP, to the nearest device
/// px and clipped to the canvas: rows copied as they are, no blending.
fn copy_pic(c: &mut Canvas, pic: &Pixmap, x: f32, y: f32) {
    let (dx, dy) = ((c.base.sx * x + c.base.tx).round() as i64, (c.base.sy * y + c.base.ty).round() as i64);
    let (cw, ch) = (c.pm.width() as i64, c.pm.height() as i64);
    let (pw, ph) = (pic.width() as i64, pic.height() as i64);
    let (x0, x1) = (dx.max(0), (dx + pw).min(cw));
    let (y0, y1) = (dy.max(0), (dy + ph).min(ch));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let n = ((x1 - x0) * 4) as usize;
    let src = pic.data();
    let dst = c.pm.data_mut();
    for row in y0..y1 {
        let s = (((row - dy) * pw + (x0 - dx)) * 4) as usize;
        let d = ((row * cw + x0) * 4) as usize;
        dst[d..d + n].copy_from_slice(&src[s..s + n]);
    }
}

/// The app icon (build/icon.png), decoded once.
pub fn icon_pixmap() -> &'static Option<Pixmap> {
    static ICON: OnceLock<Option<Pixmap>> = OnceLock::new();
    ICON.get_or_init(|| Pixmap::decode_png(include_bytes!("../../build/icon.png")).ok())
}

fn draw_app_icon(c: &mut Canvas, x: f32, y: f32, size: f32) {
    let Some(icon) = icon_pixmap() else { return };
    let k = size / icon.width() as f32;
    let ts = c.base.pre_concat(Transform::from_row(k, 0.0, 0.0, k, x, y));
    let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() };
    c.pm.draw_pixmap(0, 0, icon.as_ref(), &paint, ts, None);
}

#[allow(dead_code)]
fn _unused(_: Fill) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Switching off slides the knob from on to off (overshooting a little), never back toward on.
    #[test]
    fn switch_knob_goes_where_it_was_sent() {
        let mut aw = AppWin::new();
        assert_eq!(aw.switch_pos(Sw::Network, true), 1.0);
        for (on, from, to) in [(false, 1.0, 0.0), (true, 0.0, 1.0)] {
            let mut pos = aw.switch_pos(Sw::Network, on);
            assert!((pos - from).abs() < 1e-4, "starts where it was: {pos}");
            aw.step(0.016);
            let first = aw.switch_pos(Sw::Network, on);
            // The curve starts fast (about a third of the way after one frame), but toward `to`: the old easing put
            // it past `to` at once and then slid it back toward `from`.
            assert!((first - from).abs() < 0.5 && (to - first) * (to - from) > 0.0, "one frame in: {first}");
            for _ in 0..30 {
                aw.step(0.016);
                pos = aw.switch_pos(Sw::Network, on);
                assert!((pos - from).abs() <= 1.2, "{pos}");
            }
            assert!((pos - to).abs() < 1e-4, "ends at {to}: {pos}");
        }
    }
}
