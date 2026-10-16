//! `--screenshots <dir>`: renders every state from demo data straight to PNG at 2.4× (app scale 1.2 × 2), the way
//! scripts/screenshots.js composed the README images: the card with its panel or the update toast above it
//! (horizontal) or beside it (vertical), sharing the edge that faces the bottom-right corner of a screen.
use crate::i18n::Lang;
use crate::model::{NetState, QErr, Quota, Snapshot, Svc, Win};
use crate::ui::card::{self, CardFx, CardIn};
use crate::ui::gfx::Canvas;
use crate::ui::panel::{self, PanelFx, PanelIn};
use crate::ui::text::Fonts;
use crate::ui::toast::{self, Kind, ToastFx, ToastIn};
use std::path::Path;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// AQO_SHOT_ZOOM overrides it (5.4 matches the old docs, which were captured on a 150 % display and scaled twice).
fn zoom() -> f32 {
    std::env::var("AQO_SHOT_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(1.2 * 2.0)
}
const PAD: f32 = 2.0;
const GAP: f32 = 4.0;
const MARGIN: f32 = 6.0;
const H: i64 = 3_600_000;
const D: i64 = 24 * H;

pub fn demo(now: i64) -> Snapshot {
    let w = |id: &str, span: Option<f64>, name: Option<&str>, used: f64, at: i64| Win {
        id: id.into(),
        span,
        name: name.map(Into::into),
        used_percent: Some(used),
        resets_at: Some(at),
        ..Win::default()
    };
    let claude = Quota {
        ok: true,
        plan: "max".into(),
        windows: vec![
            w("five_hour", Some(5.0 * 3600.0), None, 38.0, now + (2.7 * H as f64) as i64),
            w("seven_day", Some(168.0 * 3600.0), None, 57.0, now + (4.4 * D as f64) as i64),
            w("seven_day_opus", Some(168.0 * 3600.0), Some("Opus"), 22.0, now + (4.4 * D as f64) as i64),
        ],
        ..Quota::default()
    };
    let codex = Quota {
        ok: true,
        plan: "pro".into(),
        windows: vec![
            w("primary", Some(5.0 * 3600.0), None, 12.0, now + (3.1 * H as f64) as i64),
            w("secondary", Some(168.0 * 3600.0), None, 71.0, now + (3.2 * D as f64) as i64),
        ],
        credits: Some(1972.7),
        ..Quota::default()
    };
    let cursor = Quota {
        ok: true,
        plan: "pro".into(),
        windows: vec![Win { billing: true, ..w("billing", None, None, 86.0, now + (11.3 * D as f64) as i64) }],
        ..Quota::default()
    };
    Snapshot { svc: [Some(claude), Some(codex), Some(cursor)], fetched_at: now }
}

/// The Opus and Codex 5 h windows switched off, as one would in Settings.
pub fn demo_hidden() -> [Vec<String>; 3] {
    [vec!["seven_day_opus".into()], vec!["primary".into()], vec![]]
}

enum Pop {
    None,
    Panel(Svc, i32),
    Toast(Kind, u32),
}

fn render(fonts: &mut Fonts, w: f32, h: f32, f: impl FnOnce(&mut Canvas, &mut Fonts)) -> Pixmap {
    let pw = ((w + 2.0 * PAD) * zoom()).ceil() as u32;
    let ph = ((h + 2.0 * PAD) * zoom()).ceil() as u32;
    let mut pm = Pixmap::new(pw, ph).expect("pixmap");
    {
        let mut c = Canvas::new(&mut pm, zoom(), PAD, PAD);
        f(&mut c, fonts);
    }
    pm
}

pub fn run(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut fonts = Fonts::load();
    // The old script rendered a moment after building its demo data, so times left read a little lower.
    let built = crate::model::now_ms();
    let now = built + 2000;
    let hidden = demo_hidden();
    let shots: Vec<(&str, Lang, bool, bool, Option<NetState>, Pop, Option<Quota>)> = vec![
        ("hero", Lang::En, false, false, None, Pop::Panel(Svc::Claude, 0), None),
        ("horizontal", Lang::En, false, false, None, Pop::None, None),
        ("vertical", Lang::En, true, false, None, Pop::Panel(Svc::Codex, 1), None),
        ("credits", Lang::En, false, false, None, Pop::Panel(Svc::Codex, 1), None),
        ("network", Lang::En, false, true, Some(NetState { online: true, ms: Some(23) }), Pop::None, None),
        ("offline", Lang::En, false, true, Some(NetState { online: false, ms: None }), Pop::None, None),
        ("thai", Lang::Th, false, false, None, Pop::Panel(Svc::Claude, 1), None),
        (
            "error",
            Lang::En,
            false,
            false,
            None,
            Pop::Panel(Svc::Claude, -1),
            Some(Quota::err(QErr { code: "claudeExpired".into(), status: Some("400".into()), ..QErr::default() })),
        ),
        ("update", Lang::En, false, false, None, Pop::Toast(Kind::Available, 0), None),
        ("update-progress", Lang::En, false, false, None, Pop::Toast(Kind::Downloading, 64), None),
        ("update-installing", Lang::En, false, false, None, Pop::Toast(Kind::Installing, 100), None),
        ("update-error", Lang::En, false, false, None, Pop::Toast(Kind::Error, 0), None),
    ];
    for (name, lang, vertical, network, net, pop, claude_override) in shots {
        let mut data = demo(built);
        if let Some(q) = claude_override {
            data.svc[0] = Some(q);
        }
        let selected = match pop {
            Pop::Panel(s, i) => Some((s, i)),
            _ => None,
        };
        let cin = CardIn {
            lang,
            vertical,
            network_on: network,
            services_on: [true; 3],
            hidden: [&hidden[0], &hidden[1], &hidden[2]],
            data: Some(&data),
            net,
            selected,
            now,
        };
        let cl = card::layout(&mut fonts, &cin);
        let card_pm = render(&mut fonts, cl.w, cl.h, |c, f| card::draw(c, f, &cl, &cin, &CardFx::default()));
        let pop_pm = match pop {
            Pop::None => None,
            Pop::Panel(s, i) => {
                let q = data.get(s).cloned().unwrap_or_default();
                let pin = PanelIn { lang, svc: s, q: &q, index: i, now };
                let pl = panel::layout(&mut fonts, &pin);
                Some(render(&mut fonts, panel::W, pl.h, |c, f| panel::draw(c, f, &pl, &pin, &PanelFx::default())))
            }
            Pop::Toast(kind, pct) => {
                let tin = ToastIn {
                    lang,
                    kind,
                    current: env!("CARGO_PKG_VERSION"),
                    version: "2.1.0",
                    percent: pct,
                    manual: false,
                    error: "Couldn't reach github.com",
                };
                let tl = toast::layout(&mut fonts, &tin);
                let fx = ToastFx { bar: pct as f32, ..ToastFx::default() };
                Some(render(&mut fonts, toast::W, tl.h, |c, f| toast::draw(c, f, &tl, &tin, &fx)))
            }
        };
        let img = compose(&card_pm, pop_pm.as_ref(), vertical);
        img.save_png(dir.join(format!("{name}.png"))).map_err(std::io::Error::other)?;
        println!("wrote {}", dir.join(format!("{name}.png")).display());
    }
    Ok(())
}

/// Panel above the card (horizontal) or to its left (vertical), right/bottom edges aligned, MARGIN around.
fn compose(card: &Pixmap, pop: Option<&Pixmap>, vertical: bool) -> Pixmap {
    let m = ((MARGIN - PAD) * zoom()).round() as u32;
    let gap = ((GAP - 2.0 * PAD) * zoom()).round() as u32;
    let (cw, ch) = (card.width(), card.height());
    let (pw, ph) = pop.map(|p| (p.width(), p.height())).unwrap_or((0, 0));
    let (w, h) = match (pop.is_some(), vertical) {
        (false, _) => (cw, ch),
        (true, false) => (cw.max(pw), ch + gap + ph),
        (true, true) => (cw + gap + pw, ch.max(ph)),
    };
    let mut out = Pixmap::new(w + 2 * m, h + 2 * m).expect("pixmap");
    let draw = |out: &mut Pixmap, p: &Pixmap, x: u32, y: u32| {
        out.draw_pixmap(x as i32, y as i32, p.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    };
    match (pop, vertical) {
        (None, _) => draw(&mut out, card, m, m),
        (Some(p), false) => {
            draw(&mut out, p, m + w - pw, m);
            draw(&mut out, card, m + w - cw, m + ph + gap);
        }
        (Some(p), true) => {
            draw(&mut out, p, m, m + h - ph);
            draw(&mut out, card, m + pw + gap, m + h - ch);
        }
    }
    out
}
