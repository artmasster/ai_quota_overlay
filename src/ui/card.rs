//! The compact card (`.card` in src/index.html): one column of rings per account, the optional internet light
//! and the "⋯" button, laid out like the page's flexbox, horizontal or vertical. A service with more than one
//! account on the card puts each account's name (an email's part before the @) on a line under the service's.
use super::gfx::{Canvas, Filter, Rgba, R};
use super::text::{Fonts, Style};
use super::{icons, js_round, left_label, level_color, svc_color, BAD, LINE, MUTED, OK, TEXT, TRACK, WARN};
use crate::i18n::{self, Lang};
use crate::model::{short_who, Aid, NetState, Snapshot, Svc};
use crate::settings::Settings;
use std::collections::HashMap;

/// What a point on the card is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A ring: account and window index (-1 = the error / no-data ring).
    Ring(Aid, i32),
    More,
    Net,
    /// The "add account" / "pick accounts" ring shown when nothing else is (opens the app window).
    Prompt,
    /// Anywhere else on the card.
    Card,
}

pub struct CardIn<'a> {
    pub lang: Lang,
    pub vertical: bool,
    pub network_on: bool,
    /// Which accounts are on and which of their windows are left off.
    pub settings: &'a Settings,
    pub data: Option<&'a Snapshot>,
    pub net: Option<NetState>,
    pub selected: Option<(Aid, i32)>,
    pub now: i64,
}

/// Animation inputs: hover amounts (0..1, eased by the caller), offline alarm phase in seconds.
#[derive(Default)]
pub struct CardFx {
    pub hover: HashMap<Target, f32>,
    /// `.card:hover` (border and the "⋯" opacity).
    pub card_hover: f32,
    pub alarm_s: f32,
}

impl CardFx {
    fn amt(&self, t: Target) -> f32 {
        self.hover.get(&t).copied().unwrap_or(0.0)
    }
}

const PAD_H: [f32; 4] = [6.0, 9.0, 5.0, 9.0];
const PAD_V: [f32; 4] = [7.0, 6.0, 5.0, 6.0];
const RING: f32 = 36.0;

fn name_style(color: Rgba) -> Style {
    Style::new(10.0, 600, color, 1.3).ls_em(0.02)
}
fn lbl_style() -> Style {
    Style::new(9.5, 400, MUTED, 1.3)
}
fn ring_text_style() -> Style {
    Style::new(10.5, 700, TEXT, 1.3).tab()
}
fn empty_style() -> Style {
    Style::new(11.0, 400, MUTED, 1.3)
}
/// The account line under a service name.
fn who_style() -> Style {
    Style::new(9.5, 400, MUTED, 1.3)
}

struct RingBox {
    target: Target,
    svc: Option<Svc>,
    r: R,
    left: f64,
    text: String,
    label: String,
}

enum Item {
    /// gap: room under the name (the account line of the columns beside it).
    Net { r: R, online: bool, name_w: f32, lbl: String, gap: f32 },
    /// stale: old numbers shown because the latest check failed (a small amber dot after the name). who: which of
    /// the service's accounts it is (a line under the name), when it has more than one here.
    Svc { r: R, svc: Svc, who: Option<String>, name_w: f32, rings: Vec<RingBox>, divider: bool, stale: bool },
    /// The first check isn't back yet ("…").
    Empty { r: R, text: String, after_net: bool },
    /// Nothing to show: an empty ring with a "+" in place of the accounts, laid out like one (`tip` is the
    /// tooltip saying why; `top`: room above the ring).
    Prompt { r: R, label: String, tip: &'static str, divider: bool, top: f32 },
    More { r: R },
}

pub struct CardLayout {
    pub w: f32,
    pub h: f32,
    items: Vec<Item>,
    vertical: bool,
}

impl CardLayout {
    /// What's under (x, y), card-relative CSS px.
    pub fn hit(&self, x: f32, y: f32) -> Option<Target> {
        if !R::new(0.0, 0.0, self.w, self.h).contains(x, y) {
            return None;
        }
        for it in &self.items {
            match it {
                Item::Svc { rings, .. } => {
                    for rb in rings {
                        if rb.r.contains(x, y) {
                            return Some(rb.target);
                        }
                    }
                }
                Item::More { r } if r.contains(x, y) => return Some(Target::More),
                Item::Net { r, .. } if r.contains(x, y) => return Some(Target::Net),
                Item::Prompt { r, .. } if r.contains(x, y) => return Some(Target::Prompt),
                _ => {}
            }
        }
        Some(Target::Card)
    }

    /// Ring targets currently on the card (to drop a selection whose ring went away).
    pub fn rings(&self) -> Vec<Target> {
        let mut v = Vec::new();
        for it in &self.items {
            if let Item::Svc { rings, .. } = it {
                v.extend(rings.iter().map(|r| r.target));
            }
        }
        v
    }

    /// The tooltip of the item under (x, y), as the page's `title` attributes gave.
    pub fn title(&self, t: Target, lang: Lang, net: Option<NetState>) -> Option<&'static str> {
        let s = i18n::t(lang);
        match t {
            Target::More => Some(s.more),
            Target::Net => Some(if net.is_some_and(|n| n.online) { s.online } else { s.offline_title }),
            Target::Prompt => self.items.iter().find_map(|it| match it {
                Item::Prompt { tip, .. } => Some(*tip),
                _ => None,
            }),
            _ => None,
        }
    }
}

/// Lays the card out; (0, 0) is the card's top-left border corner.
pub fn layout(f: &mut Fonts, inp: &CardIn) -> CardLayout {
    let t = i18n::t(inp.lang);
    let v = inp.vertical;
    // Each item measured as (width, height) of its margin box, built at the origin and moved into place below.
    let mut items: Vec<Item> = Vec::new();

    // `gap`: room under the name, to keep the light level with rings that have an account line above them.
    let net_item = |f: &mut Fonts, gap: f32| -> Option<Item> {
        let n = inp.net?;
        if !inp.network_on {
            return None;
        }
        let lbl = if n.online { format!("{} ms", n.ms.unwrap_or(0)) } else { t.offline.to_string() };
        let mut ls = lbl_style();
        let mut ns = name_style(OK);
        if !n.online {
            ls = ls.weight(700);
            ns = ns.weight(700);
        }
        let name_w = f.width(t.net, &ns);
        let lbl_w = f.width(&lbl, &ls);
        let light_w = RING.max(lbl_w) + 4.0;
        let w = name_w.max(light_w);
        let h = ns.lh + gap + 2.0 + 2.0 + RING + 1.0 + ls.lh + 1.0;
        Some(Item::Net { r: R::new(0.0, 0.0, w, h), online: n.online, name_w, lbl, gap })
    };

    let more_item = || Item::More { r: R::default() };

    match inp.data {
        None => {
            if let Some(n) = net_item(f, 0.0) {
                items.push(n);
            }
            items.push(empty_item(f, "…", v, matches!(items.last(), Some(Item::Net { .. }))));
            items.push(more_item());
        }
        Some(data) => {
            // An account shows when it's switched on, logged in, and has at least one window left on (or an error).
            let mut shown = Vec::new();
            for (a, r) in &data.accts {
                if !inp.settings.on(&a.id) || r.absent {
                    continue;
                }
                let hidden = inp.settings.hidden_of(&a.id);
                let wins: Vec<(usize, &crate::model::Win)> = if r.ok {
                    r.windows.iter().enumerate().filter(|(_, w)| !hidden.contains(&w.id)).collect()
                } else {
                    Vec::new()
                };
                if r.ok && !r.windows.is_empty() && wins.is_empty() {
                    continue;
                }
                shown.push((a, r, wins));
            }
            // Which of a service's accounts a column is, when the card shows more than one: the email's name part on
            // a line of its own under the service name (fitted to the column, never widening it).
            let whos: Vec<Option<String>> = shown
                .iter()
                .map(|(a, _, _)| {
                    let same: Vec<_> = shown.iter().filter(|(b, _, _)| b.svc == a.svc).collect();
                    (same.len() > 1).then(|| {
                        let n = same.iter().position(|(b, _, _)| b.id == a.id).unwrap_or(0) + 1;
                        if a.who.is_empty() { format!("#{n}") } else { short_who(&a.who).to_string() }
                    })
                })
                .collect();
            // Side by side, every column keeps that line (empty where it has none) so the rings stay level.
            let who_row = if !v && whos.iter().any(Option::is_some) { who_style().lh } else { 0.0 };
            if let Some(n) = net_item(f, who_row) {
                items.push(n);
            }
            if shown.is_empty() {
                // An account on with all its windows left off, or one switched off: pick what to show; else nothing
                // logged in: add an account.
                let any_on = data.accts.iter().any(|(a, r)| inp.settings.on(&a.id) && !r.absent);
                let some_off = data.accts.iter().any(|(a, _)| !inp.settings.on(&a.id));
                let (label, tip) = if any_on || some_off { (t.pick_accounts, t.all_off) } else { (t.add_account, t.not_logged_in) };
                let divider = matches!(items.last(), Some(Item::Net { .. }));
                items.push(prompt_item(f, label, tip, v, divider));
            } else {
                // `.svc + .svc` counts the internet column too (it is a `.svc`).
                let mut first = !matches!(items.last(), Some(Item::Net { .. }));
                for ((a, r, wins), who) in shown.into_iter().zip(whos) {
                    let s = a.svc;
                    let x = a.aid();
                    let stale = r.stale();
                    let mut rings = Vec::new();
                    if !r.ok {
                        rings.push((Target::Ring(x, -1), 0.0, "!".to_string(), t.error.to_string()));
                    } else if wins.is_empty() {
                        rings.push((Target::Ring(x, -1), 0.0, "–".to_string(), t.no_data.to_string()));
                    } else {
                        for (i, w) in wins {
                            let left = 100.0 - w.used();
                            rings.push((Target::Ring(x, i as i32), left, js_round(left).to_string(), left_label(w, inp.lang, inp.now)));
                        }
                    }
                    let ns = name_style(svc_color(s));
                    let name_w = f.width(s.name(), &ns);
                    let ls = lbl_style();
                    let mut boxes = Vec::new();
                    let ring_h = 2.0 + RING + 1.0 + ls.lh + 1.0;
                    let mut pos = 0.0f32;
                    for (target, left, text, label) in rings {
                        let w = RING.max(f.width(&label, &ls)) + 4.0;
                        let r = if v { R::new(0.0, pos, w, ring_h) } else { R::new(pos, 0.0, w, ring_h) };
                        pos += if v { ring_h + 3.0 } else { w + 6.0 };
                        boxes.push(RingBox { target, svc: Some(s), r, left, text, label });
                    }
                    let rings_w = if v { boxes.iter().map(|b| b.r.w).fold(0.0, f32::max) } else { pos - 6.0 };
                    let rings_h = if v { pos - 3.0 } else { ring_h };
                    let w = name_w.max(rings_w);
                    let who = who.map(|t| f.ellipsize(&t, &who_style(), w));
                    let who_h = if who.is_some() { who_style().lh } else { who_row };
                    let h = ns.lh + who_h + 2.0 + rings_h;
                    // Rings are centred under the name (align-items: center), relative to the column.
                    let off_x = (w - rings_w) / 2.0;
                    for b in &mut boxes {
                        b.r.x += off_x + if v { (rings_w - b.r.w) / 2.0 } else { 0.0 };
                        b.r.y += ns.lh + who_h + 2.0;
                    }
                    let divider = !first;
                    first = false;
                    // .svc + .svc: a 1 px border and 10 px (horizontal) / 6 px (vertical) padding before it.
                    let extra = match (divider, v) {
                        (false, _) => 0.0,
                        (true, false) => 11.0,
                        (true, true) => 7.0,
                    };
                    let r = if v { R::new(0.0, 0.0, w, h + extra) } else { R::new(0.0, 0.0, w + extra, h) };
                    if divider {
                        for b in &mut boxes {
                            if v {
                                b.r.y += extra;
                            } else {
                                b.r.x += extra;
                            }
                        }
                    }
                    items.push(Item::Svc { r, svc: s, who, name_w, rings: boxes, divider, stale });
                }
            }
            items.push(more_item());
        }
    }

    // ---------- flex placement ----------
    let pad = if v { PAD_V } else { PAD_H };
    let gap = if v { 6.0 } else { 10.0 };
    // Main-axis margins of each item (start, end) and its cross size for the line.
    let margins = |it: &Item| -> (f32, f32) {
        match it {
            Item::More { .. } => {
                if v {
                    (-3.0, -2.0)
                } else {
                    (-4.0, -5.0)
                }
            }
            Item::Empty { after_net: true, .. } if !v => (10.0, 0.0),
            _ => (0.0, 0.0),
        }
    };
    let main = |r: &R| if v { r.h } else { r.w };
    let cross = |r: &R| if v { r.w } else { r.h };
    let item_r = |it: &Item| -> R {
        match it {
            Item::Net { r, .. } | Item::Svc { r, .. } | Item::Empty { r, .. } | Item::Prompt { r, .. } | Item::More { r } => *r,
        }
    };
    // The "⋯" box: 14 px wide (horizontal) or 12 px tall (vertical), stretched across.
    let more_main = if v { 12.0 } else { 14.0 };
    let mut cross_max: f32 = 0.0;
    for it in &items {
        match it {
            Item::More { .. } => {
                // Its margin box across: svg 16 tall − 3 px of margins (horizontal) / 4 wide + 4 (vertical).
                cross_max = cross_max.max(if v { 4.0 - 4.0 } else { 16.0 - 3.0 });
            }
            Item::Empty { after_net: true, r, .. } if v => cross_max = cross_max.max(r.w + 10.0),
            _ => cross_max = cross_max.max(cross(&item_r(it))),
        }
    }
    let mut pos = 0.0f32;
    let n = items.len();
    for (i, it) in items.iter_mut().enumerate() {
        let (m0, m1) = margins(it);
        pos += m0;
        let len = match it {
            Item::More { .. } => more_main,
            _ => main(&item_r(it)),
        };
        let (ox, oy) = if v { (pad[3] + 1.0, pad[0] + 1.0 + pos) } else { (pad[3] + 1.0 + pos, pad[0] + 1.0) };
        match it {
            Item::More { r } => {
                *r = if v {
                    R::new(ox - 2.0, oy, cross_max + 4.0, more_main)
                } else {
                    R::new(ox, oy - 2.0, more_main, cross_max + 3.0)
                };
            }
            Item::Svc { r, rings, divider, name_w: _, .. } => {
                let old = *r;
                // Stretched across, contents centred (align-items: center inside the column).
                let (nw, nh) = if v { (cross_max, old.h) } else { (old.w, cross_max) };
                let shift = if v { (cross_max - old.w) / 2.0 } else { 0.0 };
                for b in rings.iter_mut() {
                    b.r.x += ox + shift;
                    b.r.y += oy;
                }
                *r = R::new(ox, oy, nw, nh);
                let _ = divider;
            }
            Item::Net { r, .. } | Item::Prompt { r, .. } => {
                let old = *r;
                *r = if v { R::new(ox, oy, cross_max, old.h) } else { R::new(ox, oy, old.w, cross_max) };
            }
            Item::Empty { r, after_net, .. } => {
                let old = *r;
                let ml = if *after_net && v { 10.0 } else { 0.0 };
                *r = if v { R::new(ox + ml, oy, old.w, old.h) } else { R::new(ox, oy, old.w, cross_max) };
            }
        }
        pos += len + m1;
        if i + 1 < n {
            pos += gap;
        }
    }
    let (w, h) = if v {
        (pad[1] + pad[3] + 2.0 + cross_max, pad[0] + pad[2] + 2.0 + pos)
    } else {
        (pad[1] + pad[3] + 2.0 + pos, pad[0] + pad[2] + 2.0 + cross_max)
    };
    CardLayout { w, h, items, vertical: v }
}

/// The ring-sized "add account" / "pick accounts" column: the ring and its label. After the internet column it
/// gets the divider a second service would, and beside it (horizontal) a name-high gap so the rings line up.
fn prompt_item(f: &mut Fonts, label: &str, tip: &'static str, vertical: bool, divider: bool) -> Item {
    let ls = lbl_style();
    let top = if divider && !vertical { name_style(MUTED).lh + 2.0 } else { 0.0 };
    let w = RING.max(f.width(label, &ls)) + 4.0;
    let h = top + 2.0 + RING + 1.0 + ls.lh + 1.0;
    let r = match (divider, vertical) {
        (false, _) => R::new(0.0, 0.0, w, h),
        (true, false) => R::new(0.0, 0.0, w + 11.0, h),
        (true, true) => R::new(0.0, 0.0, w, h + 7.0),
    };
    Item::Prompt { r, label: label.to_string(), tip, divider, top }
}

fn empty_item(f: &mut Fonts, text: &str, vertical: bool, after_net: bool) -> Item {
    let st = empty_style();
    let (w, h) = if vertical {
        let l = f.lay(text, &st, Some(70.0));
        (70.0 + 8.0, l.h + 4.0)
    } else {
        (f.width(text, &st) + 8.0, st.lh + 4.0)
    };
    // `.net + .empty`: border and 10 px padding on the left in place of 4.
    let w = if after_net { w - 4.0 + 11.0 } else { w };
    Item::Empty { r: R::new(0.0, 0.0, w, h), text: text.to_string(), after_net }
}

/// Paints the card with its top-left border corner at (0, 0).
pub fn draw(c: &mut Canvas, f: &mut Fonts, l: &CardLayout, inp: &CardIn, fx: &CardFx) {
    let card = R::new(0.0, 0.0, l.w, l.h);
    let offline = inp.network_on && inp.net.is_some_and(|n| !n.online);
    c.fill_rr(card.inset(0.5, 0.5, 0.5, 0.5), 11.5, Rgba::rgba(17, 19, 24, 0.86));
    // Only the pointer lights the border up: a check under way changes nothing here (it used to whiten it, which
    // looked like a hover coming and going by itself once a minute).
    let border = if offline { BAD } else { LINE.lerp(Rgba::rgba(255, 255, 255, 0.2), fx.card_hover) };
    c.border(card, 12.0, 1.0, border);
    let lang = inp.lang;
    let t = i18n::t(lang);

    for it in &l.items {
        match it {
            Item::Net { r, online, name_w, lbl, gap } => {
                let mut ns = name_style(if *online { OK } else { BAD });
                let mut ls = lbl_style();
                if !*online {
                    ns = ns.weight(700);
                    ls = ls.weight(700).color(BAD);
                }
                f.draw(c, r.cx() - name_w / 2.0, r.y, t.net, &ns, None, 0.0);
                let dot_cx = r.cx();
                let dot_cy = r.y + ns.lh + gap + 2.0 + 2.0 + RING / 2.0;
                let rad = RING / 2.0;
                if *online {
                    c.glow_circle(dot_cx, dot_cy, rad, 0.0, 10.0, 0.0, OK.pct(0.55));
                    c.fill_circle(dot_cx, dot_cy, rad, OK);
                    icons::WIFI.draw(c, dot_cx - 10.0, dot_cy - 10.0, 20.0, 20.0, Rgba::hex(0x0b1a10));
                } else {
                    // @keyframes alarm (1 s, ease-in-out): the green glow → a 4 px red ring + 16 px red glow, 75 % opacity.
                    let ph = (fx.alarm_s % 1.0) * 2.0;
                    let k = super::ease_in_out(if ph <= 1.0 { ph } else { 2.0 - ph });
                    c.layer(1.0 - 0.25 * k, Filter::NONE, None, 0.0, |c| {
                        c.glow_circle(dot_cx, dot_cy, rad, 0.0, 10.0 + 6.0 * k, 0.0, OK.pct(0.55).lerp(BAD, k));
                        if k > 0.0 {
                            c.glow_circle(dot_cx, dot_cy, rad, 0.0, 0.0, 4.0 * k, BAD.pct(0.35 * k));
                        }
                        c.fill_circle(dot_cx, dot_cy, rad, BAD);
                        icons::WIFI_OFF.draw(c, dot_cx - 10.0, dot_cy - 10.0, 20.0, 20.0, Rgba::WHITE);
                    });
                }
                let lw = f.width(lbl, &ls);
                f.draw(c, r.cx() - lw / 2.0, dot_cy + rad + 1.0, lbl, &ls, None, 0.0);
            }
            Item::Svc { r, svc, who, name_w, rings, divider, stale } => {
                if *divider {
                    if l.vertical {
                        c.fill_rr(R::new(r.x, r.y, r.w, 1.0), 0.0, LINE);
                    } else {
                        c.fill_rr(R::new(r.x, r.y, 1.0, r.h), 0.0, LINE);
                    }
                }
                let ns = name_style(svc_color(*svc));
                // The column's middle, and the name's top.
                let (mid, ny) = if l.vertical {
                    (r.cx(), r.y + if *divider { 7.0 } else { 0.0 })
                } else {
                    let inner_x = r.x + if *divider { 11.0 } else { 0.0 };
                    let inner_w = r.w - if *divider { 11.0 } else { 0.0 };
                    (inner_x + inner_w / 2.0, r.y)
                };
                let nx = mid - name_w / 2.0;
                f.draw(c, nx, ny, svc.name(), &ns, None, 0.0);
                if let Some(t) = who {
                    let ws = who_style();
                    let ww = f.width(t, &ws);
                    f.draw(c, mid - ww / 2.0, ny + ns.lh, t, &ws, None, 0.0);
                }
                if *stale {
                    c.fill_circle(nx + name_w + 3.5, ny + ns.lh / 2.0, 2.0, WARN);
                }
                for rb in rings {
                    draw_ring(c, f, rb, inp, fx);
                }
            }
            Item::Prompt { r, label, divider, top, .. } => {
                // The divider and the content box after it, as for a second service.
                let inner = if !*divider {
                    *r
                } else if l.vertical {
                    c.fill_rr(R::new(r.x, r.y, r.w, 1.0), 0.0, LINE);
                    r.inset(7.0, 0.0, 0.0, 0.0)
                } else {
                    c.fill_rr(R::new(r.x, r.y, 1.0, r.h), 0.0, LINE);
                    r.inset(0.0, 0.0, 0.0, 11.0)
                };
                let hov = fx.amt(Target::Prompt);
                let ls = lbl_style();
                let bw = RING.max(f.width(label, &ls)) + 4.0;
                let b = R::new(inner.cx() - bw / 2.0, inner.y + top, bw, 2.0 + RING + 1.0 + ls.lh + 1.0);
                if hov > 0.0 {
                    c.fill_rr(b, 9.0, Rgba::rgba(255, 255, 255, 0.07 * hov));
                }
                let (cx, cy) = (b.cx(), b.y + 2.0 + RING / 2.0);
                c.ring(cx, cy, RING / 2.0 - 2.0 - 1.0, 4.0, TRACK, 0.0, Rgba::TRANSPARENT);
                let col = MUTED.lerp(TEXT, hov);
                c.fill_rr(R::new(cx - 6.0, cy - 1.0, 12.0, 2.0), 1.0, col);
                c.fill_rr(R::new(cx - 1.0, cy - 6.0, 2.0, 12.0), 1.0, col);
                let lw = f.width(label, &ls);
                f.draw(c, cx - lw / 2.0, b.y + 2.0 + RING + 1.0, label, &ls.color(col), None, 0.0);
            }
            Item::Empty { r, text, after_net } => {
                let st = empty_style();
                if *after_net {
                    c.fill_rr(R::new(r.x, r.y, 1.0, r.h), 0.0, LINE);
                }
                let x = r.x + if *after_net { 11.0 } else { 4.0 };
                if l.vertical {
                    f.draw(c, x, r.y + 2.0, text, &st, Some(70.0), 0.5);
                } else {
                    f.draw(c, x, r.y + 2.0, text, &st, None, 0.0);
                }
            }
            Item::More { r } => {
                let hov = fx.amt(Target::More);
                if hov > 0.0 {
                    c.fill_rr(*r, 7.0, Rgba::rgba(255, 255, 255, 0.09 * hov));
                }
                let opacity = 0.45 + 0.55 * fx.card_hover.max(hov);
                let color = MUTED.lerp(TEXT, hov).fade(opacity);
                if l.vertical {
                    icons::MORE.draw_rot(c, r.cx() - 2.0, r.cy() - 8.0, 4.0, 16.0, color, 90.0);
                } else {
                    icons::MORE.draw(c, r.cx() - 2.0, r.cy() - 8.0, 4.0, 16.0, color);
                }
            }
        }
    }
}

fn draw_ring(c: &mut Canvas, f: &mut Fonts, rb: &RingBox, inp: &CardIn, fx: &CardFx) {
    let accent = rb.svc.map(svc_color).unwrap_or(TEXT);
    let sel = matches!(rb.target, Target::Ring(s, i) if inp.selected == Some((s, i)));
    let hov = fx.amt(rb.target);
    if sel {
        c.fill_rr(rb.r, 9.0, accent.pct(0.2));
        c.border(rb.r, 9.0, 1.0, accent.pct(0.55));
    } else if hov > 0.0 {
        c.fill_rr(rb.r, 9.0, Rgba::rgba(255, 255, 255, 0.07 * hov));
    }
    let cx = rb.r.cx();
    let cy = rb.r.y + 2.0 + RING / 2.0;
    let color = level_color(rb.left);
    c.ring(cx, cy, RING / 2.0 - 2.0 - 1.0, 4.0, TRACK, (super::clamp_pct(rb.left) / 100.0) as f32, if rb.left > 0.0 { color } else { Rgba::TRANSPARENT });
    let ts = ring_text_style();
    central_text(c, f, cx, cy, &rb.text, &ts);
    let ls = lbl_style();
    let lw = f.width(&rb.label, &ls);
    f.draw(c, cx - lw / 2.0, rb.r.y + 2.0 + RING + 1.0, &rb.label, &ls, None, 0.0);
}

/// SVG text with text-anchor middle and dominant-baseline central at (cx, cy).
pub fn central_text(c: &mut Canvas, f: &mut Fonts, cx: f32, cy: f32, text: &str, st: &Style) {
    let (a, d) = f.ascent_descent(st);
    let w = f.width(text, st);
    f.draw_at_baseline(c, cx - w / 2.0, cy + (a - d) / 2.0, text, st, 0.0);
}
