//! Text with cosmic-text (rustybuzz shaping, so Thai marks stack right) rasterised by swash into the pixmap.
//! Lines are placed like CSS line boxes: each line is `line_height` tall and the baseline sits half the leading
//! below the line top plus the primary font's ascent, whatever fallback font a glyph comes from.
use super::gfx::{Canvas, Rgba};
use cosmic_text::{
    fontdb, Attrs, Buffer, CacheKey, Family, FontFeatures, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
    Weight, Wrap,
};
use std::collections::HashMap;
use tiny_skia::{IntSize, Mask, Paint, Pixmap, Transform};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fam {
    Ui,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub size: f32,
    pub weight: u16,
    pub color: Rgba,
    /// Line box height in px (CSS line-height × size).
    pub lh: f32,
    /// Extra advance after each character, px.
    pub ls: f32,
    pub fam: Fam,
    pub tabular: bool,
}

impl Style {
    pub fn new(size: f32, weight: u16, color: Rgba, line_height: f32) -> Self {
        Self { size, weight, color, lh: size * line_height, ls: 0.0, fam: Fam::Ui, tabular: false }
    }
    pub fn ls_em(mut self, em: f32) -> Self {
        self.ls = em * self.size;
        self
    }
    pub fn lh_px(mut self, px: f32) -> Self {
        self.lh = px;
        self
    }
    pub fn tab(mut self) -> Self {
        self.tabular = true;
        self
    }
    pub fn mono(mut self) -> Self {
        self.fam = Fam::Mono;
        self
    }
    pub fn color(mut self, c: Rgba) -> Self {
        self.color = c;
        self
    }
    pub fn weight(mut self, w: u16) -> Self {
        self.weight = w;
        self
    }
}

pub struct Fonts {
    fs: FontSystem,
    cache: SwashCache,
    ui_family: String,
    mono_family: String,
    /// Ascent and descent per em of each family's primary face.
    ui_metrics: (f32, f32),
    mono_metrics: (f32, f32),
    shaped: HashMap<String, Buffer>,
}

#[derive(Clone, Debug, Default)]
pub struct Laid {
    pub lines: Vec<f32>,
    pub w: f32,
    pub h: f32,
}

impl Fonts {
    pub fn load() -> Self {
        let mut db = fontdb::Database::new();
        let (ui, mono) = load_platform_fonts(&mut db);
        let locale = "en-US".to_string();
        let mut fs = FontSystem::new_with_locale_and_db(locale, db);
        let ui_metrics = family_metrics(&mut fs, &ui);
        let mono_metrics = family_metrics(&mut fs, &mono);
        Self { fs, cache: SwashCache::new(), ui_family: ui, mono_family: mono, ui_metrics, mono_metrics, shaped: HashMap::new() }
    }

    fn key(text: &str, st: &Style, max_w: Option<f32>) -> String {
        format!("{}|{}|{}|{}|{:?}|{}|{:?}", st.size, st.weight, st.ls, st.tabular, st.fam, text, max_w.map(|w| (w * 100.0) as i32))
    }

    fn buffer(&mut self, text: &str, st: &Style, max_w: Option<f32>) -> &Buffer {
        let key = Self::key(text, st, max_w);
        if !self.shaped.contains_key(&key) {
            if self.shaped.len() > 600 {
                self.shaped.clear();
            }
            let family = match st.fam {
                Fam::Ui => self.ui_family.clone(),
                Fam::Mono => self.mono_family.clone(),
            };
            let mut feats = FontFeatures::new();
            if st.tabular {
                feats.enable(cosmic_text::FeatureTag::new(b"tnum"));
            }
            let attrs = Attrs::new()
                .family(Family::Name(&family))
                .weight(Weight(st.weight))
                .letter_spacing(if st.size > 0.0 { st.ls / st.size } else { 0.0 })
                .font_features(feats);
            let mut buf = Buffer::new(&mut self.fs, Metrics::new(st.size, st.lh.max(1.0)));
            buf.set_wrap(&mut self.fs, if max_w.is_some() { Wrap::WordOrGlyph } else { Wrap::None });
            buf.set_size(&mut self.fs, max_w, None);
            buf.set_text(&mut self.fs, text, &attrs, Shaping::Advanced);
            buf.shape_until_scroll(&mut self.fs, false);
            self.shaped.insert(key.clone(), buf);
        }
        &self.shaped[&key]
    }

    /// Width of one line of text (no wrapping).
    pub fn width(&mut self, text: &str, st: &Style) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.buffer(text, st, None).layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
    }

    /// Lines and size of text wrapped at `max_w` (None = one line).
    pub fn lay(&mut self, text: &str, st: &Style, max_w: Option<f32>) -> Laid {
        let lh = st.lh;
        let lines: Vec<f32> = self.buffer(text, st, max_w).layout_runs().map(|r| r.line_w).collect();
        let n = lines.len().max(1);
        Laid { w: lines.iter().cloned().fold(0.0, f32::max), h: n as f32 * lh, lines }
    }

    fn metrics(&self, st: &Style) -> (f32, f32) {
        let (a, d) = match st.fam {
            Fam::Ui => self.ui_metrics,
            Fam::Mono => self.mono_metrics,
        };
        (a * st.size, d * st.size)
    }

    /// Baseline offset from a line box's top.
    pub fn baseline(&self, st: &Style) -> f32 {
        let (a, d) = self.metrics(st);
        (st.lh - (a + d)) / 2.0 + a
    }

    /// Ascent and descent in px for `st` (for SVG `dominant-baseline: central`).
    pub fn ascent_descent(&self, st: &Style) -> (f32, f32) {
        self.metrics(st)
    }

    /// Draws text with its first line box's top-left at (x, y). `align` 0 = left, 0.5 = centre, 1 = right,
    /// relative to `max_w` when wrapping or to x when not. Returns the laid-out size.
    pub fn draw(&mut self, c: &mut Canvas, x: f32, y: f32, text: &str, st: &Style, max_w: Option<f32>, align: f32) -> Laid {
        self.draw_inner(c, x, y, text, st, max_w, align, None)
    }

    /// Like `draw`, with the baseline placed at `baseline` instead of from the line box.
    pub fn draw_at_baseline(&mut self, c: &mut Canvas, x: f32, baseline: f32, text: &str, st: &Style, align: f32) -> Laid {
        self.draw_inner(c, x, 0.0, text, st, None, align, Some(baseline))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_inner(&mut self, c: &mut Canvas, x: f32, y: f32, text: &str, st: &Style, max_w: Option<f32>, align: f32, baseline: Option<f32>) -> Laid {
        if text.is_empty() || st.color.a <= 0.0 {
            return Laid { lines: vec![0.0], w: 0.0, h: st.lh };
        }
        let base_off = self.baseline(st);
        // Text is only ever drawn under scale + translate transforms.
        let (scale, tx, ty) = (c.base.sx, c.base.tx, c.base.ty);
        let lh = st.lh;
        // Collect glyphs first (the buffer borrows self).
        let mut glyphs: Vec<(CacheKey, i32, i32)> = Vec::new();
        let mut lines = Vec::new();
        {
            let buf = self.buffer(text, st, max_w);
            for (i, run) in buf.layout_runs().enumerate() {
                lines.push(run.line_w);
                let ax = match max_w {
                    Some(w) => (w - run.line_w) * align,
                    None => -run.line_w * align,
                };
                let line_base = match baseline {
                    Some(b) => b + i as f32 * lh,
                    None => y + i as f32 * lh + base_off,
                };
                // Glyph x/y are relative to the run's origin and baseline; put that on ours, in device px, with
                // the baseline on a whole pixel (vertical hinting, as browsers do).
                let offset = (tx + scale * (x + ax), (ty + scale * line_base).round());
                for g in run.glyphs.iter() {
                    let pg = g.physical(offset, scale);
                    glyphs.push((pg.cache_key, pg.x, pg.y));
                }
            }
        }
        let color = st.color;
        for (key, gx, gy) in glyphs {
            let Some(img) = self.cache.get_image(&mut self.fs, key).as_ref() else { continue };
            if img.placement.width == 0 || img.placement.height == 0 {
                continue;
            }
            let px = gx + img.placement.left;
            let py = gy - img.placement.top;
            blit_glyph(c.pm, px, py, img.placement.width, img.placement.height, &img.data, img.content, color);
        }
        let n = lines.len().max(1);
        Laid { w: lines.iter().cloned().fold(0.0, f32::max), h: n as f32 * lh, lines }
    }

    /// Text cut with "…" so it fits in `max_w` (CSS text-overflow: ellipsis).
    pub fn ellipsize(&mut self, text: &str, st: &Style, max_w: f32) -> String {
        if self.width(text, st) <= max_w + 0.01 {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let s: String = chars[..mid].iter().collect::<String>() + "…";
            if self.width(&s, st) <= max_w + 0.01 {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        chars[..lo].iter().collect::<String>() + "…"
    }
}

fn blit_glyph(pm: &mut Pixmap, x: i32, y: i32, w: u32, h: u32, data: &[u8], content: SwashContent, color: Rgba) {
    match content {
        SwashContent::Mask => {
            let Some(size) = IntSize::from_wh(w, h) else { return };
            let Some(mask) = Mask::from_vec(data.to_vec(), size) else { return };
            // Fill the glyph box through the coverage mask.
            let Some(mut tmp) = Pixmap::new(w, h) else { return };
            let mut p = Paint::default();
            p.set_color(color.sk());
            tmp.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap(), &p, Transform::identity(), Some(&mask));
            pm.draw_pixmap(x, y, tmp.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
        }
        SwashContent::Color | SwashContent::SubpixelMask => {
            // Colour glyphs (emoji) are not used by the UI; draw their alpha as a mask.
            let alpha: Vec<u8> = data.chunks(4).map(|p| p[3]).collect();
            blit_glyph(pm, x, y, w, h, &alpha, SwashContent::Mask, color);
        }
    }
}

fn family_metrics(fs: &mut FontSystem, family: &str) -> (f32, f32) {
    let query = fontdb::Query { families: &[Family::Name(family)], weight: fontdb::Weight(400), ..Default::default() };
    let id = fs.db().query(&query);
    if let Some(font) = id.and_then(|id| fs.get_font(id)) {
        let m = font.as_swash().metrics(&[]);
        let upm = m.units_per_em as f32;
        if upm > 0.0 {
            return (m.ascent / upm, m.descent.abs() / upm);
        }
    }
    (1.079, 0.251)
}

/// Loads only the fonts the pages ask for ("Segoe UI", "Leelawadee UI", Tahoma; Consolas for keys), so the
/// font database stays a handful of faces. Returns the UI and monospace family names.
fn load_platform_fonts(db: &mut fontdb::Database) -> (String, String) {
    #[cfg(windows)]
    {
        let dir = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
        for f in [
            "segoeui.ttf", "seguisb.ttf", "segoeuib.ttf", "LeelawUI.ttf", "LeelaUIb.ttf", "tahoma.ttf", "tahomabd.ttf",
            "consola.ttf", "consolab.ttf", "seguisym.ttf",
        ] {
            let _ = db.load_font_file(format!("{dir}\\{f}"));
        }
        let ui = if has(db, "Segoe UI") { "Segoe UI" } else { "Tahoma" };
        let mono = if has(db, "Consolas") { "Consolas" } else { ui };
        (ui.into(), mono.into())
    }
    #[cfg(target_os = "macos")]
    {
        // Chromium on macOS resolved the pages' font stack to Tahoma (Segoe UI and Leelawadee UI are absent).
        for f in [
            "/System/Library/Fonts/Supplemental/Tahoma.ttf",
            "/System/Library/Fonts/Supplemental/Tahoma Bold.ttf",
            "/System/Library/Fonts/Supplemental/Ayuthaya.ttf",
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Apple Symbols.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ] {
            let _ = db.load_font_file(f);
        }
        let ui = if has(db, "Tahoma") { "Tahoma" } else { "Helvetica" };
        let mono = if has(db, "Menlo") { "Menlo" } else { ui };
        (ui.into(), mono.into())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        db.load_system_fonts();
        let ui = ["Segoe UI", "Tahoma", "Noto Sans", "DejaVu Sans", "Liberation Sans", "Ubuntu", "Cantarell"]
            .into_iter()
            .find(|f| has(db, f))
            .unwrap_or("sans-serif");
        let mono = ["Consolas", "DejaVu Sans Mono", "Noto Sans Mono", "Liberation Mono"]
            .into_iter()
            .find(|f| has(db, f))
            .unwrap_or(ui);
        (ui.into(), mono.into())
    }
}

#[allow(dead_code)]
fn has(db: &fontdb::Database, family: &str) -> bool {
    db.faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(family)))
}
