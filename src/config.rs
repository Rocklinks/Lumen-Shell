//! INI-style config with tolerant parsing (bad/missing values fall back to defaults; never panics).
use crate::platform::Edge;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Visibility {
    Always,
    AutoHide,
    /// Visible, but hidden while a fullscreen window is active.
    HideInFullscreen,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub theme: ThemeMode,
    pub scheme: String,
    pub accent: String,
    pub ui_scale: f32,
    pub bar_follow_theme: bool,
    pub battery_percent: bool,
    pub notify_banners: bool,
    pub manage_panel: bool,
    pub shot_dir: String,
    pub video_dir: String,
    pub record_fps: i32,
    pub record_audio: bool,
    pub sc: [String; 5],
    pub notify_takeover: bool,
    pub caffeine_fullscreen: bool,
    // bar
    pub bar_enabled: bool,
    pub bar_edge: Edge,
    pub bar_height: i32,
    pub bar_opacity: f32,
    pub bar_floating: bool,
    pub bar_vis: Visibility,
    pub bar_over_fullscreen: bool,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    // clock
    pub clock_24h: bool,
    pub clock_seconds: bool,
    pub clock_date: bool,
    // dock
    pub dock_enabled: bool,
    pub dock_edge: Edge,
    pub dock_icon: i32,
    pub dock_opacity: f32,
    pub dock_vis: Visibility,
    pub dock_over_fullscreen: bool,
    pub favorites: Vec<String>,
    // night light
    pub night_on: bool,
    pub night_temp: i32,
    // grid
    pub grid_order: Vec<String>,
    pub path: PathBuf,
    pub mtime: Option<std::time::SystemTime>,
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config"))).unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("lumen")
}

fn list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}
fn edge(s: &str, d: Edge) -> Edge {
    match s.trim().to_lowercase().as_str() {
        "top" => Edge::Top,
        "bottom" => Edge::Bottom,
        "left" => Edge::Left,
        "right" => Edge::Right,
        _ => d,
    }
}
pub fn edge_name(e: Edge) -> &'static str {
    match e {
        Edge::Top => "top",
        Edge::Bottom => "bottom",
        Edge::Left => "left",
        Edge::Right => "right",
    }
}
fn vis(s: &str, d: Visibility) -> Visibility {
    match s.trim().to_lowercase().as_str() {
        "always" => Visibility::Always,
        "auto-hide" | "autohide" => Visibility::AutoHide,
        "hide-in-fullscreen" => Visibility::HideInFullscreen,
        _ => d,
    }
}
pub fn vis_name(v: Visibility) -> &'static str {
    match v {
        Visibility::Always => "always",
        Visibility::AutoHide => "auto-hide",
        Visibility::HideInFullscreen => "hide-in-fullscreen",
    }
}
fn fl(s: Option<&str>) -> Option<f32> {
    s.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
}
fn bl(s: &str, d: bool) -> bool {
    match s.trim().to_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => true,
        "false" | "no" | "0" | "off" => false,
        _ => d,
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: ThemeMode::Auto,
            scheme: "system".into(),
            accent: "auto".into(),
            ui_scale: 1.0,
            bar_follow_theme: true,
            battery_percent: true,
            notify_banners: true,
            manage_panel: true,
            shot_dir: "~/Screenshots".into(),
            video_dir: "~/Videos".into(),
            record_fps: 30,
            record_audio: false,
            sc: ["Print".into(), "Shift+Print".into(), "Alt+Print".into(), "Ctrl+Shift+Alt+R".into(), "Alt+F1".into()],
            notify_takeover: true,
            caffeine_fullscreen: false,
            bar_enabled: true,
            bar_edge: Edge::Top,
            bar_height: 32,
            bar_opacity: 1.0,
            bar_floating: false,
            bar_vis: Visibility::Always,
            bar_over_fullscreen: true,
            left: vec!["workspaces".into()],
            center: vec!["clock".into()],
            right: vec!["tray".into(), "status".into()],
            clock_24h: true,
            clock_seconds: false,
            clock_date: true,
            dock_enabled: true,
            dock_edge: Edge::Bottom,
            dock_icon: 48,
            dock_opacity: 0.92,
            dock_vis: Visibility::Always,
            dock_over_fullscreen: true,
            favorites: vec!["firefox".into(), "org.gnome.Nautilus".into(), "thunar".into(), "xfce4-terminal".into(), "org.gnome.TextEditor".into(), "org.gnome.Settings".into()],
            night_on: false,
            night_temp: 4500,
            grid_order: vec![],
            path: config_dir().join("config.ini"),
            mtime: None,
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let mut c = Config::default();
        let txt = match std::fs::read_to_string(&c.path) {
            Ok(t) => t,
            Err(_) => {
                c.save();
                c.mtime = std::fs::metadata(&c.path).and_then(|m| m.modified()).ok();
                return c;
            }
        };
        c.mtime = std::fs::metadata(&c.path).and_then(|m| m.modified()).ok();
        let mut kv: HashMap<String, String> = HashMap::new();
        let mut section = String::new();
        for line in txt.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
                continue;
            }
            if l.starts_with('[') && l.ends_with(']') {
                section = l[1..l.len() - 1].trim().to_lowercase();
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                kv.insert(format!("{}.{}", section, k.trim().to_lowercase()), v.trim().to_string());
            }
        }
        let g = |k: &str| kv.get(k).map(|s| s.as_str());
        if let Some(v) = g("general.theme") {
            c.theme = match v.to_lowercase().as_str() {
                "light" => ThemeMode::Light,
                "dark" => ThemeMode::Dark,
                _ => ThemeMode::Auto,
            };
        }
        if let Some(v) = g("general.scheme") {
            c.scheme = v.trim().to_lowercase();
        }
        if let Some(v) = g("general.accent") {
            c.accent = v.trim().to_lowercase();
        }
        if let Some(v) = g("bar.follow_theme") {
            c.bar_follow_theme = bl(v, c.bar_follow_theme);
        }
        if let Some(v) = g("screenshots.dir") {
            c.shot_dir = v.to_string();
        }
        if let Some(v) = g("screenshots.video_dir") {
            c.video_dir = v.to_string();
        }
        if let Some(v) = g("screenshots.fps").and_then(|v| v.parse::<i32>().ok()) {
            c.record_fps = v.clamp(5, 120);
        }
        if let Some(v) = g("screenshots.audio") {
            c.record_audio = bl(v, c.record_audio);
        }
        for (i, k) in ["shortcuts.screenshot", "shortcuts.screen", "shortcuts.area", "shortcuts.record", "shortcuts.grid"].iter().enumerate() {
            if let Some(v) = g(k) {
                c.sc[i] = v.to_string();
            }
        }
        if let Some(v) = g("general.manage_panel") {
            c.manage_panel = bl(v, c.manage_panel);
        }
        if let Some(v) = g("notifications.banners") {
            c.notify_banners = bl(v, c.notify_banners);
        }
        if let Some(v) = g("notifications.takeover") {
            c.notify_takeover = bl(v, c.notify_takeover);
        }
        if let Some(v) = g("power.caffeine_fullscreen") {
            c.caffeine_fullscreen = bl(v, c.caffeine_fullscreen);
        }
        if let Some(v) = g("bar.battery_percent") {
            c.battery_percent = bl(v, c.battery_percent);
        }
        if let Some(v) = fl(g("general.ui_scale")) {
            c.ui_scale = v.clamp(0.5, 3.0);
        }
        if let Some(v) = g("bar.enabled") {
            c.bar_enabled = bl(v, c.bar_enabled);
        }
        if let Some(v) = g("bar.position") {
            c.bar_edge = edge(v, c.bar_edge);
        }
        if let Some(v) = g("bar.height").and_then(|v| v.parse::<i32>().ok()) {
            c.bar_height = v.clamp(20, 96);
        }
        if let Some(v) = fl(g("bar.opacity")) {
            c.bar_opacity = v.clamp(0.0, 1.0);
        }
        if let Some(v) = g("bar.floating") {
            c.bar_floating = bl(v, c.bar_floating);
        }
        if let Some(v) = g("bar.visibility") {
            c.bar_vis = vis(v, c.bar_vis);
        }
        if let Some(v) = g("bar.over_fullscreen") {
            c.bar_over_fullscreen = bl(v, c.bar_over_fullscreen);
        }
        if let Some(v) = g("bar.left") {
            c.left = list(v);
        }
        if let Some(v) = g("bar.center") {
            c.center = list(v);
        }
        if let Some(v) = g("bar.right") {
            c.right = list(v);
        }
        if let Some(v) = g("clock.24h") {
            c.clock_24h = bl(v, c.clock_24h);
        }
        if let Some(v) = g("clock.seconds") {
            c.clock_seconds = bl(v, c.clock_seconds);
        }
        if let Some(v) = g("clock.date") {
            c.clock_date = bl(v, c.clock_date);
        }
        if let Some(v) = g("dock.enabled") {
            c.dock_enabled = bl(v, c.dock_enabled);
        }
        if let Some(v) = g("dock.position") {
            c.dock_edge = edge(v, c.dock_edge);
        }
        if let Some(v) = g("dock.icon_size").and_then(|v| v.parse::<i32>().ok()) {
            c.dock_icon = v.clamp(24, 96);
        }
        if let Some(v) = fl(g("dock.opacity")) {
            c.dock_opacity = v.clamp(0.0, 1.0);
        }
        if let Some(v) = g("dock.visibility") {
            c.dock_vis = vis(v, c.dock_vis);
        }
        if let Some(v) = g("dock.over_fullscreen") {
            c.dock_over_fullscreen = bl(v, c.dock_over_fullscreen);
        }
        if let Some(v) = g("dock.favorites") {
            c.favorites = list(v);
        }
        if let Some(v) = g("nightlight.enabled") {
            c.night_on = bl(v, c.night_on);
        }
        if let Some(v) = g("nightlight.temperature").and_then(|v| v.parse::<i32>().ok()) {
            c.night_temp = v.clamp(1700, 6500);
        }
        if let Some(v) = g("grid.order") {
            c.grid_order = list(v);
        }
        if g("general.version").and_then(|v| v.parse::<i32>().ok()).unwrap_or(1) < 2 {
            let d = Config::default();
            c.left = d.left;
            c.center = d.center;
            c.right = d.right;
            c.save();
            c.mtime = std::fs::metadata(&c.path).and_then(|m| m.modified()).ok();
        }
        c
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(self.path.parent().unwrap_or(std::path::Path::new("/tmp")));
        let t = match self.theme {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        };
        let s = format!(
            "# lumen-shell configuration (hot-reloaded)\n[general]\nversion = 2\nmanage_panel = {}\ntheme = {}\nscheme = {}\naccent = {}\nui_scale = {}\n\n[bar]\nfollow_theme = {}\nbattery_percent = {}\nenabled = {}\nposition = {}\nheight = {}\nopacity = {}\nfloating = {}\nvisibility = {}\nover_fullscreen = {}\nleft = {}\ncenter = {}\nright = {}\n\n[clock]\n24h = {}\nseconds = {}\ndate = {}\n\n[dock]\nenabled = {}\nposition = {}\nicon_size = {}\nopacity = {}\nvisibility = {}\nover_fullscreen = {}\nfavorites = {}\n\n[nightlight]\nenabled = {}\ntemperature = {}\n\n[notifications]\nbanners = {}\ntakeover = {}\n\n[power]\ncaffeine_fullscreen = {}\n\n[screenshots]\ndir = {}\nvideo_dir = {}\nfps = {}\naudio = {}\n\n[shortcuts]\nscreenshot = {}\nscreen = {}\narea = {}\nrecord = {}\ngrid = {}\n\n[grid]\norder = {}\n",
            self.manage_panel, t, self.scheme, self.accent, self.ui_scale, self.bar_follow_theme, self.battery_percent, self.bar_enabled, edge_name(self.bar_edge), self.bar_height, self.bar_opacity, self.bar_floating, vis_name(self.bar_vis), self.bar_over_fullscreen,
            self.left.join(", "), self.center.join(", "), self.right.join(", "), self.clock_24h, self.clock_seconds, self.clock_date,
            self.dock_enabled, edge_name(self.dock_edge), self.dock_icon, self.dock_opacity, vis_name(self.dock_vis), self.dock_over_fullscreen, self.favorites.join(", "),
            self.night_on, self.night_temp, self.notify_banners, self.notify_takeover, self.caffeine_fullscreen, self.shot_dir, self.video_dir, self.record_fps, self.record_audio, self.sc[0], self.sc[1], self.sc[2], self.sc[3], self.sc[4], self.grid_order.join(", ")
        );
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }

    pub fn changed_on_disk(&self) -> bool {
        let m = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        m.is_some() && m != self.mtime
    }
}

pub fn expand_home(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(h) = std::env::var("HOME") {
            return std::path::PathBuf::from(h).join(rest);
        }
    }
    if p == "~" {
        if let Ok(h) = std::env::var("HOME") {
            return std::path::PathBuf::from(h);
        }
    }
    std::path::PathBuf::from(p)
}

/// "Ctrl+Shift+Print" -> (keysym, mods bits: 1 shift, 2 ctrl, 4 alt, 8 super)
pub fn parse_accel(a: &str) -> Option<(u32, u8)> {
    let mut mods = 0u8;
    let mut key = None;
    for part in a.split('+') {
        match part.trim().to_lowercase().as_str() {
            "shift" => mods |= 1,
            "ctrl" | "control" => mods |= 2,
            "alt" => mods |= 4,
            "super" | "win" | "meta" => mods |= 8,
            "" => {}
            k => key = Some(k.to_string()),
        }
    }
    let k = key?;
    let sym = match k.as_str() {
        "print" => 0xff61,
        "pause" => 0xff13,
        "escape" => 0xff1b,
        "space" => 0x20,
        "return" | "enter" => 0xff0d,
        "tab" => 0xff09,
        "super_l" => 0xffeb,
        f if f.starts_with('f') && f.len() <= 3 && f[1..].parse::<u32>().map_or(false, |n| (1..=12).contains(&n)) => 0xffbd + f[1..].parse::<u32>().unwrap(),
        c if c.chars().count() == 1 => c.chars().next().unwrap() as u32,
        _ => return None,
    };
    Some((sym, mods))
}

pub fn accel_name(sym: u32, mods: u8) -> Option<String> {
    let key = match sym {
        0xff61 => "Print".to_string(),
        0xff13 => "Pause".into(),
        0x20 => "Space".into(),
        0xff0d => "Return".into(),
        0xff09 => "Tab".into(),
        0xffbe..=0xffc9 => format!("F{}", sym - 0xffbd),
        0x21..=0x7e => (sym as u8 as char).to_ascii_uppercase().to_string(),
        _ => return None,
    };
    let mut parts = vec![];
    if mods & 2 != 0 {
        parts.push("Ctrl".to_string());
    }
    if mods & 8 != 0 {
        parts.push("Super".into());
    }
    if mods & 4 != 0 {
        parts.push("Alt".into());
    }
    if mods & 1 != 0 {
        parts.push("Shift".into());
    }
    parts.push(key);
    Some(parts.join("+"))
}
