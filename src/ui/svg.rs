//! The pages' small inline SVG icons: a path-data parser (M L H V C S Q A Z, absolute and relative) and a
//! drawer for icons made of paths, circles, ellipses and rounded rects, stroked and/or filled in one colour.
use super::gfx::{arc, Canvas, Rgba};
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Stroke, Transform};

pub fn parse_path(d: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let toks = tokenize(d);
    let mut i = 0;
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut last_ctrl: Option<(f32, f32)> = None;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> Option<f32> {
        match toks.get(*i) {
            Some(Tok::Num(n)) => {
                *i += 1;
                Some(*n)
            }
            _ => None,
        }
    };
    let mut open = false;
    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                pb.close();
                cx = sx;
                cy = sy;
                open = false;
                last_ctrl = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let x = num(&mut i)? + ox;
                let y = num(&mut i)? + oy;
                pb.move_to(x, y);
                open = true;
                cx = x;
                cy = y;
                sx = x;
                sy = y;
                // Further pairs after a moveto are linetos.
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let x = num(&mut i)? + ox;
                let y = num(&mut i)? + oy;
                ensure_open(&mut pb, &mut open, cx, cy);
                pb.line_to(x, y);
                cx = x;
                cy = y;
                last_ctrl = None;
            }
            'H' => {
                let x = num(&mut i)? + ox;
                ensure_open(&mut pb, &mut open, cx, cy);
                pb.line_to(x, cy);
                cx = x;
                last_ctrl = None;
            }
            'V' => {
                let y = num(&mut i)? + oy;
                ensure_open(&mut pb, &mut open, cx, cy);
                pb.line_to(cx, y);
                cy = y;
                last_ctrl = None;
            }
            'C' => {
                let (x1, y1) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x2, y2) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                ensure_open(&mut pb, &mut open, cx, cy);
                pb.cubic_to(x1, y1, x2, y2, x, y);
                last_ctrl = Some((x2, y2));
                cx = x;
                cy = y;
            }
            'S' => {
                let (x2, y2) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x1, y1) = match last_ctrl {
                    Some((lx, ly)) => (2.0 * cx - lx, 2.0 * cy - ly),
                    None => (cx, cy),
                };
                ensure_open(&mut pb, &mut open, cx, cy);
                pb.cubic_to(x1, y1, x2, y2, x, y);
                last_ctrl = Some((x2, y2));
                cx = x;
                cy = y;
            }
            'Q' => {
                let (x1, y1) = (num(&mut i)? + ox, num(&mut i)? + oy);
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                ensure_open(&mut pb, &mut open, cx, cy);
                pb.quad_to(x1, y1, x, y);
                last_ctrl = None;
                cx = x;
                cy = y;
            }
            'A' => {
                let rx = num(&mut i)?;
                let ry = num(&mut i)?;
                let rot = num(&mut i)?;
                let large = flag(&toks, &mut i)?;
                let sweep = flag(&toks, &mut i)?;
                let x = num(&mut i)? + ox;
                let y = num(&mut i)? + oy;
                ensure_open(&mut pb, &mut open, cx, cy);
                arc_to(&mut pb, cx, cy, rx, ry, rot, large, sweep, x, y);
                cx = x;
                cy = y;
                last_ctrl = None;
            }
            _ => return None,
        }
    }
    pb.finish()
}

fn ensure_open(pb: &mut PathBuilder, open: &mut bool, x: f32, y: f32) {
    if !*open {
        pb.move_to(x, y);
        *open = true;
    }
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Cmd(char),
    Num(f32),
}

fn tokenize(d: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !dot {
                    dot = true;
                    i += 1;
                } else if (ch == 'e' || ch == 'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            out.push(Tok::Num(d[start..i].parse().unwrap_or(0.0)));
        } else {
            i += 1;
        }
    }
    out
}

/// Arc flags may be written without separators ("0 1 1-6.2"); tokenized numbers like "11" are split here.
fn flag(toks: &[Tok], i: &mut usize) -> Option<bool> {
    match toks.get(*i) {
        Some(Tok::Num(n)) => {
            *i += 1;
            Some(*n != 0.0)
        }
        _ => None,
    }
}

/// SVG endpoint arc → centre parameterisation → cubic segments.
#[allow(clippy::too_many_arguments)]
fn arc_to(pb: &mut PathBuilder, x1: f32, y1: f32, rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, x2: f32, y2: f32) {
    if (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        pb.line_to(x2, y2);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = (num / den).max(0.0).sqrt();
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0;
    let cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let d = (ux * vx + uy * vy) / ((ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt());
        let a = d.clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            -a
        } else {
            a
        }
    };
    let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    if (rx - ry).abs() < 1e-4 && phi == 0.0 {
        arc(pb, cx, cy, rx, t1, dt, false);
        return;
    }
    // Ellipse: segments on the unit circle mapped through scale + rotation.
    let n = ((dt.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize).max(1);
    let step = dt / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let map = |x: f32, y: f32| (cx + cp * rx * x - sp * ry * y, cy + sp * rx * x + cp * ry * y);
    let mut a = t1;
    for _ in 0..n {
        let b = a + step;
        let (ca, sa) = (a.cos(), a.sin());
        let (cb, sb) = (b.cos(), b.sin());
        let c1 = map(ca - k * sa, sa + k * ca);
        let c2 = map(cb + k * sb, sb - k * cb);
        let e = map(cb, sb);
        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
        a = b;
    }
}

/// One shape of an icon. Coordinates are in the icon's viewBox.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    P(&'static str),
    Circle(f32, f32, f32),
    Ellipse(f32, f32, f32, f32),
    Rect(f32, f32, f32, f32, f32),
    /// Filled shape inside an otherwise stroked icon (`fill="currentColor" stroke="none"`).
    FillCircle(f32, f32, f32),
    FillRect(f32, f32, f32, f32, f32),
    FillP(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Icon {
    pub vb: (f32, f32),
    /// Stroke width in viewBox units; 0 = filled icon.
    pub stroke: f32,
    pub round_cap: bool,
    pub round_join: bool,
    pub shapes: &'static [Shape],
}

impl Icon {
    /// Draws the icon at (x, y) scaled to `w`×`h` CSS px, in `color`.
    pub fn draw(&self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        self.draw_rot(c, x, y, w, h, color, 0.0);
    }

    /// Rotated by `deg` about its centre.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rot(&self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, color: Rgba, deg: f32) {
        let sx = w / self.vb.0;
        let sy = h / self.vb.1;
        let mut ts = Transform::from_row(sx, 0.0, 0.0, sy, x, y);
        if deg != 0.0 {
            ts = Transform::from_rotate_at(deg, x + w / 2.0, y + h / 2.0).pre_concat(ts);
        }
        c.with_transform(ts, |c| {
            for s in self.shapes {
                let (path, filled) = match *s {
                    Shape::P(d) => (parse_path(d), self.stroke == 0.0),
                    Shape::FillP(d) => (parse_path(d), true),
                    Shape::Circle(cx, cy, r) => (PathBuilder::from_circle(cx, cy, r), self.stroke == 0.0),
                    Shape::FillCircle(cx, cy, r) => (PathBuilder::from_circle(cx, cy, r), true),
                    Shape::Ellipse(cx, cy, rx, ry) => {
                        (tiny_skia::Rect::from_xywh(cx - rx, cy - ry, rx * 2.0, ry * 2.0).and_then(PathBuilder::from_oval), self.stroke == 0.0)
                    }
                    Shape::Rect(x, y, w, h, r) => (super::gfx::rr(super::gfx::R::new(x, y, w, h), r), self.stroke == 0.0),
                    Shape::FillRect(x, y, w, h, r) => (super::gfx::rr(super::gfx::R::new(x, y, w, h), r), true),
                };
                let Some(path) = path else { continue };
                if filled {
                    c.fill_path(&path, color);
                } else {
                    let st = Stroke {
                        width: self.stroke,
                        line_cap: if self.round_cap { LineCap::Round } else { LineCap::Butt },
                        line_join: if self.round_join { LineJoin::Round } else { LineJoin::Miter },
                        ..Stroke::default()
                    };
                    c.stroke_path_with(&path, &st, color);
                }
            }
        });
    }
}
