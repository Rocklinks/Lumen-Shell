//! Small SVG renderer: paths (all commands incl. arcs), basic shapes, groups, <use>, transforms,
//! solid fills/strokes, gradients approximated per-pixel (linear/radial, userSpace & bbox). No filters/clips.
use crate::gfx::{Image, Raster};
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
struct Node {
    tag: String,
    attrs: HashMap<String, String>,
    kids: Vec<Node>,
}

fn parse_xml(s: &str) -> Option<Node> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut stack: Vec<Node> = vec![Node::default()];
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if s[i..].starts_with("<!--") {
            i = s[i..].find("-->").map(|e| i + e + 3)?;
            continue;
        }
        if s[i..].starts_with("<![CDATA[") {
            i = s[i..].find("]]>").map(|e| i + e + 3)?;
            continue;
        }
        if s[i..].starts_with("<?") || s[i..].starts_with("<!") {
            // DOCTYPE may contain [ ... ]
            let mut depth = 0;
            while i < b.len() {
                match b[i] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => break,
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        // tag
        let mut j = i + 1;
        let mut q = 0u8;
        while j < b.len() {
            let c = b[j];
            if q != 0 {
                if c == q {
                    q = 0;
                }
            } else if c == b'"' || c == b'\'' {
                q = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        if j >= b.len() {
            return None;
        }
        let inner = &s[i + 1..j];
        i = j + 1;
        if let Some(name) = inner.strip_prefix('/') {
            let _ = name;
            let n = stack.pop()?;
            stack.last_mut()?.kids.push(n);
            if stack.len() == 1 {
                break;
            }
            continue;
        }
        let selfclose = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let mut it = inner.splitn(2, |c: char| c.is_whitespace());
        let tag = it.next()?.rsplit(':').next()?.to_string();
        let mut node = Node { tag, ..Default::default() };
        if let Some(rest) = it.next() {
            parse_attrs(rest, &mut node.attrs);
        }
        if selfclose {
            stack.last_mut()?.kids.push(node);
        } else {
            if stack.len() > 64 {
                return None;
            }
            stack.push(node);
        }
    }
    while stack.len() > 1 {
        let n = stack.pop()?;
        stack.last_mut()?.kids.push(n);
    }
    let root = stack.pop()?;
    root.kids.into_iter().find(|n| n.tag == "svg")
}

fn parse_attrs(s: &str, out: &mut HashMap<String, String>) {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_whitespace()) {
            i += 1;
        }
        let st = i;
        while i < b.len() && b[i] != '=' && !b[i].is_whitespace() {
            i += 1;
        }
        let name: String = b[st..i].iter().collect();
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != '=' {
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let q = b[i];
        if q != '"' && q != '\'' {
            continue;
        }
        i += 1;
        let vs = i;
        while i < b.len() && b[i] != q {
            i += 1;
        }
        let val: String = b[vs..i.min(b.len())].iter().collect();
        i += 1;
        let key = name.rsplit(':').next().unwrap_or("").to_string();
        out.insert(key, val);
    }
}

type M = [f32; 6]; // a b c d e f
fn mul(p: &M, c: &M) -> M {
    [p[0] * c[0] + p[2] * c[1], p[1] * c[0] + p[3] * c[1], p[0] * c[2] + p[2] * c[3], p[1] * c[2] + p[3] * c[3], p[0] * c[4] + p[2] * c[5] + p[4], p[1] * c[4] + p[3] * c[5] + p[5]]
}
fn ap(m: &M, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}
fn inv(m: &M) -> Option<M> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d.abs() < 1e-9 {
        return None;
    }
    Some([m[3] / d, -m[1] / d, -m[2] / d, m[0] / d, (m[2] * m[5] - m[3] * m[4]) / d, (m[1] * m[4] - m[0] * m[5]) / d])
}

fn nums(s: &str) -> Vec<f32> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut prev = ' ';
    for c in s.chars() {
        let sep = c == ',' || c.is_whitespace() || c == '(' || c == ')';
        let new_num = (c == '-' || c == '+') && !cur.is_empty() && prev != 'e' && prev != 'E';
        let dot2 = c == '.' && cur.contains('.');
        if sep || new_num || dot2 {
            if let Ok(v) = cur.parse::<f32>() {
                out.push(v);
            }
            cur.clear();
        }
        if !sep {
            cur.push(c);
        }
        prev = c;
    }
    if let Ok(v) = cur.parse::<f32>() {
        out.push(v);
    }
    out
}

fn parse_transform(s: &str) -> M {
    let mut m: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut rest = s;
    while let Some(p) = rest.find('(') {
        let name = rest[..p].trim().rsplit(|c: char| c == ')' || c.is_whitespace() || c == ',').next().unwrap_or("").to_string();
        let q = match rest[p..].find(')') {
            Some(q) => p + q,
            None => break,
        };
        let a = nums(&rest[p + 1..q]);
        let t: M = match (name.as_str(), a.len()) {
            ("matrix", 6) => [a[0], a[1], a[2], a[3], a[4], a[5]],
            ("translate", 1) => [1.0, 0.0, 0.0, 1.0, a[0], 0.0],
            ("translate", 2) => [1.0, 0.0, 0.0, 1.0, a[0], a[1]],
            ("scale", 1) => [a[0], 0.0, 0.0, a[0], 0.0, 0.0],
            ("scale", 2) => [a[0], 0.0, 0.0, a[1], 0.0, 0.0],
            ("rotate", n) if n == 1 || n == 3 => {
                let r = a[0].to_radians();
                let (s, c) = (r.sin(), r.cos());
                let rot: M = [c, s, -s, c, 0.0, 0.0];
                if n == 3 {
                    mul(&mul(&[1.0, 0.0, 0.0, 1.0, a[1], a[2]], &rot), &[1.0, 0.0, 0.0, 1.0, -a[1], -a[2]])
                } else {
                    rot
                }
            }
            ("skewX", 1) => [1.0, 0.0, a[0].to_radians().tan(), 1.0, 0.0, 0.0],
            ("skewY", 1) => [1.0, a[0].to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        m = mul(&m, &t);
        rest = &rest[q + 1..];
    }
    m
}

#[derive(Clone, Debug)]
enum Paint {
    None,
    Solid(u32), // 0xAARRGGBB
    Grad(String),
}

fn named_color(n: &str) -> Option<u32> {
    Some(match n {
        "black" => 0xFF000000,
        "white" => 0xFFFFFFFF,
        "red" => 0xFFFF0000,
        "green" => 0xFF008000,
        "blue" => 0xFF0000FF,
        "gray" | "grey" => 0xFF808080,
        "yellow" => 0xFFFFFF00,
        "orange" => 0xFFFFA500,
        "transparent" => 0x00000000,
        _ => return None,
    })
}

fn parse_paint(v: &str, current: u32) -> Option<Paint> {
    let v = v.trim();
    if v == "none" {
        return Some(Paint::None);
    }
    if v == "currentColor" {
        return Some(Paint::Solid(current));
    }
    if let Some(u) = v.strip_prefix("url(") {
        let id = u.trim_start_matches(|c| c == '#' || c == '\'' || c == '"').split(|c| c == ')' || c == '\'' || c == '"').next()?.to_string();
        return Some(Paint::Grad(id));
    }
    if let Some(h) = v.strip_prefix('#') {
        let x = u32::from_str_radix(h, 16).ok()?;
        return Some(Paint::Solid(match h.len() {
            6 => 0xFF000000 | x,
            3 => 0xFF000000 | ((x >> 8 & 15) * 17) << 16 | ((x >> 4 & 15) * 17) << 8 | (x & 15) * 17,
            8 => ((x & 255) << 24) | (x >> 8),
            _ => return None,
        }));
    }
    if v.starts_with("rgb") {
        let a = nums(&v[v.find('(')?..]);
        if a.len() >= 3 {
            let al = a.get(3).copied().unwrap_or(1.0);
            return Some(Paint::Solid((((al * 255.0) as u32) << 24) | ((a[0] as u32).min(255) << 16) | ((a[1] as u32).min(255) << 8) | (a[2] as u32).min(255)));
        }
    }
    named_color(v).map(Paint::Solid)
}

#[derive(Clone)]
struct Style {
    fill: Paint,
    stroke: Paint,
    sw: f32,
    opacity: f32,
    fill_op: f32,
    stroke_op: f32,
    color: u32,
    evenodd: bool,
}

struct Ctx<'a> {
    defs: HashMap<String, &'a Node>,
    img_w: usize,
    img_h: usize,
    acc: Vec<f32>, // premultiplied RGBA f32 *4
    tint: Option<u32>,
    depth: u32,
    budget: u32,
}

fn style_get(n: &Node, key: &str) -> Option<String> {
    if let Some(st) = n.attrs.get("style") {
        for part in st.split(';') {
            if let Some((k, v)) = part.split_once(':') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    n.attrs.get(key).cloned()
}

fn len(s: &str) -> f32 {
    s.trim().trim_end_matches("px").trim_end_matches('%').parse::<f32>().unwrap_or(0.0)
}

impl<'a> Ctx<'a> {
    fn collect_defs(&mut self, n: &'a Node) {
        if let Some(id) = n.attrs.get("id") {
            self.defs.insert(id.clone(), n);
        }
        for k in &n.kids {
            self.collect_defs(k);
        }
    }

    fn walk(&mut self, n: &'a Node, m: &M, st: &Style) {
        self.depth += 1;
        if self.depth > 40 || self.budget == 0 {
            self.depth -= 1;
            return;
        }
        let mut st = st.clone();
        if let Some(c) = style_get(n, "color").and_then(|c| match parse_paint(&c, 0xFF000000) {
            Some(Paint::Solid(c)) => Some(c),
            _ => None,
        }) {
            st.color = c;
        }
        if let Some(v) = style_get(n, "fill") {
            if let Some(p) = parse_paint(&v, st.color) {
                st.fill = p;
            }
        }
        if let Some(v) = style_get(n, "stroke") {
            if let Some(p) = parse_paint(&v, st.color) {
                st.stroke = p;
            }
        }
        if let Some(v) = style_get(n, "stroke-width") {
            st.sw = len(&v);
        }
        if let Some(v) = style_get(n, "fill-opacity") {
            st.fill_op = v.trim().parse().unwrap_or(1.0);
        }
        if let Some(v) = style_get(n, "stroke-opacity") {
            st.stroke_op = v.trim().parse().unwrap_or(1.0);
        }
        if let Some(v) = style_get(n, "fill-rule") {
            st.evenodd = v.trim() == "evenodd";
        }
        let own_op: f32 = style_get(n, "opacity").and_then(|v| v.trim().parse().ok()).unwrap_or(1.0);
        st.opacity *= own_op.clamp(0.0, 1.0);
        if matches!(style_get(n, "display").as_deref(), Some("none")) || matches!(style_get(n, "visibility").as_deref(), Some("hidden")) {
            self.depth -= 1;
            return;
        }
        let mut mm = *m;
        if let Some(t) = n.attrs.get("transform") {
            mm = mul(m, &parse_transform(t));
        }
        match n.tag.as_str() {
            "g" | "a" | "svg" | "switch" => {
                for k in &n.kids {
                    self.walk(k, &mm, &st);
                }
            }
            "defs" | "linearGradient" | "radialGradient" | "clipPath" | "mask" | "filter" | "style" | "title" | "desc" | "metadata" | "pattern" | "symbol" | "namedview" | "sodipodi" => {}
            "use" => {
                if let Some(h) = n.attrs.get("href") {
                    if let Some(t) = self.defs.get(h.trim_start_matches('#')).copied() {
                        let (x, y) = (n.attrs.get("x").map_or(0.0, |v| len(v)), n.attrs.get("y").map_or(0.0, |v| len(v)));
                        let m2 = mul(&mm, &[1.0, 0.0, 0.0, 1.0, x, y]);
                        self.walk(t, &m2, &st);
                    }
                }
            }
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                let subs = shape_to_paths(n);
                if !subs.is_empty() {
                    self.draw(&subs, &mm, &st);
                }
            }
            _ => {}
        }
        self.depth -= 1;
    }

    fn grad_fn(&self, id: &str) -> Option<GradInfo> {
        let n = *self.defs.get(id)?;
        let mut stops: Vec<(f32, u32)> = vec![];
        let mut cur = n;
        let mut guard = 0;
        let mut attrs = n.attrs.clone();
        loop {
            for (k, v) in &cur.attrs {
                attrs.entry(k.clone()).or_insert_with(|| v.clone());
            }
            if stops.is_empty() {
                for s in cur.kids.iter().filter(|k| k.tag == "stop") {
                    let off_s = style_get(s, "offset").unwrap_or_default();
                    let off = if off_s.ends_with('%') { len(&off_s) / 100.0 } else { off_s.parse().unwrap_or(0.0) };
                    let mut col = match style_get(s, "stop-color").and_then(|c| parse_paint(&c, 0xFF000000)) {
                        Some(Paint::Solid(c)) => c,
                        _ => 0xFF000000,
                    };
                    let so: f32 = style_get(s, "stop-opacity").and_then(|v| v.parse().ok()).unwrap_or(1.0);
                    col = (((((col >> 24) as f32) * so) as u32) << 24) | (col & 0xFFFFFF);
                    stops.push((off, col));
                }
            }
            match cur.attrs.get("href").and_then(|h| self.defs.get(h.trim_start_matches('#')).copied()) {
                Some(next) if guard < 8 => {
                    cur = next;
                    guard += 1;
                }
                _ => break,
            }
        }
        if stops.is_empty() {
            return None;
        }
        let units_user = attrs.get("gradientUnits").map_or(false, |u| u == "userSpaceOnUse");
        let gt = attrs.get("gradientTransform").map(|t| parse_transform(t)).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let f = |k: &str, d: f32| attrs.get(k).map_or(d, |v| if v.ends_with('%') { len(v) / 100.0 } else { len(v) });
        let radial = n.tag == "radialGradient";
        let (x1, y1, x2, y2) = if radial { (f("cx", 0.5), f("cy", 0.5), f("fx", f("cx", 0.5)), f("fy", f("cy", 0.5))) } else { (f("x1", 0.0), f("y1", 0.0), f("x2", 1.0), f("y2", 0.0)) };
        let r = f("r", 0.5);
        Some(GradInfo { stops, units_user, gt, radial, x1, y1, x2, y2, r })
    }

    fn draw(&mut self, subs: &[Vec<(f32, f32)>], m: &M, st: &Style) {
        self.budget = self.budget.saturating_sub(1);
        // device-space polygons
        let polys: Vec<Vec<(f32, f32)>> = subs.iter().map(|s| s.iter().map(|&(x, y)| ap(m, x, y)).collect()).collect();
        let scale = ((m[0] * m[3] - m[1] * m[2]).abs()).sqrt();
        // bbox in user space for objectBoundingBox gradients
        let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for s in subs {
            for &(x, y) in s {
                bx0 = bx0.min(x);
                by0 = by0.min(y);
                bx1 = bx1.max(x);
                by1 = by1.max(y);
            }
        }
        let fillp = st.fill.clone();
        if !matches!(fillp, Paint::None) {
            let mut r = Raster::new(self.img_w, self.img_h);
            for p in &polys {
                for i in 0..p.len() {
                    let a = clampp(p[i], self.img_w, self.img_h);
                    let b = clampp(p[(i + 1) % p.len()], self.img_w, self.img_h);
                    r.line(a, b);
                }
            }
            let cov = if st.evenodd { r.coverage_evenodd() } else { r.coverage() };
            self.composite(&cov, &fillp, st.opacity * st.fill_op, m, (bx0, by0, bx1, by1));
        }
        if !matches!(st.stroke, Paint::None) && st.sw > 0.0 {
            let hw = (st.sw * scale / 2.0).max(0.4);
            let mut cov = vec![0f32; self.img_w * self.img_h];
            for p in &polys {
                let n = p.len();
                for i in 0..n.saturating_sub(1).max(if n == 2 { 1 } else { 0 }) {
                    stroke_seg(&mut cov, self.img_w, self.img_h, p[i], p[(i + 1) % n], hw);
                }
                if n > 2 && closed_like(&subs[0]) {
                    stroke_seg(&mut cov, self.img_w, self.img_h, p[n - 1], p[0], hw);
                }
            }
            self.composite(&cov, &st.stroke.clone(), st.opacity * st.stroke_op, m, (bx0, by0, bx1, by1));
        }
    }

    fn composite(&mut self, cov: &[f32], paint: &Paint, op: f32, m: &M, bbox: (f32, f32, f32, f32)) {
        let g = match paint {
            Paint::Grad(id) => self.grad_fn(id),
            _ => None,
        };
        let solid = match paint {
            Paint::Solid(c) => Some(*c),
            Paint::Grad(_) if g.is_none() => Some(0xFF808080),
            _ => None,
        };
        let im = inv(m);
        for y in 0..self.img_h {
            for x in 0..self.img_w {
                let c = cov[y * self.img_w + x];
                if c <= 0.003 {
                    continue;
                }
                let col = if let Some(s) = solid {
                    s
                } else if let (Some(g), Some(im)) = (&g, &im) {
                    let (ux, uy) = ap(im, x as f32 + 0.5, y as f32 + 0.5);
                    g.color_at(ux, uy, bbox)
                } else {
                    0xFF808080
                };
                let col = match self.tint {
                    Some(t) => (t & 0xFFFFFF) | (col & 0xFF000000),
                    None => col,
                };
                let a = (col >> 24) as f32 / 255.0 * c.min(1.0) * op;
                let (r, gg, b) = (((col >> 16) & 255) as f32 / 255.0, ((col >> 8) & 255) as f32 / 255.0, (col & 255) as f32 / 255.0);
                let i = (y * self.img_w + x) * 4;
                let ia = 1.0 - a;
                self.acc[i] = r * a + self.acc[i] * ia;
                self.acc[i + 1] = gg * a + self.acc[i + 1] * ia;
                self.acc[i + 2] = b * a + self.acc[i + 2] * ia;
                self.acc[i + 3] = a + self.acc[i + 3] * ia;
            }
        }
    }
}

struct GradInfo {
    stops: Vec<(f32, u32)>,
    units_user: bool,
    gt: M,
    radial: bool,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    r: f32,
}
impl GradInfo {
    fn color_at(&self, ux: f32, uy: f32, bb: (f32, f32, f32, f32)) -> u32 {
        let (mut x, mut y) = (ux, uy);
        if !self.units_user {
            let (w, h) = ((bb.2 - bb.0).max(1e-6), (bb.3 - bb.1).max(1e-6));
            x = (x - bb.0) / w;
            y = (y - bb.1) / h;
        }
        if let Some(gi) = inv(&self.gt) {
            let p = ap(&gi, x, y);
            x = p.0;
            y = p.1;
        }
        let t = if self.radial {
            let d = ((x - self.x1).powi(2) + (y - self.y1).powi(2)).sqrt();
            d / self.r.max(1e-6)
        } else {
            let (vx, vy) = (self.x2 - self.x1, self.y2 - self.y1);
            let l2 = (vx * vx + vy * vy).max(1e-9);
            ((x - self.x1) * vx + (y - self.y1) * vy) / l2
        }
        .clamp(0.0, 1.0);
        let mut prev = self.stops[0];
        if t <= prev.0 {
            return prev.1;
        }
        for &s in &self.stops[1..] {
            if t <= s.0 {
                let f = if s.0 > prev.0 { (t - prev.0) / (s.0 - prev.0) } else { 1.0 };
                return crate::gfx::mix(prev.1, s.1, f);
            }
            prev = s;
        }
        prev.1
    }
}

fn clampp(p: (f32, f32), w: usize, h: usize) -> (f32, f32) {
    (p.0.clamp(0.0, w as f32), p.1.clamp(0.0, h as f32))
}
fn closed_like(s: &[(f32, f32)]) -> bool {
    s.len() > 2
}

fn stroke_seg(cov: &mut [f32], w: usize, h: usize, a: (f32, f32), b: (f32, f32), hw: f32) {
    let x0 = (a.0.min(b.0) - hw - 1.0).floor().max(0.0) as usize;
    let x1 = ((a.0.max(b.0) + hw + 1.0).ceil() as usize).min(w);
    let y0 = (a.1.min(b.1) - hw - 1.0).floor().max(0.0) as usize;
    let y1 = ((a.1.max(b.1) + hw + 1.0).ceil() as usize).min(h);
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let l2 = vx * vx + vy * vy;
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let t = if l2 < 1e-6 { 0.0 } else { (((px - a.0) * vx + (py - a.1) * vy) / l2).clamp(0.0, 1.0) };
            let d = ((px - a.0 - vx * t).powi(2) + (py - a.1 - vy * t).powi(2)).sqrt() - hw;
            let c = (0.5 - d).clamp(0.0, 1.0);
            let i = y * w + x;
            if c > cov[i] {
                cov[i] = c;
            }
        }
    }
}

fn flatten_cubic(out: &mut Vec<(f32, f32)>, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) {
    let n = 14;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        out.push((
            u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
            u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
        ));
    }
}

fn arc_to(out: &mut Vec<(f32, f32)>, p0: (f32, f32), rx: f32, ry: f32, rot: f32, large: bool, sweep: bool, p1: (f32, f32)) {
    if p0 == p1 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 {
        out.push(p1);
        return;
    }
    let phi = rot.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx = (p0.0 - p1.0) / 2.0;
    let dy = (p0.1 - p1.1) / 2.0;
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
    let co = if den.abs() < 1e-9 { 0.0 } else { (num / den).max(0.0).sqrt() } * if large == sweep { -1.0 } else { 1.0 };
    let cxp = co * rx * y1p / ry;
    let cyp = -co * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (p0.0 + p1.0) / 2.0;
    let cy = sp * cxp + cp * cyp + (p0.1 + p1.1) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let th1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dth = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dth > 0.0 {
        dth -= std::f32::consts::TAU;
    } else if sweep && dth < 0.0 {
        dth += std::f32::consts::TAU;
    }
    let n = ((dth.abs() * rx.max(ry)).ceil() as usize).clamp(6, 48);
    for i in 1..=n {
        let t = th1 + dth * i as f32 / n as f32;
        let (x, y) = (rx * t.cos(), ry * t.sin());
        out.push((cp * x - sp * y + cx, sp * x + cp * y + cy));
    }
}

fn parse_path(d: &str) -> Vec<Vec<(f32, f32)>> {
    let mut subs: Vec<Vec<(f32, f32)>> = vec![];
    let mut cur: Vec<(f32, f32)> = vec![];
    let toks = tokenize_path(d);
    let (mut x, mut y) = (0f32, 0f32);
    let (mut sx, mut sy) = (0f32, 0f32);
    let mut last_c: Option<(f32, f32)> = None; // last cubic ctrl
    let mut last_q: Option<(f32, f32)> = None;
    let mut i = 0;
    let mut cmd = ' ';
    let take = |i: &mut usize, n: usize| -> Option<Vec<f32>> {
        let mut v = vec![];
        for _ in 0..n {
            match toks.get(*i) {
                Some(Tok::N(x)) => {
                    v.push(*x);
                    *i += 1;
                }
                _ => return None,
            }
        }
        Some(v)
    };
    while i < toks.len() {
        if let Tok::C(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'z' || c == 'Z' {
                if !cur.is_empty() {
                    subs.push(std::mem::take(&mut cur));
                }
                x = sx;
                y = sy;
                cur.push((x, y));
                last_c = None;
                last_q = None;
                continue;
            }
        } else if cmd == ' ' {
            i += 1;
            continue;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let v = match take(&mut i, 2) {
                    Some(v) => v,
                    None => break,
                };
                if cur.len() > 1 {
                    subs.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                x = ox + v[0];
                y = oy + v[1];
                sx = x;
                sy = y;
                cur.push((x, y));
                cmd = if rel { 'l' } else { 'L' };
                last_c = None;
                last_q = None;
            }
            'L' => {
                let v = match take(&mut i, 2) {
                    Some(v) => v,
                    None => break,
                };
                x = ox + v[0];
                y = oy + v[1];
                cur.push((x, y));
                last_c = None;
                last_q = None;
            }
            'H' => {
                let v = match take(&mut i, 1) {
                    Some(v) => v,
                    None => break,
                };
                x = ox + v[0];
                cur.push((x, y));
                last_c = None;
                last_q = None;
            }
            'V' => {
                let v = match take(&mut i, 1) {
                    Some(v) => v,
                    None => break,
                };
                y = oy + v[0];
                cur.push((x, y));
                last_c = None;
                last_q = None;
            }
            'C' => {
                let v = match take(&mut i, 6) {
                    Some(v) => v,
                    None => break,
                };
                let (c1, c2, p) = ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]), (ox + v[4], oy + v[5]));
                flatten_cubic(&mut cur, (x, y), c1, c2, p);
                x = p.0;
                y = p.1;
                last_c = Some(c2);
                last_q = None;
            }
            'S' => {
                let v = match take(&mut i, 4) {
                    Some(v) => v,
                    None => break,
                };
                let c1 = last_c.map_or((x, y), |c| (2.0 * x - c.0, 2.0 * y - c.1));
                let (c2, p) = ((ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]));
                flatten_cubic(&mut cur, (x, y), c1, c2, p);
                x = p.0;
                y = p.1;
                last_c = Some(c2);
                last_q = None;
            }
            'Q' | 'T' => {
                let (c, p);
                if cmd.to_ascii_uppercase() == 'Q' {
                    let v = match take(&mut i, 4) {
                        Some(v) => v,
                        None => break,
                    };
                    c = (ox + v[0], oy + v[1]);
                    p = (ox + v[2], oy + v[3]);
                } else {
                    let v = match take(&mut i, 2) {
                        Some(v) => v,
                        None => break,
                    };
                    c = last_q.map_or((x, y), |q| (2.0 * x - q.0, 2.0 * y - q.1));
                    p = (ox + v[0], oy + v[1]);
                }
                let c1 = (x + 2.0 / 3.0 * (c.0 - x), y + 2.0 / 3.0 * (c.1 - y));
                let c2 = (p.0 + 2.0 / 3.0 * (c.0 - p.0), p.1 + 2.0 / 3.0 * (c.1 - p.1));
                flatten_cubic(&mut cur, (x, y), c1, c2, p);
                x = p.0;
                y = p.1;
                last_q = Some(c);
                last_c = None;
            }
            'A' => {
                let v = match take(&mut i, 7) {
                    Some(v) => v,
                    None => break,
                };
                let p = (ox + v[5], oy + v[6]);
                arc_to(&mut cur, (x, y), v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0, p);
                x = p.0;
                y = p.1;
                last_c = None;
                last_q = None;
            }
            _ => {
                i += 1;
            }
        }
        if cur.len() > 200_000 {
            break;
        }
    }
    if cur.len() > 1 {
        subs.push(cur);
    }
    subs
}

enum Tok {
    C(char),
    N(f32),
}
fn tokenize_path(d: &str) -> Vec<Tok> {
    let mut out = vec![];
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<Tok>| {
        if !cur.is_empty() {
            if let Ok(v) = cur.parse::<f32>() {
                out.push(Tok::N(v));
            }
            cur.clear();
        }
    };
    let mut prev = ' ';
    for c in d.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            flush(&mut cur, &mut out);
            out.push(Tok::C(c));
        } else if c == ',' || c.is_whitespace() {
            flush(&mut cur, &mut out);
        } else if (c == '-' || c == '+') && !cur.is_empty() && prev != 'e' && prev != 'E' {
            flush(&mut cur, &mut out);
            cur.push(c);
        } else if c == '.' && cur.contains('.') {
            flush(&mut cur, &mut out);
            cur.push(c);
        } else {
            cur.push(c);
        }
        prev = c;
    }
    flush(&mut cur, &mut out);
    out
}

fn shape_to_paths(n: &Node) -> Vec<Vec<(f32, f32)>> {
    let f = |k: &str| n.attrs.get(k).map_or(0.0, |v| len(v));
    match n.tag.as_str() {
        "path" => n.attrs.get("d").map(|d| parse_path(d)).unwrap_or_default(),
        "rect" => {
            let (x, y, w, h) = (f("x"), f("y"), f("width"), f("height"));
            if w <= 0.0 || h <= 0.0 {
                return vec![];
            }
            let mut rx = f("rx");
            let mut ry = f("ry");
            if rx == 0.0 {
                rx = ry;
            }
            if ry == 0.0 {
                ry = rx;
            }
            rx = rx.min(w / 2.0);
            ry = ry.min(h / 2.0);
            if rx <= 0.0 {
                return vec![vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]];
            }
            let mut p = vec![];
            let seg = 8;
            let corner = |p: &mut Vec<(f32, f32)>, cx: f32, cy: f32, a0: f32| {
                for i in 0..=seg {
                    let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / seg as f32;
                    p.push((cx + rx * a.cos(), cy + ry * a.sin()));
                }
            };
            corner(&mut p, x + w - rx, y + ry, -std::f32::consts::FRAC_PI_2);
            corner(&mut p, x + w - rx, y + h - ry, 0.0);
            corner(&mut p, x + rx, y + h - ry, std::f32::consts::FRAC_PI_2);
            corner(&mut p, x + rx, y + ry, std::f32::consts::PI);
            vec![p]
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (f("cx"), f("cy"));
            let (rx, ry) = if n.tag == "circle" { (f("r"), f("r")) } else { (f("rx"), f("ry")) };
            if rx <= 0.0 || ry <= 0.0 {
                return vec![];
            }
            let steps = ((rx.max(ry) * 2.0) as usize).clamp(24, 120);
            vec![(0..steps).map(|i| {
                let a = std::f32::consts::TAU * i as f32 / steps as f32;
                (cx + rx * a.cos(), cy + ry * a.sin())
            }).collect()]
        }
        "line" => vec![vec![(f("x1"), f("y1")), (f("x2"), f("y2"))]],
        "polyline" | "polygon" => {
            let v = nums(n.attrs.get("points").map(|s| s.as_str()).unwrap_or(""));
            let pts: Vec<(f32, f32)> = v.chunks_exact(2).map(|c| (c[0], c[1])).collect();
            if pts.len() < 2 { vec![] } else { vec![pts] }
        }
        _ => vec![],
    }
}

/// Render SVG text to an RGBA image of size `size` x `size` (aspect preserved, centered).
/// `tint`: recolor everything (used for symbolic icons).
pub fn render(svg: &str, size: usize, tint: Option<u32>) -> Option<Image> {
    if svg.len() > 4_000_000 || size == 0 || size > 1024 {
        return None;
    }
    let root = parse_xml(svg)?;
    let (mut vx, mut vy, mut vw, mut vh) = (0.0, 0.0, 0.0, 0.0);
    if let Some(vb) = root.attrs.get("viewBox") {
        let a = nums(vb);
        if a.len() == 4 {
            vx = a[0];
            vy = a[1];
            vw = a[2];
            vh = a[3];
        }
    }
    if vw <= 0.0 || vh <= 0.0 {
        vw = root.attrs.get("width").map_or(0.0, |v| len(v));
        vh = root.attrs.get("height").map_or(0.0, |v| len(v));
    }
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }
    let s = (size as f32 / vw).min(size as f32 / vh);
    let ox = (size as f32 - vw * s) / 2.0 - vx * s;
    let oy = (size as f32 - vh * s) / 2.0 - vy * s;
    let m: M = [s, 0.0, 0.0, s, ox, oy];
    let mut cx2 = Ctx { defs: HashMap::new(), img_w: size, img_h: size, acc: vec![0.0; size * size * 4], tint, depth: 0, budget: 4000 };
    cx2.collect_defs(&root);
    let st = Style { fill: Paint::Solid(0xFF000000), stroke: Paint::None, sw: 1.0, opacity: 1.0, fill_op: 1.0, stroke_op: 1.0, color: tint.unwrap_or(0xFF000000), evenodd: false };
    cx2.walk(&root, &m, &st);
    let mut px = vec![0u32; size * size];
    for i in 0..size * size {
        let a = cx2.acc[i * 4 + 3];
        if a > 0.002 {
            let r = (cx2.acc[i * 4] / a * 255.0).clamp(0.0, 255.0) as u32;
            let g = (cx2.acc[i * 4 + 1] / a * 255.0).clamp(0.0, 255.0) as u32;
            let b = (cx2.acc[i * 4 + 2] / a * 255.0).clamp(0.0, 255.0) as u32;
            px[i] = (((a * 255.0) as u32).min(255) << 24) | (r << 16) | (g << 8) | b;
        }
    }
    Some(Image { w: size, h: size, px })
}
