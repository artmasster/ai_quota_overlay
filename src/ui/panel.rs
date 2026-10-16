//! The detail panel (`.detail` in src/index.html) of the ring that was clicked.
use super::card::central_text;
use super::gfx::{Canvas, Rgba, R};
use super::text::{Fonts, Style};
use super::{clamp_pct, icons, js_round, level_color, ring_label, svc_color, LINE, MUTED, TEXT, TRACK, WARN};
use crate::fmt;
use crate::i18n::{self, Lang};
use crate::model::{Quota, Svc};

pub const W: f32 = 248.0;
const CONTENT_X: f32 = 13.0;
const CONTENT_W: f32 = W - 26.0;

pub struct PanelIn<'a> {
    pub lang: Lang,
    pub svc: Svc,
    pub q: &'a Quota,
    pub index: i32,
    pub now: i64,
}

/// Hover amount of the × button.
#[derive(Default, Clone, Copy)]
pub struct PanelFx {
    pub close_hover: f32,
}

pub struct PanelLayout {
    pub h: f32,
    pub close: R,
}

fn head_name() -> Style {
    Style::new(12.5, 600, TEXT, 1.3)
}
fn badge_style(c: Rgba) -> Style {
    Style::new(9.5, 600, c, 1.3).ls_em(0.06)
}
fn win_style() -> Style {
    Style::new(11.0, 400, MUTED, 1.3)
}
fn err_style() -> Style {
    Style::new(11.5, 400, WARN, 1.45)
}
fn hint_style() -> Style {
    Style::new(10.5, 400, MUTED, 1.3)
}
fn cap_style() -> Style {
    Style::new(11.0, 400, MUTED, 1.3)
}
fn row_style() -> Style {
    Style::new(11.5, 400, TEXT, 1.3)
}

fn window<'a>(inp: &PanelIn<'a>) -> Option<&'a crate::model::Win> {
    if inp.q.ok && inp.index >= 0 {
        inp.q.windows.get(inp.index as usize)
    } else {
        None
    }
}

/// Rows under the ring: (icon, label, bold value, extra plain text after the value, right-aligned text).
fn rows(inp: &PanelIn) -> Vec<(super::svg::Icon, String, String, String, String)> {
    let t = i18n::t(inp.lang);
    let mut rows = Vec::new();
    let Some(w) = window(inp) else { return rows };
    if let Some(at) = w.resets_at {
        rows.push((icons::CLOCK, t.resets_in.to_string(), fmt::until(at, inp.now, inp.lang), String::new(), fmt::when(at, inp.now, inp.lang)));
    }
    if inp.svc == Svc::Codex && inp.q.credits.is_some() {
        let b = inp.q.credits.unwrap_or(0.0).floor() as i64;
        rows.push((icons::COIN, t.credits_left.to_string(), fmt::grouped(b), String::new(), String::new()));
    } else if let Some(x) = &inp.q.extra {
        rows.push((icons::COIN, t.extra_usage.to_string(), format!("${:.2}", x.used / 100.0), format!(" / ${:.0}", x.limit / 100.0), String::new()));
    }
    rows
}

pub fn layout(f: &mut Fonts, inp: &PanelIn) -> PanelLayout {
    let t = i18n::t(inp.lang);
    let mut y = 11.0 + 20.0 + 8.0;
    if !inp.q.ok {
        let msg = inp.q.error.as_ref().map(|e| i18n::error_text(e, inp.lang)).unwrap_or_default();
        y += f.lay(&msg, &err_style(), Some(CONTENT_W)).h;
        y += 6.0 + f.lay(t.retry_hint, &hint_style(), Some(CONTENT_W)).h;
    } else if window(inp).is_none() {
        y += cap_style().lh;
    } else {
        y += 58.0;
        let n = rows(inp).len();
        if n > 0 {
            y += 9.0 + 1.0 + 8.0 + n as f32 * row_style().lh + (n as f32 - 1.0) * 4.0;
        }
    }
    let h = y + 11.0 + 1.0;
    PanelLayout { h, close: R::new(CONTENT_X + CONTENT_W - 20.0, 11.0, 20.0, 20.0) }
}

pub fn draw(c: &mut Canvas, f: &mut Fonts, l: &PanelLayout, inp: &PanelIn, fx: &PanelFx) {
    let t = i18n::t(inp.lang);
    let accent = svc_color(inp.svc);
    let bx = R::new(0.0, 0.0, W, l.h);
    // 1 px dark outline outside, panel, border, accent strip.
    c.shadow(bx, 14.0, 0.0, 0.0, 0.0, 1.0, Rgba::rgba(0, 0, 0, 0.25));
    c.fill_rr(bx.inset(0.5, 0.5, 0.5, 0.5), 13.5, Rgba::rgba(22, 24, 31, 0.96));
    c.border(bx, 14.0, 1.0, LINE);
    c.fill_rrect(R::new(15.0, 1.0, W - 30.0, 2.0), [0.0, 0.0, 2.0, 2.0], accent);

    // ---------- head ----------
    let hy = 11.0;
    let mid = hy + 10.0;
    let mut x = CONTENT_X;
    c.fill_circle(x + 4.0, mid, 4.0, accent);
    x += 8.0 + 6.0;
    let ns = head_name();
    let nw = f.width(inp.svc.name(), &ns);
    f.draw(c, x, mid - ns.lh / 2.0, inp.svc.name(), &ns, None, 0.0);
    x += nw + 6.0;
    if inp.q.ok && !inp.q.plan.is_empty() {
        let bs = badge_style(accent);
        let plan = inp.q.plan.to_uppercase();
        let bw = f.width(&plan, &bs) + 12.0;
        let bh = bs.lh + 2.0;
        c.fill_rr(R::new(x, mid - bh / 2.0, bw, bh), 6.0, accent.pct(0.16));
        f.draw(c, x + 6.0, mid - bh / 2.0 + 1.0, &plan, &bs, None, 0.0);
        x += bw + 6.0;
    }
    if let Some(w) = window(inp) {
        let ws = win_style();
        let max = l.close.x - 6.0 - x;
        let label = f.ellipsize(&i18n::window_label(w, inp.lang, false), &ws, max);
        f.draw(c, x, mid - ws.lh / 2.0, &label, &ws, None, 0.0);
    }
    if fx.close_hover > 0.0 {
        c.fill_rr(l.close, 6.0, Rgba::rgba(255, 255, 255, 0.08 * fx.close_hover));
    }
    let xs = Style::new(15.0, 400, MUTED.lerp(TEXT, fx.close_hover), 1.0).lh_px(20.0);
    let xw = f.width("×", &xs);
    f.draw(c, l.close.cx() - xw / 2.0, l.close.y, "×", &xs, None, 0.0);

    let y = hy + 20.0 + 8.0;
    if !inp.q.ok {
        let msg = inp.q.error.as_ref().map(|e| i18n::error_text(e, inp.lang)).unwrap_or_default();
        let lay = f.draw(c, CONTENT_X, y, &msg, &err_style(), Some(CONTENT_W), 0.0);
        f.draw(c, CONTENT_X, y + lay.h + 6.0, t.retry_hint, &hint_style(), Some(CONTENT_W), 0.0);
        return;
    }
    let Some(w) = window(inp) else {
        f.draw(c, CONTENT_X, y, t.no_quota, &cap_style(), None, 0.0);
        return;
    };

    // ---------- big ring and numbers ----------
    let left = clamp_pct(100.0 - w.used());
    let color = level_color(left);
    let (rcx, rcy) = (CONTENT_X + 29.0, y + 29.0);
    c.ring(rcx, rcy, 25.0, 6.0, TRACK, (left / 100.0) as f32, if left > 0.0 { color } else { Rgba::TRANSPARENT });
    central_text(c, f, rcx, rcy, &ring_label(w, inp.lang), &Style::new(11.0, 600, MUTED, 1.3));
    let bx0 = CONTENT_X + 58.0 + 12.0;
    let big = Style::new(26.0, 700, color, 1.0).tab();
    let small = Style::new(13.0, 600, color, 1.0);
    let cap = cap_style();
    let block_h = big.lh + 3.0 + cap.lh;
    let by = y + (58.0 - block_h) / 2.0;
    let num = js_round(left).to_string();
    let base = by + f.baseline(&big);
    let nw = f.draw_at_baseline(c, bx0, base, &num, &big, 0.0).w;
    f.draw_at_baseline(c, bx0 + nw + 1.0, base, "%", &small, 0.0);
    f.draw(c, bx0, by + big.lh + 3.0, &i18n::left_used(inp.lang, js_round(w.used())), &cap, None, 0.0);

    // ---------- rows ----------
    let rows = rows(inp);
    if rows.is_empty() {
        return;
    }
    let mut ry = y + 58.0 + 9.0;
    c.fill_rr(R::new(CONTENT_X, ry, CONTENT_W, 1.0), 0.0, LINE);
    ry += 1.0 + 8.0;
    let rs = row_style();
    let bold = rs.weight(700);
    let when = cap_style();
    for (icon, label, value, after, right) in rows {
        let mid = ry + rs.lh / 2.0;
        icon.draw(c, CONTENT_X, mid - 6.5, 13.0, 13.0, TEXT.fade(0.75));
        let mut x = CONTENT_X + 13.0 + 7.0;
        let lw = f.width(&format!("{label} "), &rs);
        f.draw(c, x, ry, &label, &rs, None, 0.0);
        x += lw;
        let vw = f.draw(c, x, ry, &value, &bold, None, 0.0).w;
        x += vw;
        if !after.is_empty() {
            f.draw(c, x, ry, &after, &rs, None, 0.0);
        }
        if !right.is_empty() {
            let rw = f.width(&right, &when);
            f.draw(c, CONTENT_X + CONTENT_W - rw, mid - when.lh / 2.0, &right, &when, None, 0.0);
        }
        ry += rs.lh + 4.0;
    }
}
