//! The update toast (src/toast.html): available / downloading / installing / download error.
use super::gfx::{linear, Canvas, Fill, Rgba, R};
use super::text::{Fonts, Style};
use super::{icons, ACCENT, ACCENT2, BAD, LINE, MUTED, TEXT};
use crate::i18n::{self, Lang};
use tiny_skia::{GradientStop, LinearGradient, Paint, Point, SpreadMode, Transform};

pub const W: f32 = 236.0;
const CX: f32 = 13.0;
const CW: f32 = W - 26.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Available,
    Downloading,
    Installing,
    Error,
}

pub struct ToastIn<'a> {
    pub lang: Lang,
    pub kind: Kind,
    pub current: &'a str,
    pub version: &'a str,
    pub percent: u32,
    pub manual: bool,
    pub error: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Btn {
    Close,
    Primary,
    Skip,
}

#[derive(Default, Clone, Copy)]
pub struct ToastFx {
    pub hover_close: f32,
    pub hover_primary: f32,
    pub hover_skip: f32,
    /// Seconds since the toast state began (bob, spin, bar flow).
    pub t: f32,
    /// Bar width shown, 0..100 (eased toward the real percent).
    pub bar: f32,
}

pub struct ToastLayout {
    pub h: f32,
    head_h: f32,
    pub close: Option<R>,
    pub primary: Option<R>,
    pub skip: Option<R>,
    bar_y: Option<f32>,
    btn_lines: (Vec<f32>, Vec<f32>),
}

impl ToastLayout {
    pub fn hit(&self, x: f32, y: f32) -> Option<Btn> {
        if self.close.is_some_and(|r| r.contains(x, y)) {
            return Some(Btn::Close);
        }
        if self.primary.is_some_and(|r| r.contains(x, y)) {
            return Some(Btn::Primary);
        }
        if self.skip.is_some_and(|r| r.contains(x, y)) {
            return Some(Btn::Skip);
        }
        None
    }
}

fn title_style() -> Style {
    Style::new(12.0, 600, TEXT, 1.35)
}
fn sub_style() -> Style {
    Style::new(10.5, 400, MUTED, 1.35).tab()
}
fn pct_style() -> Style {
    Style::new(11.5, 700, TEXT, 1.35).tab()
}
fn btn_style() -> Style {
    Style::new(11.0, 600, MUTED, 1.35)
}

struct Texts {
    title: String,
    sub: String,
    primary: Option<String>,
    skip: Option<String>,
    closable: bool,
    bar: bool,
}

fn texts(inp: &ToastIn) -> Texts {
    let u = &i18n::t(inp.lang).u;
    match inp.kind {
        Kind::Available => Texts {
            title: u.available.into(),
            sub: i18n::from_to(inp.current, inp.version),
            primary: Some(if inp.manual { u.download } else { u.update }.into()),
            skip: Some(u.skip.into()),
            closable: true,
            bar: false,
        },
        Kind::Downloading => Texts {
            title: i18n::downloading(inp.lang, inp.version),
            sub: format!("{}%", inp.percent),
            primary: None,
            skip: None,
            closable: false,
            bar: true,
        },
        Kind::Installing => Texts {
            title: i18n::installing(inp.lang, inp.version),
            sub: u.restart_note.into(),
            primary: None,
            skip: None,
            closable: false,
            bar: true,
        },
        Kind::Error => Texts {
            title: u.failed.into(),
            sub: inp.error.into(),
            primary: Some(u.retry.into()),
            skip: Some(u.skip.into()),
            closable: true,
            bar: false,
        },
    }
}

pub fn layout(f: &mut Fonts, inp: &ToastIn) -> ToastLayout {
    let tx = texts(inp);
    let head_h = (title_style().lh + sub_style().lh).max(30.0);
    let mut y = 1.0 + 11.0 + head_h;
    let close = tx.closable.then(|| R::new(CX + CW + 4.0 - 20.0, 12.0 - 3.0, 20.0, 20.0));
    let (mut primary, mut skip, mut bar_y) = (None, None, None);
    let mut btn_lines = (Vec::new(), Vec::new());
    if let (Some(p), Some(s)) = (&tx.primary, &tx.skip) {
        y += 10.0;
        let bw = (CW - 6.0) / 2.0;
        let bs = btn_style();
        let lp = f.lay(p, &bs, Some(bw - 16.0));
        let ls = f.lay(s, &bs, Some(bw - 16.0));
        let bh = lp.h.max(ls.h) + 12.0;
        primary = Some(R::new(CX, y, bw, bh));
        skip = Some(R::new(CX + bw + 6.0, y, bw, bh));
        btn_lines = (lp.lines, ls.lines);
        y += bh;
    } else if tx.bar {
        y += 10.0;
        bar_y = Some(y);
        y += 5.0;
    }
    ToastLayout { h: y + 12.0 + 1.0, head_h, close, primary, skip, bar_y, btn_lines }
}

pub fn draw(c: &mut Canvas, f: &mut Fonts, l: &ToastLayout, inp: &ToastIn, fx: &ToastFx) {
    let tx = texts(inp);
    let err = inp.kind == Kind::Error;
    let bx = R::new(0.0, 0.0, W, l.h);
    c.shadow(bx, 14.0, 0.0, 0.0, 0.0, 1.0, Rgba::rgba(0, 0, 0, 0.25));
    c.fill_rr(bx.inset(0.5, 0.5, 0.5, 0.5), 13.5, Rgba::rgba(22, 24, 31, 0.97));
    c.border(bx, 14.0, 1.0, LINE);
    let strip = R::new(15.0, 1.0, W - 30.0, 2.0);
    let strip_fill = if err { Fill::Solid(BAD) } else { linear(90.0, strip, &[(0.0, ACCENT), (1.0, ACCENT2)]) };
    c.fill_rrect(strip, [0.0, 0.0, 2.0, 2.0], strip_fill);

    // ---------- head ----------
    let hy = 12.0;
    let badge = R::new(CX, hy + (l.head_h - 30.0) / 2.0, 30.0, 30.0);
    if err {
        let bad = BAD.mix(0.85, Rgba::hex(0x000000));
        c.shadow(badge, 10.0, 0.0, 2.0, 10.0, 0.0, BAD.pct(0.4));
        c.fill_rr(badge, 10.0, bad);
    } else {
        c.shadow(badge, 10.0, 0.0, 2.0, 10.0, 0.0, ACCENT.pct(0.45));
        c.fill_rr(badge, 10.0, linear(135.0, badge, &[(0.0, ACCENT), (1.0, ACCENT2)]));
    }
    let (icon, mut dy, mut rot) = match inp.kind {
        Kind::Available => (icons::UP, 0.0, 0.0),
        Kind::Downloading => (icons::DOWN, 0.0, 0.0),
        Kind::Installing => (icons::SPIN, 0.0, 0.0),
        Kind::Error => (icons::WARN, 0.0, 0.0),
    };
    if inp.kind == Kind::Downloading {
        // @keyframes bob 1.1 s ease-in-out: down 2 px at the middle.
        let ph = (fx.t % 1.1) / 1.1 * 2.0;
        dy = 2.0 * super::ease_in_out(if ph <= 1.0 { ph } else { 2.0 - ph });
    }
    if inp.kind == Kind::Installing {
        rot = (fx.t % 0.9) / 0.9 * 360.0;
    }
    icon.draw_rot(c, badge.cx() - 8.0, badge.cy() - 8.0 + dy, 16.0, 16.0, Rgba::WHITE, rot);

    let titles_x = CX + 30.0 + 9.0;
    let titles_w = CW - 39.0 - if tx.closable { 9.0 + 16.0 } else { 0.0 };
    let ts = title_style();
    let ss = sub_style();
    let ty = hy + (l.head_h - ts.lh - ss.lh) / 2.0;
    f.draw(c, titles_x, ty, &tx.title, &ts, None, 0.0);
    if inp.kind == Kind::Downloading {
        f.draw(c, titles_x, ty + ts.lh, &tx.sub, &pct_style(), None, 0.0);
    } else {
        let sub = f.ellipsize(&tx.sub, &ss, titles_w);
        f.draw(c, titles_x, ty + ts.lh, &sub, &ss, None, 0.0);
    }
    if let Some(cr) = l.close {
        if fx.hover_close > 0.0 {
            c.fill_rr(cr, 6.0, Rgba::rgba(255, 255, 255, 0.08 * fx.hover_close));
        }
        let xs = Style::new(15.0, 400, MUTED.lerp(TEXT, fx.hover_close), 1.0).lh_px(20.0);
        let xw = f.width("×", &xs);
        f.draw(c, cr.cx() - xw / 2.0, cr.y, "×", &xs, None, 0.0);
    }

    // ---------- buttons ----------
    if let (Some(p), Some(s), Some(pt), Some(st)) = (l.primary, l.skip, &tx.primary, &tx.skip) {
        let prim = linear(135.0, p, &[(0.0, ACCENT), (1.0, ACCENT2)]);
        let hp = fx.hover_primary;
        c.layer(1.0, super::gfx::Filter { grayscale: 0.0, brightness: 1.0 + 0.1 * hp }, None, 0.0, |c| c.fill_rr(p, 8.0, prim));
        let bs = btn_style();
        let n = l.btn_lines.0.len().max(1) as f32;
        f.draw(c, p.x + 8.0, p.y + (p.h - n * bs.lh) / 2.0, pt, &bs.color(Rgba::WHITE), Some(p.w - 16.0), 0.5);
        let hs = fx.hover_skip;
        c.fill_rr(s, 8.0, Rgba::rgba(255, 255, 255, 0.06 + 0.05 * hs));
        let n = l.btn_lines.1.len().max(1) as f32;
        f.draw(c, s.x + 8.0, s.y + (s.h - n * bs.lh) / 2.0, st, &bs.color(MUTED.lerp(TEXT, hs)), Some(s.w - 16.0), 0.5);
    }

    // ---------- progress bar ----------
    if let Some(by) = l.bar_y {
        let track = R::new(CX, by, CW, 5.0);
        c.fill_rr(track, 5.0, Rgba::rgba(255, 255, 255, 0.09));
        let pct = if inp.kind == Kind::Installing { 100.0 } else { fx.bar };
        let w = CW * pct.clamp(0.0, 100.0) / 100.0;
        if w > 0.0 {
            let bar = R::new(CX, by, w, 5.0);
            // background: linear-gradient(90deg, a, b, a) at 200 % size, flowing one period every 1.2 s.
            let period = 2.0 * w;
            let off = period * ((fx.t % 1.2) / 1.2);
            if let Some(path) = super::gfx::rr(bar, 5.0) {
                let start = Point::from_xy(bar.x + off, 0.0);
                let end = Point::from_xy(bar.x + off + period, 0.0);
                let stops = vec![
                    GradientStop::new(0.0, ACCENT.sk()),
                    GradientStop::new(0.5, ACCENT2.sk()),
                    GradientStop::new(1.0, ACCENT.sk()),
                ];
                if let Some(shader) = LinearGradient::new(start, end, stops, SpreadMode::Repeat, Transform::identity()) {
                    let paint = Paint { shader, anti_alias: true, ..Paint::default() };
                    c.pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, c.base, None);
                }
            }
        }
    }
}
