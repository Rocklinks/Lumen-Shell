//! Drawing helpers shared by bar, popups, dock and grid.
use crate::gfx::*;
use crate::icons::Icons;
use crate::theme::Theme;

pub type R = (f32, f32, f32, f32);

pub fn inside(r: R, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

pub struct Paint<'a> {
    pub tx: &'a mut TextCtx,
    pub txb: &'a mut TextCtx,
    pub icons: &'a mut Icons,
    pub th: &'a Theme,
    pub s: f32,
}

impl<'a> Paint<'a> {
    /// Base UI font size in pixels (GNOME's 11pt at 96 dpi by default).
    pub fn px(&self, pt_mul: f32) -> f32 {
        self.th.font_size * 96.0 / 72.0 * self.s * pt_mul
    }
    pub fn text_w(&mut self, t: &str, px: f32, bold: bool) -> f32 {
        if bold { self.txb.width(t, px) } else { self.tx.width(t, px) }
    }
    /// Draw text vertically centered on `cy`. Returns width.
    pub fn text(&mut self, cv: &mut Canvas, t: &str, px: f32, x: f32, cy: f32, color: Color, bold: bool) -> f32 {
        let ctx = if bold { &mut *self.txb } else { &mut *self.tx };
        if !ctx.usable() {
            return 0.0;
        }
        let asc = ctx.ascent(px);
        let lh = ctx.line_height(px);
        let base = cy - lh / 2.0 + asc;
        ctx.draw(cv, t, px, x, base, color)
    }
    pub fn text_fit(&mut self, cv: &mut Canvas, t: &str, px: f32, x: f32, cy: f32, max_w: f32, color: Color, bold: bool) -> f32 {
        let s = if bold { self.txb.fit(t, px, max_w) } else { self.tx.fit(t, px, max_w) };
        self.text(cv, &s, px, x, cy, color, bold)
    }
    /// Word-wrap into at most `max_lines` lines (last line ellipsized if cut).
    pub fn wrap(&mut self, t: &str, px: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = vec![];
        let mut cur = String::new();
        for para in t.split('\n') {
            for word in para.split_whitespace() {
                let cand = if cur.is_empty() { word.to_string() } else { format!("{} {}", cur, word) };
                if self.tx.width(&cand, px) <= max_w || cur.is_empty() {
                    cur = cand;
                } else {
                    lines.push(std::mem::take(&mut cur));
                    cur = word.to_string();
                }
                if lines.len() >= max_lines {
                    break;
                }
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if lines.len() >= max_lines {
                break;
            }
        }
        let cut = lines.len() > max_lines;
        lines.truncate(max_lines);
        let n = lines.len();
        for (i, l) in lines.iter_mut().enumerate() {
            if self.tx.width(l, px) > max_w || (cut && i + 1 == n) {
                *l = self.tx.fit(&format!("{}…", l.trim_end_matches('…')), px, max_w);
            }
        }
        lines
    }
    pub fn text_center(&mut self, cv: &mut Canvas, t: &str, px: f32, cx: f32, cy: f32, color: Color, bold: bool) {
        let w = self.text_w(t, px, bold);
        self.text(cv, t, px, cx - w / 2.0, cy, color, bold);
    }
    /// Themed symbolic icon tinted to `color`, optically normalized so all glyphs have the same visual size.
    pub fn icon(&mut self, cv: &mut Canvas, name: &str, x: f32, y: f32, size: f32, color: Color) {
        if name == "@cup" {
            let u = size / 16.0;
            let mut sh = Shape::for_rect(x, y, size, size, 2);
            sh.rrect(x + 2.5 * u, y + 6.0 * u, 8.5 * u, 7.5 * u, 2.0 * u);
            sh.arc(x + 11.0 * u, y + 9.2 * u, 2.2 * u, 1.6 * u, -1.57, 1.57);
            sh.capsule(x + 4.5 * u, y + 1.5 * u, x + 4.5 * u, y + 4.0 * u, 0.6 * u);
            sh.capsule(x + 7.0 * u, y + 1.0 * u, x + 7.0 * u, y + 4.0 * u, 0.6 * u);
            sh.capsule(x + 9.5 * u, y + 1.5 * u, x + 9.5 * u, y + 4.0 * u, 0.6 * u);
            cv.paint(&sh, color);
            return;
        }
        let sz = (size * 1.5).round().max(12.0) as u32;
        match self.icons.get_fit(name, sz, color) {
            Some(img) => {
                let target = size * 0.84;
                let sc = (target / img.w.max(img.h) as f32).clamp(0.3, 1.0);
                let (w, h) = ((img.w as f32 * sc).round().max(1.0), (img.h as f32 * sc).round().max(1.0));
                cv.draw_image(&img, (x + (size - w) / 2.0).round() as i32, (y + (size - h) / 2.0).round() as i32, w as i32, h as i32);
            }
            None => cv.fill_circle(x + size / 2.0, y + size / 2.0, size * 0.18, color),
        }
    }
    /// App icon (not tinted); falls back to a colored letter tile.
    pub fn app_icon(&mut self, cv: &mut Canvas, name: &str, label: &str, x: f32, y: f32, size: f32) {
        let sz = size.round().max(8.0) as u32;
        match self.icons.get(name, sz, None) {
            Some(img) => cv.draw_image(&img, x.round() as i32, y.round() as i32, sz as i32, sz as i32),
            None => {
                let h = label.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
                let hue = [0xFF3584E4u32, 0xFF33D17A, 0xFFF6D32D, 0xFFFF7800, 0xFFE01B24, 0xFF9141AC, 0xFF865E3C][(h % 7) as usize];
                cv.fill_rrect(x + size * 0.06, y + size * 0.06, size * 0.88, size * 0.88, size * 0.22, hue);
                let c: String = label.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                let px = size * 0.5;
                self.text_center(cv, &c, px, x + size / 2.0, y + size / 2.0, 0xFFFFFFFF, true);
            }
        }
    }
    pub fn slider(&mut self, cv: &mut Canvas, r: R, frac: f32, accent: Color, track: Color) {
        let h = 6.0 * self.s;
        let cy = r.1 + r.3 / 2.0;
        cv.fill_rrect(r.0, cy - h / 2.0, r.2, h, h / 2.0, track);
        let f = frac.clamp(0.0, 1.0);
        let kn = 8.0 * self.s;
        let w = (r.2 - 2.0 * kn) * f + 2.0 * kn;
        cv.fill_rrect(r.0, cy - h / 2.0, w.max(h), h, h / 2.0, accent);
        cv.fill_circle(r.0 + w - kn, cy, kn, 0xFFFFFFFF);
    }
    pub fn switch(&mut self, cv: &mut Canvas, x: f32, cy: f32, on: bool, accent: Color) {
        let (w, h) = (40.0 * self.s, 22.0 * self.s);
        cv.fill_rrect(x, cy - h / 2.0, w, h, h / 2.0, if on { accent } else { 0xFF5A5A5A });
        let kx = if on { x + w - h / 2.0 } else { x + h / 2.0 };
        cv.fill_circle(kx, cy, h / 2.0 - 2.5 * self.s, 0xFFFFFFFF);
    }
}

/// Soft drop shadow approximation: stacked translucent rounded rects.
pub fn shadow(cv: &mut Canvas, x: f32, y: f32, w: f32, h: f32, r: f32, spread: f32, strength: f32) {
    let steps = 6;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let g = spread * (1.0 - t);
        cv.fill_rrect(x - g, y - g + spread * 0.35, w + 2.0 * g, h + 2.0 * g, r + g, with_alpha(0xFF000000, strength * 0.10 * (0.4 + t)));
    }
}

pub fn days_in_month(y: i32, m: i32) -> i32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 },
    }
}
/// 0 = Sunday.
pub fn weekday(y: i32, m: i32, d: i32) -> i32 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d) % 7
}
pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const MONTHS_S: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
pub const DAYS_S: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
