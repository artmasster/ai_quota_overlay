//! Drawing in CSS px on a tiny-skia pixmap: colours with CSS `color-mix`, rounded boxes with borders, rings,
//! gradients, blurred shadows and layers (opacity / grayscale / brightness), all through one transform
//! (CSS px → device px).
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LineCap, LinearGradient, Mask, Paint, Path, PathBuilder,
    Pixmap, PixmapPaint, Point, Rect, Shader, SpreadMode, Stroke, Transform,
};

/// Non-premultiplied sRGB colour, 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn hex(v: u32) -> Self {
        Self::rgba(((v >> 16) & 255) as u8, ((v >> 8) & 255) as u8, (v & 255) as u8, 1.0)
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
    }
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const WHITE: Rgba = Rgba::hex(0xffffff);

    /// The colour at `a` × its alpha (CSS `opacity` on a solid colour).
    pub fn fade(self, a: f32) -> Self {
        Self { a: self.a * a, ..self }
    }
    /// `color-mix(in srgb, self p, transparent)`.
    pub fn pct(self, p: f32) -> Self {
        self.fade(p)
    }
    /// `color-mix(in srgb, self p, other)`, interpolated premultiplied like browsers do.
    pub fn mix(self, p: f32, other: Rgba) -> Self {
        let q = 1.0 - p;
        let a = self.a * p + other.a * q;
        if a <= 0.0 {
            return Rgba::TRANSPARENT;
        }
        let c = |x: f32, y: f32| (x * self.a * p + y * other.a * q) / a;
        Self { r: c(self.r, other.r), g: c(self.g, other.g), b: c(self.b, other.b), a }
    }
    /// Linear interpolation (for transitions).
    pub fn lerp(self, other: Rgba, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Self { r: l(self.r, other.r), g: l(self.g, other.g), b: l(self.b, other.b), a: l(self.a, other.a) }
    }
    pub fn sk(self) -> Color {
        Color::from_rgba(self.r.clamp(0.0, 1.0), self.g.clamp(0.0, 1.0), self.b.clamp(0.0, 1.0), self.a.clamp(0.0, 1.0))
            .unwrap_or(Color::TRANSPARENT)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    /// Shrunk by t, r, b, l.
    pub fn inset(&self, t: f32, r: f32, b: f32, l: f32) -> Self {
        Self::new(self.x + l, self.y + t, (self.w - l - r).max(0.0), (self.h - t - b).max(0.0))
    }
    pub fn grow(&self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
}

/// Paint for a fill: a colour or a CSS `linear-gradient(<deg>, stops…)` over the filled box.
#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Rgba),
    Linear { deg: f32, stops: Vec<(f32, Rgba)>, bx: R },
}

impl From<Rgba> for Fill {
    fn from(c: Rgba) -> Self {
        Fill::Solid(c)
    }
}

pub fn linear(deg: f32, bx: R, stops: &[(f32, Rgba)]) -> Fill {
    Fill::Linear { deg, stops: stops.to_vec(), bx }
}

const K: f32 = 0.552_284_8;

/// Rounded rectangle with per-corner radii (tl, tr, br, bl), clamped like CSS.
pub fn rrect(r: R, radii: [f32; 4]) -> Option<Path> {
    if r.w <= 0.0 || r.h <= 0.0 {
        return None;
    }
    let [mut tl, mut tr, mut br, mut bl] = radii.map(|v| v.max(0.0));
    let f = [
        r.w / (tl + tr).max(1e-6),
        r.h / (tr + br).max(1e-6),
        r.w / (br + bl).max(1e-6),
        r.h / (bl + tl).max(1e-6),
        1.0,
    ]
    .into_iter()
    .fold(f32::INFINITY, f32::min);
    if f < 1.0 {
        tl *= f;
        tr *= f;
        br *= f;
        bl *= f;
    }
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + tl, y0);
    pb.line_to(x1 - tr, y0);
    if tr > 0.0 {
        pb.cubic_to(x1 - tr + tr * K, y0, x1, y0 + tr - tr * K, x1, y0 + tr);
    }
    pb.line_to(x1, y1 - br);
    if br > 0.0 {
        pb.cubic_to(x1, y1 - br + br * K, x1 - br + br * K, y1, x1 - br, y1);
    }
    pb.line_to(x0 + bl, y1);
    if bl > 0.0 {
        pb.cubic_to(x0 + bl - bl * K, y1, x0, y1 - bl + bl * K, x0, y1 - bl);
    }
    pb.line_to(x0, y0 + tl);
    if tl > 0.0 {
        pb.cubic_to(x0, y0 + tl - tl * K, x0 + tl - tl * K, y0, x0 + tl, y0);
    }
    pb.close();
    pb.finish()
}

pub fn rr(r: R, radius: f32) -> Option<Path> {
    rrect(r, [radius; 4])
}

/// Circular arc from angle a0 sweeping `sweep` radians (clockwise on screen, 0 = 3 o'clock), as cubics.
pub fn arc(pb: &mut PathBuilder, cx: f32, cy: f32, rad: f32, a0: f32, sweep: f32, move_first: bool) {
    let n = ((sweep.abs() / (std::f32::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let step = sweep / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let p = |a: f32| (cx + rad * a.cos(), cy + rad * a.sin());
    let (sx, sy) = p(a0);
    if move_first {
        pb.move_to(sx, sy);
    } else {
        pb.line_to(sx, sy);
    }
    let mut a = a0;
    for _ in 0..n {
        let b = a + step;
        let (x0, y0) = p(a);
        let (x3, y3) = p(b);
        let (c1x, c1y) = (x0 - k * rad * a.sin(), y0 + k * rad * a.cos());
        let (c2x, c2y) = (x3 + k * rad * b.sin(), y3 - k * rad * b.cos());
        pb.cubic_to(c1x, c1y, c2x, c2y, x3, y3);
        a = b;
    }
}

pub fn circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
    PathBuilder::from_circle(cx, cy, r)
}

/// Canvas: everything below takes CSS px; `base` maps them to device px.
pub struct Canvas<'a> {
    pub pm: &'a mut Pixmap,
    pub base: Transform,
    /// Device px per CSS px (for text rasterisation and blur radii).
    pub scale: f32,
}

impl<'a> Canvas<'a> {
    pub fn new(pm: &'a mut Pixmap, scale: f32, ox: f32, oy: f32) -> Self {
        let base = Transform::from_row(scale, 0.0, 0.0, scale, ox * scale, oy * scale);
        Self { pm, base, scale }
    }

    fn paint(&self, fill: &Fill) -> Paint<'static> {
        let mut p = Paint { anti_alias: true, ..Paint::default() };
        match fill {
            Fill::Solid(c) => p.set_color(c.sk()),
            Fill::Linear { deg, stops, bx } => {
                let (start, end) = gradient_line(*deg, *bx);
                let stops = stops.iter().map(|(o, c)| GradientStop::new(*o, c.sk())).collect::<Vec<_>>();
                p.shader = LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity())
                    .unwrap_or(Shader::SolidColor(Color::TRANSPARENT));
            }
        }
        p
    }

    pub fn fill_path(&mut self, path: &Path, fill: impl Into<Fill>) {
        let p = self.paint(&fill.into());
        self.pm.fill_path(path, &p, FillRule::Winding, self.base, None);
    }

    pub fn fill_path_eo(&mut self, path: &Path, fill: impl Into<Fill>) {
        let p = self.paint(&fill.into());
        self.pm.fill_path(path, &p, FillRule::EvenOdd, self.base, None);
    }

    pub fn fill_rr(&mut self, r: R, radius: f32, fill: impl Into<Fill>) {
        if let Some(p) = rr(r, radius) {
            self.fill_path(&p, fill);
        }
    }

    pub fn fill_rrect(&mut self, r: R, radii: [f32; 4], fill: impl Into<Fill>) {
        if let Some(p) = rrect(r, radii) {
            self.fill_path(&p, fill);
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, rad: f32, fill: impl Into<Fill>) {
        if let Some(p) = circle(cx, cy, rad) {
            self.fill_path(&p, fill);
        }
    }

    /// CSS border: the band between the border box and the padding box (radius − width inside).
    pub fn border(&mut self, r: R, radius: f32, width: f32, color: Rgba) {
        self.border_sides(r, [radius; 4], [width; 4], color);
    }

    /// Border widths per side (t, r, b, l).
    pub fn border_sides(&mut self, r: R, radii: [f32; 4], w: [f32; 4], color: Rgba) {
        if color.a <= 0.0 {
            return;
        }
        let [t, rt, b, l] = w;
        let inner = r.inset(t, rt, b, l);
        let ir = [
            (radii[0] - t.max(l)).max(0.0),
            (radii[1] - t.max(rt)).max(0.0),
            (radii[2] - b.max(rt)).max(0.0),
            (radii[3] - b.max(l)).max(0.0),
        ];
        let mut pb = PathBuilder::new();
        if let Some(o) = rrect(r, radii) {
            pb.push_path(&o);
        }
        if let Some(i) = rrect(inner, ir) {
            pb.push_path(&i);
        }
        if let Some(p) = pb.finish() {
            self.fill_path_eo(&p, color);
        }
    }

    pub fn stroke_path(&mut self, path: &Path, width: f32, color: Rgba, cap: LineCap) {
        let mut p = Paint { anti_alias: true, ..Paint::default() };
        p.set_color(color.sk());
        let s = Stroke { width, line_cap: cap, ..Stroke::default() };
        self.pm.stroke_path(path, &p, &s, self.base, None);
    }

    pub fn stroke_path_with(&mut self, path: &Path, stroke: &Stroke, color: Rgba) {
        let mut p = Paint { anti_alias: true, ..Paint::default() };
        p.set_color(color.sk());
        self.pm.stroke_path(path, &p, stroke, self.base, None);
    }

    /// A quota ring: track circle plus a clockwise arc from 12 o'clock with round caps (SVG dasharray).
    pub fn ring(&mut self, cx: f32, cy: f32, rad: f32, width: f32, track: Rgba, frac: f32, color: Rgba) {
        if let Some(c) = circle(cx, cy, rad) {
            self.stroke_path(&c, width, track, LineCap::Butt);
        }
        let frac = frac.clamp(0.0, 1.0);
        if frac <= 0.0 {
            return;
        }
        let mut pb = PathBuilder::new();
        let sweep = frac * std::f32::consts::TAU;
        arc(&mut pb, cx, cy, rad, -std::f32::consts::FRAC_PI_2, sweep, true);
        if frac >= 1.0 {
            pb.close();
        }
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, width, color, if frac >= 1.0 { LineCap::Butt } else { LineCap::Round });
        }
    }

    /// CSS `box-shadow` (outer): `shape` grown by `spread`, offset, blurred by `blur` px, painted in `color`.
    pub fn shadow(&mut self, r: R, radius: f32, dx: f32, dy: f32, blur: f32, spread: f32, color: Rgba) {
        self.shadow_rrect(r, [radius; 4], dx, dy, blur, spread, color, false);
    }

    /// `inset` shadows are drawn inside the box (clipped to it).
    #[allow(clippy::too_many_arguments)]
    pub fn shadow_rrect(&mut self, r: R, radii: [f32; 4], dx: f32, dy: f32, blur: f32, spread: f32, color: Rgba, inset: bool) {
        if color.a <= 0.0 {
            return;
        }
        let s = self.scale;
        if inset {
            // Inset spread only (no blur is used by the pages): a band of width `spread` inside the box.
            let inner = r.inset(spread + dy, spread - dx, spread - dy, spread + dx);
            let ir = radii.map(|v| (v - spread).max(0.0));
            let mut pb = PathBuilder::new();
            if let Some(o) = rrect(r, radii) {
                pb.push_path(&o);
            }
            if let Some(i) = rrect(inner, ir) {
                pb.push_path(&i);
            }
            if let Some(p) = pb.finish() {
                self.fill_path_eo(&p, color);
            }
            return;
        }
        let grown = r.grow(spread);
        let gr = radii.map(|v| if v > 0.0 { (v + spread).max(0.0) } else { 0.0 });
        let Some(shape) = rrect(R::new(grown.x + dx, grown.y + dy, grown.w, grown.h), gr) else { return };
        // The outer shadow never shows under the box itself (CSS clips it out).
        let hole = rrect(r, radii);
        if blur <= 0.0 {
            let mut pb = PathBuilder::new();
            pb.push_path(&shape);
            if let Some(h) = &hole {
                pb.push_path(h);
            }
            if let Some(p) = pb.finish() {
                self.fill_path_eo(&p, color);
            }
            return;
        }
        // Blurred: draw the shape into a scratch mask covering its bounds plus 3 sigma, blur, composite.
        let sigma = blur / 2.0 * s;
        let pad = (sigma * 3.0).ceil() + 2.0;
        let b = shape.bounds().transform(self.base).unwrap_or(shape.bounds());
        let x0 = (b.left() - pad).floor();
        let y0 = (b.top() - pad).floor();
        let w = ((b.right() + pad).ceil() - x0) as u32;
        let h = ((b.bottom() + pad).ceil() - y0) as u32;
        let Some(mut tmp) = Pixmap::new(w.max(1), h.max(1)) else { return };
        let ts = self.base.post_translate(-x0, -y0);
        let mut p = Paint { anti_alias: true, ..Paint::default() };
        p.set_color(color.sk());
        tmp.fill_path(&shape, &p, FillRule::Winding, ts, None);
        blur_pixmap(&mut tmp, sigma);
        if let Some(h) = &hole {
            // Punch the box out of the shadow.
            let mut cut = Paint { anti_alias: true, blend_mode: BlendMode::DestinationOut, ..Paint::default() };
            cut.set_color(Color::BLACK);
            tmp.fill_path(h, &cut, FillRule::Winding, ts, None);
        }
        self.pm.draw_pixmap(x0 as i32, y0 as i32, tmp.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }

    /// Soft glow of a circle (box-shadow on a round element).
    pub fn glow_circle(&mut self, cx: f32, cy: f32, rad: f32, dy: f32, blur: f32, spread: f32, color: Rgba) {
        self.shadow(R::new(cx - rad, cy - rad, rad * 2.0, rad * 2.0), rad, 0.0, dy, blur, spread, color);
    }

    /// Draws what `f` paints into a separate layer, then composites it with `opacity` and an optional
    /// filter (grayscale amount, brightness) and an extra transform about (ox, oy) in CSS px.
    pub fn layer(&mut self, opacity: f32, filter: Filter, scale_about: Option<(f32, f32, f32)>, dy: f32, f: impl FnOnce(&mut Canvas)) {
        let Some(mut tmp) = Pixmap::new(self.pm.width(), self.pm.height()) else { return };
        {
            let mut c = Canvas { pm: &mut tmp, base: self.base, scale: self.scale };
            f(&mut c);
        }
        apply_filter(&mut tmp, filter);
        let mut ts = Transform::identity();
        if let Some((k, ox, oy)) = scale_about {
            let mut p = Point::from_xy(ox, oy);
            self.base.map_point(&mut p);
            ts = Transform::from_translate(-p.x, -p.y).post_scale(k, k).post_translate(p.x, p.y);
        }
        if dy != 0.0 {
            ts = ts.post_translate(0.0, dy * self.scale);
        }
        let paint = PixmapPaint { opacity: opacity.clamp(0.0, 1.0), quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
        self.pm.draw_pixmap(0, 0, tmp.as_ref(), &paint, ts, None);
    }

    /// Clip drawing of `f` to a rounded rectangle (CSS overflow: hidden).
    pub fn clipped(&mut self, r: R, radius: f32, f: impl FnOnce(&mut Canvas)) {
        let Some(mut tmp) = Pixmap::new(self.pm.width(), self.pm.height()) else { return };
        {
            let mut c = Canvas { pm: &mut tmp, base: self.base, scale: self.scale };
            f(&mut c);
        }
        let Some(path) = rr(r, radius) else { return };
        let Some(mut mask) = Mask::new(self.pm.width(), self.pm.height()) else { return };
        mask.fill_path(&path, FillRule::Winding, true, self.base);
        self.pm.draw_pixmap(0, 0, tmp.as_ref(), &PixmapPaint::default(), Transform::identity(), Some(&mask));
    }

    /// Runs `f` with an extra transform (CSS px) applied on top of the base.
    pub fn with_transform(&mut self, ts: Transform, f: impl FnOnce(&mut Canvas)) {
        let old = self.base;
        self.base = old.pre_concat(ts);
        f(self);
        self.base = old;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Filter {
    pub grayscale: f32,
    pub brightness: f32,
}

impl Filter {
    pub const NONE: Filter = Filter { grayscale: 0.0, brightness: 1.0 };
}

fn apply_filter(pm: &mut Pixmap, f: Filter) {
    let gs = f.grayscale.clamp(0.0, 1.0);
    let br = if f.brightness == 0.0 { 1.0 } else { f.brightness };
    if gs == 0.0 && br == 1.0 {
        return;
    }
    // CSS grayscale(g) matrix (on premultiplied values, which is exact for a linear map with no offset).
    let a = 1.0 - gs;
    let m = [
        [0.2126 + 0.7874 * a, 0.7152 - 0.7152 * a, 0.0722 - 0.0722 * a],
        [0.2126 - 0.2126 * a, 0.7152 + 0.2848 * a, 0.0722 - 0.0722 * a],
        [0.2126 - 0.2126 * a, 0.7152 - 0.7152 * a, 0.0722 + 0.9278 * a],
    ];
    for px in pm.pixels_mut() {
        let al = px.alpha();
        if al == 0 {
            continue;
        }
        let (r, g, b) = (px.red() as f32, px.green() as f32, px.blue() as f32);
        let ch = |row: [f32; 3]| ((row[0] * r + row[1] * g + row[2] * b) * br).round().clamp(0.0, al as f32) as u8;
        let (nr, ng, nb) = (ch(m[0]), ch(m[1]), ch(m[2]));
        *px = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, al).unwrap_or(*px);
    }
}

/// Approximate Gaussian blur (three box passes each way) on a premultiplied pixmap.
pub fn blur_pixmap(pm: &mut Pixmap, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let boxes = box_sizes(sigma, 3);
    let data = pm.data_mut();
    let mut buf = vec![0u8; data.len()];
    for r in boxes {
        box_blur_h(data, &mut buf, w, h, r);
        box_blur_v(&buf, data, w, h, r);
    }
}

fn box_sizes(sigma: f32, n: usize) -> Vec<usize> {
    let w_ideal = (12.0 * sigma * sigma / n as f32 + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - (n as i32 * wl * wl) as f32 - 4.0 * n as f32 * wl as f32 - 3.0 * n as f32)
        / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as i32;
    (0..n as i32).map(|i| (((if i < m { wl } else { wu }) - 1) / 2).max(0) as usize).collect()
}

fn box_blur_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let div = (2 * r + 1) as u32;
    for y in 0..h {
        let row = y * w * 4;
        for c in 0..4 {
            let at = |x: isize| -> u32 {
                if x < 0 || x >= w as isize {
                    0
                } else {
                    src[row + x as usize * 4 + c] as u32
                }
            };
            let mut acc: u32 = (-(r as isize)..=r as isize).map(at).sum();
            for x in 0..w {
                dst[row + x * 4 + c] = ((acc + div / 2) / div) as u8;
                acc += at(x as isize + r as isize + 1);
                acc -= at(x as isize - r as isize);
            }
        }
    }
}

fn box_blur_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let div = (2 * r + 1) as u32;
    for x in 0..w {
        for c in 0..4 {
            let at = |y: isize| -> u32 {
                if y < 0 || y >= h as isize {
                    0
                } else {
                    src[y as usize * w * 4 + x * 4 + c] as u32
                }
            };
            let mut acc: u32 = (-(r as isize)..=r as isize).map(at).sum();
            for y in 0..h {
                dst[y * w * 4 + x * 4 + c] = ((acc + div / 2) / div) as u8;
                acc += at(y as isize + r as isize + 1);
                acc -= at(y as isize - r as isize);
            }
        }
    }
}

/// Start and end of the CSS gradient line for `deg` over box `b` (0deg = to top, 90deg = to right).
fn gradient_line(deg: f32, b: R) -> (Point, Point) {
    let a = deg.to_radians();
    let (sin, cos) = a.sin_cos();
    let len = (b.w * sin).abs() + (b.h * cos).abs();
    let (dx, dy) = (sin * len / 2.0, -cos * len / 2.0);
    (Point::from_xy(b.cx() - dx, b.cy() - dy), Point::from_xy(b.cx() + dx, b.cy() + dy))
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Rect> {
    Rect::from_xywh(x, y, w, h)
}
