//! `--screenshots <dir>`: renders every state from demo data straight to PNG at 2.4× (app scale 1.2 × 2), the way
//! scripts/screenshots.js composed the README images: the card with its panel or the update toast above it
//! (horizontal) or beside it (vertical), sharing the edge that faces the bottom-right corner of a screen.
use crate::i18n::Lang;
use crate::model::{aid, Acct, NetState, QErr, Quota, Snapshot, Svc, Win};
use crate::settings::Settings;
use crate::ui::card::{self, CardFx, CardIn};
use crate::ui::gfx::Canvas;
use crate::ui::panel::{self, PanelFx, PanelIn};
use crate::ui::text::Fonts;
use crate::ui::toast::{self, Kind, ToastFx, ToastIn};
use std::collections::BTreeMap;
use std::path::Path;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// The demo accounts; WORK is the second Claude account of the "accounts" shots.
const CLAUDE: &str = "claude-3f9a2c";
const CODEX: &str = "codex-81d0e4";
const CURSOR: &str = "cursor-5b27aa";
const WORK: &str = "claude-7e41b0";

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
    let acct = |id: &str, svc: Svc| Acct { id: id.into(), svc, who: "me@example.com".into() };
    Snapshot {
        accts: vec![(acct(CLAUDE, Svc::Claude), claude), (acct(CODEX, Svc::Codex), codex), (acct(CURSOR, Svc::Cursor), cursor)],
        fetched_at: now,
    }
}

/// The demo plus a second Claude account signed in here.
pub fn demo_two(now: i64) -> Snapshot {
    let mut d = demo(now);
    let mut work = d.accts[0].1.clone();
    work.plan = "pro".into();
    for (w, used) in work.windows.iter_mut().zip([81.0, 64.0, 30.0]) {
        w.used_percent = Some(used);
    }
    d.accts.insert(1, (Acct { id: WORK.into(), svc: Svc::Claude, who: "work@example.com".into() }, work));
    let premium = Win { id: "premium_interactions".into(), name: Some("Premium".into()), used_percent: Some(37.0), resets_at: Some(now + 3 * D), ..Win::default() };
    d.accts.push((Acct { id: "copilot-2c9e11".into(), svc: Svc::Copilot, who: "octo-me".into() }, Quota { ok: true, plan: "pro".into(), windows: vec![premium], ..Quota::default() }));
    d
}

/// The Opus and Codex 5 h windows switched off, as one would in Settings.
pub fn demo_hidden() -> BTreeMap<String, Vec<String>> {
    [(CLAUDE.into(), vec!["seven_day_opus".into()]), (CODEX.into(), vec!["primary".into()]), (WORK.into(), vec!["seven_day_opus".into()])].into()
}

enum Pop {
    None,
    /// The ring of an account.
    Panel(&'static str, i32),
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
    let settings = Settings { hidden: demo_hidden(), ..Settings::default() };
    let extra = std::env::var_os("AQO_SHOT_EXTRA").is_some();
    // The README's images ("accounts…" ones show two Claude accounts and Copilot), then with AQO_SHOT_EXTRA=1 the
    // other states, to look at.
    let mut shots: Vec<(&str, Lang, bool, bool, Option<NetState>, Pop, Option<Quota>)> = vec![
        ("hero", Lang::En, false, false, None, Pop::Panel(WORK, 0), None),
        ("vertical", Lang::Th, true, false, None, Pop::Panel(CODEX, 1), None),
        ("offline", Lang::En, false, true, Some(NetState { online: false, ms: None }), Pop::None, None),
        (
            "error",
            Lang::Th,
            false,
            false,
            None,
            Pop::Panel(CLAUDE, -1),
            Some(Quota::err(QErr { code: "appExpired".into(), service: Some("Claude".into()), status: Some("400".into()), ..QErr::default() })),
        ),
        ("empty", Lang::Th, false, false, None, Pop::None, None),
        ("update", Lang::En, false, false, None, Pop::Toast(Kind::Available, 0), None),
    ];
    if extra {
        shots.extend([
            ("horizontal", Lang::En, false, false, None, Pop::None, None),
            ("credits", Lang::En, false, false, None, Pop::Panel(CODEX, 1), None),
            ("network", Lang::En, false, true, Some(NetState { online: true, ms: Some(23) }), Pop::None, None),
            ("thai", Lang::Th, false, false, None, Pop::Panel(CLAUDE, 1), None),
            ("accounts-vertical", Lang::Th, true, false, None, Pop::None, None),
            ("empty-network", Lang::En, false, true, Some(NetState { online: true, ms: Some(7) }), Pop::None, None),
            ("empty-vertical", Lang::Th, true, true, Some(NetState { online: true, ms: Some(7) }), Pop::None, None),
            ("update-progress", Lang::En, false, false, None, Pop::Toast(Kind::Downloading, 64), None),
            ("update-installing", Lang::En, false, false, None, Pop::Toast(Kind::Installing, 100), None),
            ("update-error", Lang::En, false, false, None, Pop::Toast(Kind::Error, 0), None),
        ]);
    }
    // Also the "too many requests" states.
    if extra {
        let limited = |manual: bool| QErr { retry_at: (!manual).then_some(now + 40 * 60_000), ..QErr::rate_limited(None) };
        let retry = |manual: bool| Some(crate::model::Retry { at: now + if manual { 4 } else { 40 } * 60_000, manual });
        let bare = |manual: bool| Quota { retry: retry(manual), ..Quota::err(limited(manual)) };
        let stale = Quota { error: Some(limited(true)), as_of: Some(now - 25 * 60_000), retry: retry(true), ..demo(built).accts[0].1.clone() };
        for (lang, [a, b, c]) in [(Lang::En, ["limited-en", "limited-wait-en", "stale-en"]), (Lang::Th, ["limited-th", "limited-wait-th", "stale-th"])] {
            shots.push((a, lang, false, false, None, Pop::Panel(CLAUDE, -1), Some(bare(true))));
            shots.push((b, lang, false, false, None, Pop::Panel(CLAUDE, -1), Some(bare(false))));
            shots.push((c, lang, false, false, None, Pop::Panel(CLAUDE, 0), Some(stale.clone())));
        }
    }
    for (name, lang, vertical, network, net, pop, claude_override) in shots {
        let mut data = if name == "hero" || name.starts_with("accounts") { demo_two(built) } else { demo(built) };
        if name.starts_with("empty") {
            data.accts.clear();
        }
        if let Some(q) = claude_override {
            data.accts[0].1 = q;
        }
        let selected = match pop {
            Pop::Panel(id, i) => Some((aid(id), i)),
            _ => None,
        };
        let cin = CardIn { lang, vertical, network_on: network, settings: &settings, data: Some(&data), net, selected, now };
        let cl = card::layout(&mut fonts, &cin);
        let card_pm = render(&mut fonts, cl.w, cl.h, |c, f| card::draw(c, f, &cl, &cin, &CardFx::default()));
        let pop_pm = match pop {
            Pop::None => None,
            Pop::Panel(id, i) => {
                let (a, q) = data.by_aid(aid(id)).cloned().unwrap_or_default();
                let who = (data.count(a.svc) > 1).then_some(a.who.as_str());
                let pin = PanelIn { lang, svc: a.svc, who, q: &q, index: i, now };
                let pl = panel::layout(&mut fonts, &pin);
                Some(render(&mut fonts, panel::W, pl.h, |c, f| panel::draw(c, f, &pl, &pin, &PanelFx::default())))
            }
            Pop::Toast(kind, pct) => {
                let tin = ToastIn {
                    lang,
                    kind,
                    current: env!("CARGO_PKG_VERSION"),
                    version: "2.2.0",
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
    app_window(dir, &mut fonts, now)
}

/// The whole app window at its fixed size (settings-en.png / settings-th.png).
pub fn app_window(dir: &Path, fonts: &mut Fonts, now: i64) -> std::io::Result<()> {
    use crate::ui::app_window::{AppWin, Ctx};
    use crate::updater::{Phase, View};
    let zoom: f32 = std::env::var("AQO_SETTINGS_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    // Claude waiting out "too many requests", its last numbers kept.
    let mut waiting = demo(now - 2000);
    waiting.accts[0].1 = Quota {
        error: Some(QErr::rate_limited(None)),
        as_of: Some(now - 3 * 60_000),
        retry: Some(crate::model::Retry { at: now + 4 * 60_000, manual: true }),
        ..waiting.accts[0].1.clone()
    };
    // A login that ran out.
    let mut expired = demo(now - 2000);
    expired.accts[2].1 = Quota::err(QErr { code: "appExpired".into(), service: Some("Cursor".into()), ..QErr::default() });
    let mut shots = vec![("settings-en", Lang::En, demo(now - 2000), None), ("settings-th", Lang::Th, demo(now - 2000), None)];
    if std::env::var_os("AQO_SHOT_EXTRA").is_some() {
        shots.extend([
            // Signing in to another Claude account, with one already signed in here.
            ("settings-accounts", Lang::En, demo_two(now - 2000), Some(Svc::Claude)),
            ("settings-empty", Lang::Th, Snapshot { accts: Vec::new(), fetched_at: now - 2000 }, None),
            ("settings-expired", Lang::Th, expired, None),
            ("settings-waiting", Lang::Th, waiting, None),
            // GitHub's device sign-in, its code on screen.
            ("settings-copilot", Lang::En, demo(now - 2000), Some(Svc::Copilot)),
        ]);
    }
    for (name, lang, data, login) in shots {
        let mut settings = crate::settings::Settings { network: true, lang: lang.key().into(), ..Default::default() };
        settings.hidden = demo_hidden();
        for (a, q) in &data.accts {
            if !q.windows.is_empty() {
                settings.seen.insert(a.id.clone(), q.windows.iter().map(|w| w.seen()).collect());
            }
        }
        let ctx = Ctx {
            lang,
            settings,
            visible: true,
            startup: Some(true),
            startup_failed: false,
            data: Some(data.clone()),
            net: Some(NetState { online: true, ms: Some(23) }),
            update: View {
                phase: Phase::Available,
                current: env!("CARGO_PKG_VERSION").into(),
                version: Some("2.2.0".into()),
                percent: 0,
                manual: false,
                error: None,
                skipped: false,
                dismissed: false,
            },
            version: env!("CARGO_PKG_VERSION").into(),
            login,
            login_code: (login == Some(Svc::Copilot)).then(|| "B7C2-9F4D".to_string()),
            note: None,
        };
        let (w, h) = (crate::ui::app_window::SIZE.0 as f32, crate::ui::app_window::SIZE.1 as f32);
        let mut aw = AppWin::new();
        let mut pm = Pixmap::new((w * zoom).round() as u32, (h * zoom).round() as u32).expect("pixmap");
        {
            let mut c = Canvas::new(&mut pm, zoom, 0.0, 0.0);
            aw.draw(&mut c, fonts, &ctx, w, h);
        }
        if aw.content_height() > h - 46.0 + 0.5 {
            println!("{name}: content is {} px tall, {} px fit", aw.content_height(), h - 46.0);
            // All of it, as scrolling through would show it.
            let fh = aw.content_height() + 46.0;
            let mut full = Pixmap::new((w * zoom).round() as u32, (fh * zoom).round() as u32).expect("pixmap");
            {
                let mut c = Canvas::new(&mut full, zoom, 0.0, 0.0);
                AppWin::new().draw(&mut c, fonts, &ctx, w, fh);
            }
            full.save_png(dir.join(format!("{name}-full.png"))).map_err(std::io::Error::other)?;
        }
        pm.save_png(dir.join(format!("{name}.png"))).map_err(std::io::Error::other)?;
        println!("wrote {}", dir.join(format!("{name}.png")).display());
        // AQO_SHOT_EXTRA=1: also a drag and drop and a sliding pill, frames and timings (not in the README).
        if name == "settings-en" && std::env::var_os("AQO_SHOT_EXTRA").is_some() {
            drag_shot(dir, fonts, &ctx, &mut aw, zoom)?;
        }
    }
    Ok(())
}

/// settings-drag.png: the first account's card held and dragged down over the next one (frames of 16 ms, so the
/// other card slides as it would), and the place it would be dropped at.
fn drag_shot(dir: &Path, fonts: &mut Fonts, ctx: &crate::ui::app_window::Ctx, aw: &mut crate::ui::app_window::AppWin, zoom: f32) -> std::io::Result<()> {
    use crate::app::{Btn3, Mouse};
    use crate::ui::app_window::Action;
    let (w, h) = (crate::ui::app_window::SIZE.0 as f32, crate::ui::app_window::SIZE.1 as f32);
    let Some(r) = aw.card_box(CLAUDE) else { return Ok(()) };
    let (x, y0) = (r.x + 40.0, r.y + 12.0);
    // Frames as the app paints them while cards move (only the changed part, from pictures).
    let frame = |aw: &mut crate::ui::app_window::AppWin, fonts: &mut Fonts| {
        let mut pm = Pixmap::new((w * zoom).round() as u32, (h * zoom).round() as u32).expect("pixmap");
        aw.step(0.016);
        let mut c = Canvas::new(&mut pm, zoom, 0.0, 0.0);
        aw.draw_part(&mut c, fonts, ctx, w, h);
        drop(c);
        pm
    };
    aw.mouse(fonts, ctx, Mouse::Down(Btn3::Left), x, y0);
    frame(aw, fonts);
    let t = std::time::Instant::now();
    aw.mouse(fonts, ctx, Mouse::Move, x, y0 + 12.0);
    let mut pm = frame(aw, fonts);
    let first = t.elapsed();
    let t = std::time::Instant::now();
    for step in 2..=12 {
        aw.mouse(fonts, ctx, Mouse::Move, x, y0 + step as f32 * 12.0);
        pm = frame(aw, fonts);
    }
    println!(
        "drag: first frame (takes the pictures) {:.1} ms, then {:.1} ms a frame (whole window)",
        first.as_secs_f64() * 1000.0,
        t.elapsed().as_secs_f64() * 1000.0 / 11.0
    );
    pm.save_png(dir.join("settings-drag.png")).map_err(std::io::Error::other)?;
    println!("wrote {}", dir.join("settings-drag.png").display());
    for a in aw.mouse(fonts, ctx, Mouse::Up(Btn3::Left), x, y0 + 144.0) {
        if let Action::MoveTo(id, to) = a {
            println!("drop: {id} → place {to}");
        }
    }

    // A segmented control's pill sliding (Horizontal → Vertical), painted as the app does: only the changed box.
    let vctx = crate::ui::app_window::Ctx { settings: crate::settings::Settings { vertical: true, ..ctx.settings.clone() }, ..ctx.clone() };
    let mut aw = crate::ui::app_window::AppWin::new();
    let mut full = Pixmap::new((w * zoom).round() as u32, (h * zoom).round() as u32).expect("pixmap");
    aw.draw(&mut Canvas::new(&mut full, zoom, 0.0, 0.0), fonts, ctx, w, h);
    // Clicking "Vertical" repaints everything once (the setting changed); then the pill slides.
    aw.draw(&mut Canvas::new(&mut full, zoom, 0.0, 0.0), fonts, &vctx, w, h);
    let t = std::time::Instant::now();
    let mut frames = 0;
    while frames < 60 {
        aw.step(0.016);
        let Some(d) = aw.damage.take() else { break };
        let mut part = Pixmap::new(((d.w + 8.0) * zoom).ceil() as u32, ((d.h + 8.0) * zoom).ceil() as u32).expect("pixmap");
        let mut c = Canvas::new(&mut part, zoom, -d.x + 4.0, -d.y + 4.0);
        aw.draw_part(&mut c, fonts, &vctx, w, h);
        drop(c);
        if [0, 2, 4, 8, 20].contains(&frames) {
            part.save_png(dir.join(format!("pill-{frames:02}.png"))).map_err(std::io::Error::other)?;
        }
        frames += 1;
    }
    println!("pill: {frames} frames to settle, {:.1} ms a frame", t.elapsed().as_secs_f64() * 1000.0 / frames.max(1) as f64);
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
