//! The app window (src/settings.html): custom dark title bar, General / Internet status / Services / Updates,
//! the action buttons and the footer. Laid out and painted in one pass; the pass records hit rectangles.
use super::gfx::{linear, Canvas, Fill, Filter, Rgba, R};
use super::svg::Icon;
use super::text::{Fonts, Style};
use super::{icons, js_round, level_color, svc_color, ACCENT, ACCENT2, BAD, FAINT, MUTED, OK, TEXT, WARN};
use crate::anim::Fades;
use crate::app::{Btn3, CursorKind, Mouse};
use crate::i18n::{self, Lang, LANGS};
use crate::model::{NetState, Snapshot, Svc, Win, SERVICES};
use crate::settings::Settings;
use crate::updater::{During, Phase, View};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;
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

pub struct Ctx {
    pub lang: Lang,
    pub settings: Settings,
    pub visible: bool,
    pub startup: Option<bool>,
    pub data: Option<Snapshot>,
    pub net: Option<NetState>,
    pub update: View,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Close,
    TitleBar,
    Visible,
    Lang(Lang),
    Layout(bool),
    Slider,
    Startup,
    Network,
    Service(Svc),
    Chip(Svc, String),
    Upd,
    Refresh,
    Snap,
    Restart,
    Quit,
    Thumb,
}

impl Hit {
    fn pointer(&self) -> bool {
        !matches!(self, Hit::TitleBar | Hit::Thumb)
    }
}

pub enum Action {
    Close,
    DragWindow,
    Lang(Lang),
    Visible(bool),
    Vertical(bool),
    Opacity(f64),
    Network(bool),
    Startup(bool),
    Service(Svc, bool),
    Window(Svc, String, bool),
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
    Svc(usize),
}

impl Sw {
    fn hit(self) -> Hit {
        match self {
            Sw::Visible => Hit::Visible,
            Sw::Startup => Hit::Startup,
            Sw::Network => Hit::Network,
            Sw::Svc(i) => Hit::Service(SERVICES[i]),
        }
    }
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
        match &self.hover {
            Some(h) if h.pointer() => CursorKind::Pointer,
            _ => CursorKind::Default,
        }
    }

    /// Advances animations; true while anything moves.
    pub fn step(&mut self, dt: f32) -> bool {
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
            self.damage_key(hkey(&k.hit()));
        }
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
        // transition: transform .18s cubic-bezier(.3, 1.4, .5, 1) (a little overshoot).
        let p = if e.1 >= e.0 { e.0 } else { 1.0 - e.0 };
        let eased = super::bezier(0.3, 1.4, 0.5, 1.0, if e.1 >= 0.5 { p } else { 1.0 - p });
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
                if !self.slider_drag && self.thumb_drag.is_none() {
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
                    _ => {}
                }
                self.damage_hit(&h);
                if self.slider_drag {
                    self.damage(self.slider_rect);
                }
            }
            Mouse::Up(Btn3::Left) => {
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
                    Hit::Startup => out.push(Action::Startup(!ctx.startup.unwrap_or(false))),
                    Hit::Network => out.push(Action::Network(!s.network)),
                    Hit::Service(svc) => out.push(Action::Service(svc, !s.services[svc.index()])),
                    Hit::Chip(svc, id) => {
                        let shown = !s.hidden[svc.index()].contains(&id);
                        out.push(Action::Window(svc, id, !shown));
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
                    Hit::TitleBar | Hit::Slider | Hit::Thumb => {}
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
        y += self.row(c, f, x0, ry, cw, &icons::EYE, s.s.show_overlay, Some(s.s.show_overlay_hint), &mut |me, c, f, right, mid| {
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
            y += self.row(c, f, x0, y, cw, &icons::POWER, s.s.startup, Some(s.s.startup_hint), &mut |me, c, _f, right, mid| {
                me.switch(c, right - 38.0, mid, pos, on, ACCENT, Hit::Startup);
                (38.0, 22.0)
            });
        }
        y += 1.0;
        self.group_frame(c, x0, rows_y, cw, y - rows_y, &rows);

        // ---------- Internet status ----------
        y += GAP;
        y = self.sec_head(c, f, x0, y, s.s.network);
        let gy = y;
        y += 1.0;
        let net_on = ctx.settings.network;
        let pos = self.switch_pos(Sw::Network, net_on);
        let net = ctx.net;
        let net_y = y;
        y += self.row(c, f, x0, y, cw, &icons::WIFI_ROW, s.s.network_show, Some(s.s.network_hint), &mut |me, c, f, right, mid| {
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
        self.group_frame(c, x0, gy, cw, y - gy, &[]);

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
        y += 10.0;
        for (i, svc) in SERVICES.iter().enumerate() {
            if i > 0 {
                y += 10.0;
            }
            y += self.service(c, f, ctx, *svc, x0, y, cw);
        }

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
        // there is room)
        let x0 = 16.0;
        let bw = (cw - 3.0 * 8.0) / 4.0;
        let bh = Style::new(12.5, 500, TEXT, 1.4).lh + 18.0 + 2.0;
        let y = (left_end + 16.0).max(right_end - bh);
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
        let right_end = y;

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
        hint: Option<&str>,
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
        let hh = hint.map(|h| 1.0 + f.lay(h, &hs, Some(text_w)).h).unwrap_or(0.0);
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
        if let Some(h) = hint {
            f.draw(c, tx, ty + th + 1.0, h, &hs, Some(text_w), 0.0);
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
        let mut x = r.x + 1.0 + 3.0;
        for ((ic, text, hit, pressed), bw) in items.iter().zip(widths) {
            let b = R::new(x, r.y + 4.0, bw, bh);
            if *pressed {
                c.shadow(b, 7.0, 0.0, 1.0, 3.0, 0.0, Rgba::rgba(0, 0, 0, 0.35));
                c.fill_rr(b, 7.0, SURFACE2);
                c.shadow_rrect(b, [7.0; 4], 0.0, 1.0, 0.0, 0.0, Rgba::rgba(255, 255, 255, 0.06), true);
            }
            let hv = self.hov(hit);
            let color = if *pressed { TEXT } else { MUTED.lerp(TEXT, hv) };
            let mut tx = b.x + 12.0;
            if let Some(icon) = ic {
                let (iw, ih) = (icon.vb.0, icon.vb.1);
                icon.draw(c, tx, b.cy() - ih / 2.0, iw, ih, color);
                tx += 14.0 + 6.0;
            }
            f.draw(c, tx, b.cy() - st.lh / 2.0, text, &st.color(color), None, 0.0);
            self.hits.push((b, hit.clone()));
            x += bw + 2.0;
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

    /// One `.svc` card; returns its height.
    #[allow(clippy::too_many_arguments)]
    fn service(&mut self, c: &mut Canvas, f: &mut Fonts, ctx: &Ctx, svc: Svc, x: f32, y: f32, w: f32) -> f32 {
        let s = i18n::t(ctx.lang);
        let col = svc_color(svc);
        let on = ctx.settings.services[svc.index()];
        let r = if on { ctx.data.as_ref().and_then(|d| d.get(svc)).cloned() } else { None };
        let hidden = &ctx.settings.hidden[svc.index()];

        // Body first (to know the height).
        let body_x = x + 1.0 + 14.0 + 43.0;
        let body_w = w - 2.0 - 28.0 - 43.0;
        let note = Style::new(11.5, 400, FAINT, 1.4);
        let chip_st = Style::new(12.0, 400, MUTED, 1.4);
        let pct_st = Style::new(11.0, 600, MUTED, 1.4).tab();
        let live = r.as_ref().filter(|q| q.ok && !q.windows.is_empty()).map(|q| q.windows.clone());
        let wins: Vec<Win> = live.clone().or_else(|| ctx.settings.seen[svc.index()].clone()).unwrap_or_default();
        enum Body {
            Note(String, bool),
            Chips(Option<String>),
        }
        let body = if on && r.as_ref().is_some_and(|q| q.absent) {
            Body::Note(
                match svc {
                    Svc::Claude => s.s.login_claude,
                    Svc::Codex => s.s.login_codex,
                    Svc::Cursor => s.s.login_cursor,
                }
                .to_string(),
                false,
            )
        } else if !wins.is_empty() {
            let err = r.as_ref().filter(|q| !q.ok).and_then(|q| q.error.as_ref()).map(|e| i18n::error_text(e, ctx.lang));
            Body::Chips(if on { err } else { None })
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
            Body::Chips(err) => {
                body_h = 0.0;
                if let Some(e) = err {
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
        c.fill_rr(card, 14.0, SURFACE);
        c.border(card, 14.0, 1.0, if on { col.mix(0.28, LINE) } else { LINE });
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
                Some(q) if !q.ok => (s.s.failed, WARN, WARN),
                Some(_) => (s.s.connected, OK, MUTED),
            }
        };
        let sy = ny + ns.lh + status_top;
        if dot == OK {
            c.glow_circle(nx + 3.0, sy + ss.lh / 2.0, 3.0, 0.0, 5.0, 0.0, OK);
        }
        c.fill_circle(nx + 3.0, sy + ss.lh / 2.0, 3.0, dot);
        f.draw(c, nx + 6.0 + 5.0, sy, status, &ss.color(scol), None, 0.0);
        let pos = self.switch_pos(Sw::Svc(svc.index()), on);
        self.switch(c, x + w - 1.0 - 14.0 - 38.0, mid, pos, on, col, Hit::Service(svc));

        // Body.
        let by = hy + head_h + 11.0;
        match body {
            Body::Note(t, err) => {
                f.draw(c, body_x, by, &t, &note.color(if err { WARN } else { FAINT }), Some(body_w), 0.0);
            }
            Body::Chips(err) => {
                if let Some(e) = err {
                    f.draw(c, body_x, by, &e, &note.color(WARN), Some(body_w), 0.0);
                }
                for (cr, wdw, label, pct) in chips {
                    let r = R::new(body_x + cr.x, by + cr.y, cr.w, cr.h);
                    let shown = !hidden.contains(&wdw.id);
                    let hit = Hit::Chip(svc, wdw.id.clone());
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
