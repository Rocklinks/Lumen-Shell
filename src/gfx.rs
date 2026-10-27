//! Software renderer: premultiplied ARGB canvas, AA shapes, TrueType text, image blit.
use crate::ttf::{Contour, Font};
use std::collections::HashMap;

/// Straight (non-premultiplied) color 0xAARRGGBB.
pub type Color = u32;

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
pub fn with_alpha(c: Color, a: f32) -> Color {
    let na = (((c >> 24) as f32) * a.clamp(0.0, 1.0)).round() as u32;
    (na << 24) | (c & 0xFFFFFF)
}
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| -> u32 {
        let x = ((a >> s) & 255) as f32;
        let y = ((b >> s) & 255) as f32;
        (x + (y - x) * t).round() as u32
    };
    (ch(24) << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}
pub fn luminance(c: Color) -> f32 {
    let r = ((c >> 16) & 255) as f32 / 255.0;
    let g = ((c >> 8) & 255) as f32 / 255.0;
    let b = (c & 255) as f32 / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}
fn premul(c: Color) -> u32 {
    let a = c >> 24;
    let r = ((c >> 16) & 255) * a / 255;
    let g = ((c >> 8) & 255) * a / 255;
    let b = (c & 255) * a / 255;
    (a << 24) | (r << 16) | (g << 8) | b
}

#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>, // premultiplied ARGB
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![0; w * h] }
    }
    pub fn clear(&mut self) {
        for p in self.px.iter_mut() {
            *p = 0;
        }
    }
    /// Blend one premultiplied source pixel scaled by coverage (0..255).
    #[inline]
    fn blend(&mut self, x: usize, y: usize, src_pm: u32, cov: u32) {
        let i = y * self.w + x;
        let d = self.px[i];
        let (sa, sr, sg, sb) = ((src_pm >> 24) * cov / 255, ((src_pm >> 16) & 255) * cov / 255, ((src_pm >> 8) & 255) * cov / 255, (src_pm & 255) * cov / 255);
        let ia = 255 - sa;
        let a = sa + (d >> 24) * ia / 255;
        let r = sr + ((d >> 16) & 255) * ia / 255;
        let g = sg + ((d >> 8) & 255) * ia / 255;
        let b = sb + (d & 255) * ia / 255;
        self.px[i] = (a.min(255) << 24) | (r.min(255) << 16) | (g.min(255) << 8) | b.min(255);
    }

    pub fn paint(&mut self, s: &Shape, color: Color) {
        let pm = premul(color);
        for y in 0..s.h {
            let cy = s.y + y as i32;
            if cy < 0 || cy >= self.h as i32 {
                continue;
            }
            for x in 0..s.w {
                let cx = s.x + x as i32;
                if cx < 0 || cx >= self.w as i32 {
                    continue;
                }
                let c = s.cov[y * s.w + x];
                if c > 0.003 {
                    self.blend(cx as usize, cy as usize, pm, (c.min(1.0) * 255.0 + 0.5) as u32);
                }
            }
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        let pm = premul(color);
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                self.blend(xx as usize, yy as usize, pm, 255);
            }
        }
    }
    pub fn fill_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
        let mut s = Shape::for_rect(x, y, w, h, 1);
        s.rrect(x, y, w, h, r);
        self.paint(&s, color);
    }
    pub fn stroke_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, color: Color) {
        let mut s = Shape::for_rect(x, y, w, h, 1);
        s.rrect_ring(x, y, w, h, r, t);
        self.paint(&s, color);
    }
    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let mut s = Shape::for_rect(cx - r, cy - r, r * 2.0, r * 2.0, 1);
        s.circle(cx, cy, r);
        self.paint(&s, color);
    }

    /// Composite `src` (premultiplied) over self at (x,y).
    pub fn draw_canvas(&mut self, src: &Canvas, x: i32, y: i32, opacity: f32) {
        let o = (opacity.clamp(0.0, 1.0) * 255.0) as u32;
        for sy in 0..src.h {
            let dy = y + sy as i32;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            for sx in 0..src.w {
                let dx = x + sx as i32;
                if dx < 0 || dx >= self.w as i32 {
                    continue;
                }
                let p = src.px[sy * src.w + sx];
                if p >> 24 != 0 {
                    self.blend(dx as usize, dy as usize, p, o);
                }
            }
        }
    }

    /// Scale an RGBA image (straight alpha, row-major) into the box using area averaging.
    pub fn draw_image(&mut self, img: &Image, x: i32, y: i32, w: i32, h: i32) {
        if img.w == 0 || img.h == 0 || w <= 0 || h <= 0 {
            return;
        }
        let sc = img.w as f32 / w as f32;
        let sr = img.h as f32 / h as f32;
        for dy in 0..h {
            let py = y + dy;
            if py < 0 || py >= self.h as i32 {
                continue;
            }
            for dx in 0..w {
                let px = x + dx;
                if px < 0 || px >= self.w as i32 {
                    continue;
                }
                let (x0, x1) = (dx as f32 * sc, (dx + 1) as f32 * sc);
                let (y0, y1) = (dy as f32 * sr, (dy + 1) as f32 * sr);
                let (ix0, ix1) = (x0.floor() as usize, (x1.ceil() as usize).min(img.w));
                let (iy0, iy1) = (y0.floor() as usize, (y1.ceil() as usize).min(img.h));
                let (mut ar, mut ag, mut ab, mut aa, mut wt) = (0f32, 0f32, 0f32, 0f32, 0f32);
                for iy in iy0..iy1 {
                    let wy = (y1.min(iy as f32 + 1.0) - y0.max(iy as f32)).max(0.0);
                    for ix in ix0..ix1 {
                        let wx = (x1.min(ix as f32 + 1.0) - x0.max(ix as f32)).max(0.0);
                        let wgt = wx * wy;
                        let p = img.px[iy * img.w + ix];
                        let a = (p >> 24) as f32 / 255.0;
                        ar += ((p >> 16) & 255) as f32 * a * wgt;
                        ag += ((p >> 8) & 255) as f32 * a * wgt;
                        ab += (p & 255) as f32 * a * wgt;
                        aa += a * wgt;
                        wt += wgt;
                    }
                }
                if wt > 0.0 && aa > 0.0 {
                    let pm = (((aa / wt) * 255.0 + 0.5) as u32) << 24
                        | (((ar / wt) + 0.5) as u32).min(255) << 16
                        | (((ag / wt) + 0.5) as u32).min(255) << 8
                        | (((ab / wt) + 0.5) as u32).min(255);
                    self.blend(px as usize, py as usize, pm, 255);
                }
            }
        }
    }
}

/// Straight-alpha RGBA image (0xAARRGGBB per pixel).
#[derive(Clone, Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

/// Anti-aliased coverage mask for a pixel box; shapes union via max().
pub struct Shape {
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub cov: Vec<f32>,
}

impl Shape {
    pub fn for_rect(x: f32, y: f32, w: f32, h: f32, pad: i32) -> Shape {
        let x0 = x.floor() as i32 - pad;
        let y0 = y.floor() as i32 - pad;
        let x1 = (x + w).ceil() as i32 + pad;
        let y1 = (y + h).ceil() as i32 + pad;
        let (sw, sh) = (((x1 - x0).max(1) as usize).min(8192), ((y1 - y0).max(1) as usize).min(8192));
        Shape { x: x0, y: y0, w: sw, h: sh, cov: vec![0.0; sw * sh] }
    }
    fn each<F: Fn(f32, f32) -> f32>(&mut self, f: F) {
        for y in 0..self.h {
            for x in 0..self.w {
                let px = (self.x + x as i32) as f32 + 0.5;
                let py = (self.y + y as i32) as f32 + 0.5;
                let c = f(px, py);
                let i = y * self.w + x;
                if c > self.cov[i] {
                    self.cov[i] = c;
                }
            }
        }
    }
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        self.each(|px, py| {
            let qx = (px - cx).abs() - hx;
            let qy = (py - cy).abs() - hy;
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            (0.5 - d).clamp(0.0, 1.0)
        });
    }
    pub fn rrect_ring(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        self.each(|px, py| {
            let qx = (px - cx).abs() - hx;
            let qy = (py - cy).abs() - hy;
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            (0.5 - (d + t / 2.0).abs() + t / 2.0).clamp(0.0, 1.0)
        });
    }
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.each(|px, py| (0.5 - (((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r)).clamp(0.0, 1.0));
    }
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, t: f32) {
        self.each(|px, py| {
            let d = (((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r).abs() - t / 2.0;
            (0.5 - d).clamp(0.0, 1.0)
        });
    }
    pub fn capsule(&mut self, ax: f32, ay: f32, bx: f32, by: f32, r: f32) {
        self.each(|px, py| {
            let (vx, vy) = (bx - ax, by - ay);
            let l2 = vx * vx + vy * vy;
            let t = if l2 < 1e-6 { 0.0 } else { (((px - ax) * vx + (py - ay) * vy) / l2).clamp(0.0, 1.0) };
            let d = ((px - ax - vx * t).powi(2) + (py - ay - vy * t).powi(2)).sqrt() - r;
            (0.5 - d).clamp(0.0, 1.0)
        });
    }
    /// Arc from angle a0 to a1 (radians, 0 = +x, clockwise on screen), stroke width t.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, t: f32, a0: f32, a1: f32) {
        let n = (((a1 - a0).abs() * r).ceil() as usize).clamp(4, 64);
        let mut prev = (cx + r * a0.cos(), cy + r * a0.sin());
        for i in 1..=n {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            let p = (cx + r * a.cos(), cy + r * a.sin());
            self.capsule(prev.0, prev.1, p.0, p.1, t / 2.0);
            prev = p;
        }
    }
    pub fn polygon(&mut self, pts: &[(f32, f32)]) {
        let mut r = Raster::new(self.w, self.h);
        let o = (self.x as f32, self.y as f32);
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            r.line((a.0 - o.0, a.1 - o.1), (b.0 - o.0, b.1 - o.1));
        }
        let c = r.coverage();
        for (d, s) in self.cov.iter_mut().zip(c.iter()) {
            if *s > *d {
                *d = *s;
            }
        }
    }
}

/// Signed-area accumulation rasterizer (exact-area AA), used for glyphs and polygons.
pub struct Raster {
    w: usize,
    h: usize,
    stride: usize,
    a: Vec<f32>,
}
impl Raster {
    pub fn new(w: usize, h: usize) -> Raster {
        let stride = w + 3;
        Raster { w, h, stride, a: vec![0.0; stride * h + 8] }
    }
    pub fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if (p0.1 - p1.1).abs() < 1e-9 {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 { (1.0f32, p0, p1) } else { (-1.0f32, p1, p0) };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        let mut y0 = 0usize;
        if p0.1 > 0.0 {
            y0 = p0.1 as usize;
        } else {
            x -= p0.1 * dxdy;
        }
        let ymax = self.h.min(p1.1.ceil().max(0.0) as usize);
        let wmax = self.w as f32;
        for y in y0..ymax {
            let ls = y * self.stride;
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let (xa, xb) = (xa.clamp(0.0, wmax), xb.clamp(0.0, wmax));
            let x0f = xa.floor();
            let x0i = x0f as usize;
            let x1c = xb.ceil();
            let x1i = x1c as usize;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (xa + xb) - x0f;
                self.a[ls + x0i] += d - d * xmf;
                self.a[ls + x0i + 1] += d * xmf;
            } else {
                let s = 1.0 / (xb - xa);
                let x0fr = xa - x0f;
                let a0 = 0.5 * s * (1.0 - x0fr) * (1.0 - x0fr);
                let x1fr = xb - x1c + 1.0;
                let am = 0.5 * s * x1fr * x1fr;
                self.a[ls + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.a[ls + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0fr);
                    self.a[ls + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.a[ls + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.a[ls + x1i - 1] += d * (1.0 - a2 - am);
                }
                self.a[ls + x1i] += d * am;
            }
            x = xnext;
        }
    }
    pub fn coverage(&self) -> Vec<f32> {
        let mut out = vec![0.0; self.w * self.h];
        for y in 0..self.h {
            let mut acc = 0.0f32;
            for x in 0..self.w {
                acc += self.a[y * self.stride + x];
                out[y * self.w + x] = acc.abs().min(1.0);
            }
        }
        out
    }
}

pub struct Glyph {
    pub w: usize,
    pub h: usize,
    pub left: i32,
    pub top: i32, // pixels above baseline
    pub adv: f32,
    pub cov: Vec<u8>,
}

fn flatten(contour: &Contour, scale: f32, ox: f32, oy: f32, r: &mut Raster) {
    let n = contour.len();
    if n < 2 {
        return;
    }
    let tr = |p: &crate::ttf::Pt| (p.x * scale + ox, -p.y * scale + oy);
    // find a starting on-curve point
    let mut pts: Vec<((f32, f32), bool)> = contour.iter().map(|p| (tr(p), p.on)).collect();
    let start_idx = pts.iter().position(|p| p.1);
    let start = match start_idx {
        Some(i) => {
            pts.rotate_left(i);
            pts[0].0
        }
        None => {
            let a = pts[0].0;
            let b = pts[n - 1].0;
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            pts.insert(0, (mid, true));
            mid
        }
    };
    let mut cur = start;
    let mut i = 1;
    let len = pts.len();
    let mut segs: Vec<((f32, f32), (f32, f32), (f32, f32))> = Vec::new(); // quad: from, ctrl, to ; ctrl==from => line
    while i <= len {
        let p = pts[i % len];
        if p.1 {
            segs.push((cur, cur, p.0));
            cur = p.0;
            i += 1;
        } else {
            let next = pts[(i + 1) % len];
            if next.1 {
                segs.push((cur, p.0, next.0));
                cur = next.0;
                i += 2;
            } else {
                let mid = ((p.0 .0 + next.0 .0) / 2.0, (p.0 .1 + next.0 .1) / 2.0);
                segs.push((cur, p.0, mid));
                cur = mid;
                i += 1;
            }
        }
        if i > len + 1 {
            break;
        }
    }
    let mut last = start;
    for (a, c, b) in segs {
        if a == c {
            r.line(last, b);
        } else {
            let steps = 8;
            let mut prev = a;
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                let u = 1.0 - t;
                let p = (u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1);
                r.line(prev, p);
                prev = p;
            }
        }
        last = b;
    }
    r.line(last, start);
}

pub struct TextCtx {
    pub fonts: Vec<Font>, // primary first, then fallbacks
    cache: HashMap<(char, u32), std::rc::Rc<Glyph>>,
}

impl TextCtx {
    pub fn new(fonts: Vec<Font>) -> TextCtx {
        TextCtx { fonts, cache: HashMap::new() }
    }
    pub fn usable(&self) -> bool {
        !self.fonts.is_empty()
    }
    fn pick(&self, ch: char) -> Option<&Font> {
        self.fonts.iter().find(|f| f.has_glyph(ch)).or_else(|| self.fonts.first())
    }
    pub fn glyph(&mut self, ch: char, px: f32) -> Option<std::rc::Rc<Glyph>> {
        let key = (ch, (px * 4.0) as u32);
        if let Some(g) = self.cache.get(&key) {
            return Some(g.clone());
        }
        let f = self.pick(ch)?;
        let gid = f.glyph_index(ch);
        let scale = px / f.units;
        let adv = f.advance(gid) * scale;
        let outline = f.outline(gid);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in &outline {
            for p in c {
                x0 = x0.min(p.x * scale);
                x1 = x1.max(p.x * scale);
                y0 = y0.min(-p.y * scale);
                y1 = y1.max(-p.y * scale);
            }
        }
        let g = if outline.is_empty() || x1 < x0 {
            Glyph { w: 0, h: 0, left: 0, top: 0, adv, cov: vec![] }
        } else {
            let left = x0.floor() as i32 - 1;
            let top = (-y0).ceil() as i32 + 1;
            let w = ((x1.ceil() as i32 - left) + 2).clamp(1, 512) as usize;
            let h = (((y1.ceil() as f32 + top as f32) as i32) + 2).clamp(1, 512) as usize;
            let mut r = Raster::new(w, h);
            for c in &outline {
                flatten(c, scale, -(left as f32), top as f32, &mut r);
            }
            let cov = r.coverage().iter().map(|v| (v * 255.0 + 0.5) as u8).collect();
            Glyph { w, h, left, top, adv, cov }
        };
        let rc = std::rc::Rc::new(g);
        if self.cache.len() > 4000 {
            self.cache.clear();
        }
        self.cache.insert(key, rc.clone());
        Some(rc)
    }
    pub fn width(&mut self, s: &str, px: f32) -> f32 {
        let mut w = 0.0;
        for ch in s.chars() {
            if let Some(g) = self.glyph(ch, px) {
                w += g.adv;
            }
        }
        w
    }
    pub fn line_height(&self, px: f32) -> f32 {
        self.fonts.first().map_or(px * 1.2, |f| (f.ascent - f.descent + f.line_gap) / f.units * px)
    }
    pub fn ascent(&self, px: f32) -> f32 {
        self.fonts.first().map_or(px * 0.8, |f| f.ascent / f.units * px)
    }
    /// Draw with baseline at y. Returns advance width.
    pub fn draw(&mut self, cv: &mut Canvas, s: &str, px: f32, x: f32, baseline: f32, color: Color) -> f32 {
        let mut pen = x;
        let pm = premul(color);
        for ch in s.chars() {
            if let Some(g) = self.glyph(ch, px) {
                let gx = (pen + g.left as f32).round() as i32;
                let gy = baseline.round() as i32 - g.top;
                for yy in 0..g.h {
                    let cy = gy + yy as i32;
                    if cy < 0 || cy >= cv.h as i32 {
                        continue;
                    }
                    for xx in 0..g.w {
                        let cx = gx + xx as i32;
                        if cx < 0 || cx >= cv.w as i32 {
                            continue;
                        }
                        let c = g.cov[yy * g.w + xx] as u32;
                        if c > 0 {
                            cv.blend(cx as usize, cy as usize, pm, c);
                        }
                    }
                }
                pen += g.adv;
            }
        }
        pen - x
    }
    /// Truncate with ellipsis to fit max_w.
    pub fn fit(&mut self, s: &str, px: f32, max_w: f32) -> String {
        if self.width(s, px) <= max_w {
            return s.to_string();
        }
        let mut out = String::new();
        let ell = self.width("…", px);
        for ch in s.chars() {
            let t = format!("{}{}", out, ch);
            if self.width(&t, px) + ell > max_w {
                break;
            }
            out.push(ch);
        }
        out.push('…');
        out
    }
}

/// Uncompressed PNG writer (dev/test screenshots).
pub fn write_png(path: &str, cv: &Canvas, bg: Color) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((cv.w * 3 + 1) * cv.h);
    let (br, bgc, bb) = ((bg >> 16) & 255, (bg >> 8) & 255, bg & 255);
    for y in 0..cv.h {
        raw.push(0);
        for x in 0..cv.w {
            let p = cv.px[y * cv.w + x];
            let ia = 255 - (p >> 24);
            raw.push((((p >> 16) & 255) + br * ia / 255).min(255) as u8);
            raw.push((((p >> 8) & 255) + bgc * ia / 255).min(255) as u8);
            raw.push(((p & 255) + bb * ia / 255).min(255) as u8);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut i = 0;
    while i < raw.len() {
        let n = (raw.len() - i).min(65535);
        z.push(if i + n >= raw.len() { 1 } else { 0 });
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[i..i + n]);
        i += n;
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFFFFFFu32;
        for &v in data {
            c ^= v as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut chunk = |tag: &[u8; 4], d: &[u8]| {
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut t = tag.to_vec();
        t.extend_from_slice(d);
        out.extend_from_slice(&t);
        out.extend_from_slice(&crc(&t).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(cv.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(cv.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, out)
}

impl Raster {
    /// Even-odd variant of coverage (folds accumulated winding).
    pub fn coverage_evenodd(&self) -> Vec<f32> {
        let mut out = vec![0.0; self.w * self.h];
        for y in 0..self.h {
            let mut acc = 0.0f32;
            for x in 0..self.w {
                acc += self.a[y * self.stride + x];
                let a = acc.abs() % 2.0;
                out[y * self.w + x] = if a > 1.0 { 2.0 - a } else { a };
            }
        }
        out
    }
}
