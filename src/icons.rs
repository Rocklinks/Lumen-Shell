//! freedesktop icon-theme resolver (index.theme, Inherits, size matching) with PNG and SVG loading + caches.
use crate::gfx::Image;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone, Debug)]
struct Dir {
    path: String,
    size: i32,
    min: i32,
    max: i32,
    thresh: i32,
    kind: char, // F fixed, S scalable, T threshold
    scale: i32,
}

#[derive(Clone, Debug)]
struct ThemeIdx {
    name: String,
    base: PathBuf,
    dirs: Vec<Dir>,
    inherits: Vec<String>,
}

pub struct Icons {
    bases: Vec<PathBuf>,
    themes: HashMap<String, Vec<ThemeIdx>>,
    chain: Vec<String>,
    cache: HashMap<(String, u32, bool), Option<Rc<Image>>>,
    path_cache: HashMap<(String, u32), Option<PathBuf>>,
    fit_cache: HashMap<(String, u32, u32), Option<Rc<Image>>>,
    pub extra_dirs: Vec<PathBuf>,
    pub theme_name: String,
}

fn parse_theme(base: &Path, name: &str) -> Option<ThemeIdx> {
    let txt = std::fs::read_to_string(base.join(name).join("index.theme")).ok()?;
    let mut sections: Vec<(String, HashMap<String, String>)> = vec![];
    for l in txt.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            sections.push((l.trim_matches(|c| c == '[' || c == ']').to_string(), HashMap::new()));
        } else if let Some((k, v)) = l.split_once('=') {
            if let Some(s) = sections.last_mut() {
                s.1.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    let head = sections.iter().find(|s| s.0 == "Icon Theme")?.1.clone();
    let mut dirs_list: Vec<String> = head.get("Directories").map(|d| d.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    if let Some(sd) = head.get("ScaledDirectories") {
        dirs_list.extend(sd.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    let mut dirs = vec![];
    for d in dirs_list {
        if let Some(sec) = sections.iter().find(|s| s.0 == d) {
            let g = |k: &str| sec.1.get(k).and_then(|v| v.parse::<i32>().ok());
            let size = g("Size").unwrap_or(0);
            if size == 0 {
                continue;
            }
            let kind = match sec.1.get("Type").map(|s| s.as_str()) {
                Some("Fixed") => 'F',
                Some("Scalable") => 'S',
                _ => 'T',
            };
            dirs.push(Dir { path: d.clone(), size, min: g("MinSize").unwrap_or(size), max: g("MaxSize").unwrap_or(size), thresh: g("Threshold").unwrap_or(2), kind, scale: g("Scale").unwrap_or(1) });
        }
    }
    let inherits = head.get("Inherits").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    Some(ThemeIdx { name: name.to_string(), base: base.join(name), dirs, inherits })
}

fn dist(d: &Dir, want: i32) -> i32 {
    let s = d.size * d.scale;
    match d.kind {
        'F' => (s - want).abs(),
        'S' => {
            if want < d.min * d.scale {
                d.min * d.scale - want
            } else if want > d.max * d.scale {
                want - d.max * d.scale
            } else {
                0
            }
        }
        _ => {
            if want < (d.size - d.thresh) * d.scale {
                (d.size - d.thresh) * d.scale - want
            } else if want > (d.size + d.thresh) * d.scale {
                want - (d.size + d.thresh) * d.scale
            } else {
                0
            }
        }
    }
}

impl Icons {
    pub fn new(theme: &str) -> Icons {
        let mut bases = vec![];
        if let Ok(h) = std::env::var("HOME") {
            bases.push(PathBuf::from(&h).join(".icons"));
            bases.push(PathBuf::from(&h).join(".local/share/icons"));
        }
        for d in crate::desktop::data_dirs().into_iter().skip(1) {
            bases.push(d.join("icons"));
        }
        bases.push(PathBuf::from("/usr/share/icons"));
        bases.dedup();
        let mut ic = Icons { bases, themes: HashMap::new(), chain: vec![], cache: HashMap::new(), path_cache: HashMap::new(), fit_cache: HashMap::new(), extra_dirs: vec![], theme_name: theme.to_string() };
        ic.build_chain(theme);
        ic
    }

    fn load_theme(&mut self, name: &str) {
        if self.themes.contains_key(name) {
            return;
        }
        let mut v = vec![];
        for b in &self.bases {
            if let Some(t) = parse_theme(b, name) {
                v.push(t);
            }
        }
        self.themes.insert(name.to_string(), v);
    }

    fn build_chain(&mut self, theme: &str) {
        let mut chain: Vec<String> = vec![];
        let mut queue = vec![theme.to_string()];
        while let Some(t) = queue.pop() {
            if chain.contains(&t) || chain.len() > 16 {
                continue;
            }
            self.load_theme(&t);
            chain.push(t.clone());
            let mut inh: Vec<String> = self.themes.get(&t).map(|v| v.iter().flat_map(|x| x.inherits.clone()).collect()).unwrap_or_default();
            inh.reverse();
            queue.extend(inh);
        }
        for fallback in ["Adwaita", "gnome", "hicolor"] {
            if !chain.iter().any(|c| c == fallback) {
                self.load_theme(fallback);
                chain.push(fallback.to_string());
            }
        }
        self.chain = chain;
    }

    pub fn add_dir(&mut self, d: &str) {
        let p = PathBuf::from(d);
        if !d.is_empty() && !self.extra_dirs.contains(&p) && self.extra_dirs.len() < 64 {
            self.extra_dirs.push(p);
            self.path_cache.clear();
            self.cache.clear();
            self.fit_cache.clear();
        }
    }

    pub fn set_theme(&mut self, theme: &str) {
        if theme != self.theme_name {
            self.theme_name = theme.to_string();
            self.cache.clear();
            self.path_cache.clear();
            self.fit_cache.clear();
            self.build_chain(theme);
        }
    }

    fn find_path(&mut self, name: &str, size: i32) -> Option<PathBuf> {
        let key = (name.to_string(), size as u32);
        if let Some(c) = self.path_cache.get(&key) {
            return c.clone();
        }
        let r = self.find_path_uncached(name, size);
        if self.path_cache.len() > 2000 {
            self.path_cache.clear();
        }
        self.path_cache.insert(key, r.clone());
        r
    }

    fn find_path_uncached(&self, name: &str, size: i32) -> Option<PathBuf> {
        if name.starts_with('/') {
            return if Path::new(name).exists() { Some(PathBuf::from(name)) } else { None };
        }
        for d in &self.extra_dirs {
            for ext in ["svg", "png"] {
                let p = d.join(format!("{}.{}", name, ext));
                if p.exists() {
                    return Some(p);
                }
            }
        }
        for t in &self.chain {
            if let Some(idxs) = self.themes.get(t) {
                let mut best: Option<(i32, PathBuf)> = None;
                for idx in idxs {
                    for d in &idx.dirs {
                        let dd = dist(d, size);
                        if let Some((bd, _)) = &best {
                            if dd >= *bd {
                                continue;
                            }
                        }
                        for ext in ["svg", "png"] {
                            let p = idx.base.join(&d.path).join(format!("{}.{}", name, ext));
                            if p.exists() {
                                let tie = if ext == "svg" { dd } else { dd };
                                best = Some((tie, p));
                                break;
                            }
                        }
                    }
                }
                if let Some((_, p)) = best {
                    return Some(p);
                }
            }
        }
        for ext in ["png", "svg", "xpm"] {
            let p = PathBuf::from("/usr/share/pixmaps").join(format!("{}.{}", name, ext));
            if p.exists() && ext != "xpm" {
                return Some(p);
            }
        }
        None
    }

    /// Load an icon scaled to `size`. `tint`: recolor symbolic SVGs.
    pub fn get(&mut self, name: &str, size: u32, tint: Option<u32>) -> Option<Rc<Image>> {
        if name.is_empty() {
            return None;
        }
        let key = (format!("{}{}", name, tint.map_or(String::new(), |t| format!("#{:x}", t))), size, tint.is_some());
        if let Some(c) = self.cache.get(&key) {
            return c.clone();
        }
        let img = self.load(name, size, tint).filter(|i| i.px.iter().any(|p| p >> 24 > 8)).map(Rc::new);
        if self.cache.len() > 600 {
            self.cache.clear();
        }
        self.cache.insert(key, img.clone());
        img
    }

    /// Symbolic icon cropped to its visible bounds (so different glyphs can be sized optically equal).
    pub fn get_fit(&mut self, name: &str, size: u32, tint: u32) -> Option<Rc<Image>> {
        let key = (name.to_string(), size, tint);
        if let Some(c) = self.fit_cache.get(&key) {
            return c.clone();
        }
        let r = self.get(name, size, Some(tint)).and_then(|img| {
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
            for y in 0..img.h {
                for x in 0..img.w {
                    if img.px[y * img.w + x] >> 24 > 24 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
            }
            if x0 == usize::MAX {
                return None;
            }
            let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
            let mut px = Vec::with_capacity(w * h);
            for y in y0..=y1 {
                px.extend_from_slice(&img.px[y * img.w + x0..y * img.w + x1 + 1]);
            }
            Some(Rc::new(Image { w, h, px }))
        });
        if self.fit_cache.len() > 600 {
            self.fit_cache.clear();
        }
        self.fit_cache.insert(key, r.clone());
        r
    }

    fn load(&mut self, name: &str, size: u32, tint: Option<u32>) -> Option<Image> {
        let p = self.find_path(name, size as i32)?;
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "png" => {
                let d = std::fs::read(&p).ok()?;
                crate::png::decode_png(&d)
            }
            "svg" => {
                let t = std::fs::read_to_string(&p).ok()?;
                let is_sym = name.ends_with("-symbolic") || tint.is_some();
                crate::svg::render(&t, size as usize, if is_sym { tint } else { None })
            }
            _ => None,
        }
    }
}
