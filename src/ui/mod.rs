//! Views drawn by hand in CSS px, matching the old pages (src/*.html on `old-electron`).
pub mod app_window;
pub mod card;
pub mod gfx;
pub mod icons;
pub mod panel;
pub mod svg;
pub mod text;
pub mod toast;

use crate::i18n::{self, Lang};
use crate::model::{Svc, Win};
use gfx::Rgba;

pub const TEXT: Rgba = Rgba::hex(0xeceef2);
pub const MUTED: Rgba = Rgba::hex(0x8b90a0);
pub const FAINT: Rgba = Rgba::hex(0x5d6272);
pub const OK: Rgba = Rgba::hex(0x4ade80);
pub const WARN: Rgba = Rgba::hex(0xfbbf24);
pub const BAD: Rgba = Rgba::hex(0xf87171);
pub const LINE: Rgba = Rgba::rgba(255, 255, 255, 0.08);
pub const TRACK: Rgba = Rgba::rgba(255, 255, 255, 0.12);
pub const ACCENT: Rgba = Rgba::hex(0x8b9cff);
pub const ACCENT2: Rgba = Rgba::hex(0xb38bff);

pub fn svc_color(s: Svc) -> Rgba {
    match s {
        Svc::Claude => Rgba::hex(0xd97757),
        Svc::Codex => Rgba::hex(0x10a37f),
        Svc::Cursor => Rgba::hex(0xa5b4fc),
        Svc::Copilot => Rgba::hex(0x58a6ff),
    }
}

/// Green above 50 % left, amber above 20 %, red otherwise.
pub fn level_color(left: f64) -> Rgba {
    if left > 50.0 {
        OK
    } else if left > 20.0 {
        WARN
    } else {
        BAD
    }
}

pub fn clamp_pct(v: f64) -> f64 {
    v.clamp(0.0, 100.0)
}

/// JS Math.round (halves round up).
pub fn js_round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

/// Label under a ring: time left until reset, as days to one decimal (1.3d), else whole hours or minutes; the
/// window name if there is no reset time. Rounded down, so it never shows more time than is left.
pub fn left_label(w: &Win, lang: Lang, now: i64) -> String {
    let Some(at) = w.resets_at else { return i18n::window_label(w, lang, true) };
    let m = ((at - now) as f64 / 60000.0).floor().max(0.0) as i64;
    if m >= 1440 {
        let d = (m as f64 / 144.0).floor() / 10.0;
        i18n::left_days(lang, &format!("{d:.1}"))
    } else if m >= 60 {
        i18n::left_hours(lang, m / 60)
    } else {
        i18n::left_minutes(lang, m)
    }
}

/// Inside the big ring only the span fits ("5h", "7d"); the model name goes in the panel header.
pub fn ring_label(w: &Win, lang: Lang) -> String {
    if w.span.is_some_and(|s| s > 0.0) || w.billing {
        let bare = Win { name: None, tag: None, ..w.clone() };
        i18n::window_label(&bare, lang, true)
    } else {
        i18n::window_label(w, lang, true)
    }
}

/// CSS ease-out / ease-in-out timing curves (cubic-bezier).
pub fn ease_out(t: f32) -> f32 {
    bezier(0.0, 0.0, 0.58, 1.0, t)
}
pub fn ease_in_out(t: f32) -> f32 {
    bezier(0.42, 0.0, 0.58, 1.0, t)
}
pub fn ease(t: f32) -> f32 {
    bezier(0.25, 0.1, 0.25, 1.0, t)
}

pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Solve x(s) = t for s, then return y(s).
    let bx = |s: f32| 3.0 * (1.0 - s).powi(2) * s * x1 + 3.0 * (1.0 - s) * s * s * x2 + s * s * s;
    let by = |s: f32| 3.0 * (1.0 - s).powi(2) * s * y1 + 3.0 * (1.0 - s) * s * s * y2 + s * s * s;
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if bx(mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    by((lo + hi) / 2.0)
}
