//! Resolve palette, font and icon theme from the user's applied theme (xfconf xsettings, GTK settings.ini, env).
use crate::config::ThemeMode;
use crate::gfx::{luminance, mix, Color};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub panel_bg: Color,
    pub panel_fg: Color,
    pub popup_bg: Color,
    pub popup_fg: Color,
    pub dim_fg: Color,
    pub card: Color,
    pub hover: Color,
    pub border: Color,
    pub accent: Color,
    pub accent_fg: Color,
    pub mantle: Color,
    pub scheme: String,
    pub font_family: String,
    pub font_size: f32,
    pub gtk_theme: String,
    pub icon_theme: String,
}

pub struct Scheme {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: bool,
    pub base: u32,
    pub mantle: u32,
    pub text: u32,
    pub sub: u32,
    pub surface0: u32,
    pub surface1: u32,
    pub accent: u32,
    pub accent_fg: u32,
}

macro_rules! sch {
    ($id:expr, $n:expr, $d:expr, $b:expr, $m:expr, $t:expr, $s:expr, $s0:expr, $s1:expr, $a:expr, $af:expr) => {
        Scheme { id: $id, name: $n, dark: $d, base: 0xFF000000 | $b, mantle: 0xFF000000 | $m, text: 0xFF000000 | $t, sub: 0xFF000000 | $s, surface0: 0xFF000000 | $s0, surface1: 0xFF000000 | $s1, accent: 0xFF000000 | $a, accent_fg: 0xFF000000 | $af }
    };
}

/// Built-in color schemes (palettes approximated from the published Catppuccin/Nord/Dracula/Gruvbox/Tokyo Night
/// color specs and the COSMIC look).
pub const SCHEMES: [Scheme; 11] = [
    sch!("catppuccin-mocha", "Catppuccin Mocha", true, 0x1e1e2e, 0x181825, 0xcdd6f4, 0xa6adc8, 0x313244, 0x45475a, 0xcba6f7, 0x11111b),
    sch!("catppuccin-macchiato", "Catppuccin Macchiato", true, 0x24273a, 0x1e2030, 0xcad3f5, 0xa5adcb, 0x363a4f, 0x494d64, 0xc6a0f6, 0x181926),
    sch!("catppuccin-frappe", "Catppuccin Frappé", true, 0x303446, 0x292c3c, 0xc6d0f5, 0xa5adce, 0x414559, 0x51576d, 0xca9ee6, 0x232634),
    sch!("catppuccin-latte", "Catppuccin Latte", false, 0xeff1f5, 0xe6e9ef, 0x4c4f69, 0x6c6f85, 0xccd0da, 0xbcc0cc, 0x8839ef, 0xeff1f5),
    sch!("cosmic-dark", "COSMIC Dark", true, 0x1b1b1b, 0x101010, 0xe6e6e6, 0xa0a0a0, 0x2b2b2b, 0x3a3a3a, 0x63d0df, 0x062a30),
    sch!("cosmic-light", "COSMIC Light", false, 0xf2f2f2, 0xe4e4e4, 0x1b1b1b, 0x5a5a5a, 0xffffff, 0xd0d0d0, 0x00727f, 0xffffff),
    sch!("nord", "Nord", true, 0x2e3440, 0x272c36, 0xeceff4, 0xd8dee9, 0x3b4252, 0x434c5e, 0x88c0d0, 0x2e3440),
    sch!("dracula", "Dracula", true, 0x282a36, 0x21222c, 0xf8f8f2, 0xbfbfd0, 0x343746, 0x44475a, 0xbd93f9, 0x282a36),
    sch!("gruvbox-dark", "Gruvbox Dark", true, 0x282828, 0x1d2021, 0xebdbb2, 0xbdae93, 0x3c3836, 0x504945, 0xfabd2f, 0x282828),
    sch!("tokyo-night", "Tokyo Night", true, 0x1a1b26, 0x16161e, 0xc0caf5, 0xa9b1d6, 0x24283b, 0x2f334d, 0x7aa2f7, 0x16161e),
    sch!("rose-pine", "Rosé Pine", true, 0x191724, 0x14121f, 0xe0def4, 0x908caa, 0x26233a, 0x403d52, 0xc4a7e7, 0x191724),
];

pub const ACCENTS: [(&str, u32); 8] = [("blue", 0xFF3584E4), ("teal", 0xFF2190A4), ("green", 0xFF3A944A), ("yellow", 0xFFC88800), ("orange", 0xFFED5B00), ("red", 0xFFE62D42), ("pink", 0xFFD56199), ("purple", 0xFF9141AC)];

pub fn parse_accent(s: &str) -> Option<Color> {
    if let Some((_, c)) = ACCENTS.iter().find(|(n, _)| *n == s) {
        return Some(*c);
    }
    parse_color(s)
}

#[derive(Default, Clone, Debug)]
pub struct Raw {
    pub gtk_theme: String,
    pub icon_theme: String,
    pub font: String,
    pub prefer_dark: Option<bool>,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}

fn xml_val(xml: &str, name: &str) -> Option<String> {
    let key = format!("name=\"{}\"", name);
    let i = xml.find(&key)?;
    let rest = &xml[i..];
    let end = rest.find('>')?;
    let tag = &rest[..end];
    let v = tag.find("value=\"")? + 7;
    let e = tag[v..].find('"')?;
    Some(tag[v..v + e].to_string())
}

pub fn read_raw() -> Raw {
    let mut r = Raw::default();
    let cfg = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".config"));
    // GTK3 settings.ini
    for p in [cfg.join("gtk-3.0/settings.ini"), cfg.join("gtk-4.0/settings.ini")] {
        if let Ok(t) = std::fs::read_to_string(p) {
            for l in t.lines() {
                if let Some((k, v)) = l.split_once('=') {
                    let v = v.trim().trim_matches('"').to_string();
                    match k.trim() {
                        "gtk-theme-name" if r.gtk_theme.is_empty() => r.gtk_theme = v,
                        "gtk-icon-theme-name" if r.icon_theme.is_empty() => r.icon_theme = v,
                        "gtk-font-name" if r.font.is_empty() => r.font = v,
                        "gtk-application-prefer-dark-theme" if r.prefer_dark.is_none() => r.prefer_dark = Some(v == "1" || v == "true"),
                        _ => {}
                    }
                }
            }
        }
    }
    // xfconf xsettings overrides (what XFCE actually applies)
    if let Ok(x) = std::fs::read_to_string(cfg.join("xfce4/xfconf/xfce-perchannel-xml/xsettings.xml")) {
        if let Some(v) = xml_val(&x, "ThemeName") {
            r.gtk_theme = v;
        }
        if let Some(v) = xml_val(&x, "IconThemeName") {
            r.icon_theme = v;
        }
        if let Some(v) = xml_val(&x, "FontName") {
            r.font = v;
        }
    }
    if let Ok(t) = std::env::var("GTK_THEME") {
        if !t.is_empty() {
            r.prefer_dark = Some(t.ends_with(":dark") || t.to_lowercase().contains("dark"));
            r.gtk_theme = t.split(':').next().unwrap_or("").to_string();
        }
    }
    if r.icon_theme.is_empty() {
        r.icon_theme = "Adwaita".into();
    }
    r
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        let v = u32::from_str_radix(h, 16).ok()?;
        return match h.len() {
            6 => Some(0xFF000000 | v),
            3 => Some(0xFF000000 | ((v >> 8 & 15) * 17) << 16 | ((v >> 4 & 15) * 17) << 8 | (v & 15) * 17),
            _ => None,
        };
    }
    let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?.strip_suffix(')')?;
    let p: Vec<f32> = inner.split(',').filter_map(|x| x.trim().trim_end_matches('%').parse::<f32>().ok()).collect();
    if p.len() >= 3 {
        let a = p.get(3).copied().unwrap_or(1.0);
        return Some((((a * 255.0) as u32) << 24) | ((p[0] as u32).min(255) << 16) | ((p[1] as u32).min(255) << 8) | (p[2] as u32).min(255));
    }
    None
}

/// Collect `@define-color name value;` from a GTK theme's gtk.css (following simple relative @imports).
fn theme_colors(theme: &str, dark: bool) -> HashMap<String, Color> {
    let mut out = HashMap::new();
    if theme.is_empty() {
        return out;
    }
    let dirs = [home().join(".themes"), home().join(".local/share/themes"), PathBuf::from("/usr/share/themes"), PathBuf::from("/usr/local/share/themes")];
    for d in dirs {
        let base = d.join(theme).join("gtk-3.0");
        let mut files = vec![base.join(if dark { "gtk-dark.css" } else { "gtk.css" }), base.join("gtk.css")];
        files.dedup();
        for f in files {
            if let Ok(t) = std::fs::read_to_string(&f) {
                scan_defs(&t, &mut out);
                for l in t.lines() {
                    if let Some(i) = l.find("@import") {
                        let name: String = l[i + 7..].chars().filter(|c| !matches!(c, '"' | '\'' | ';' | '(' | ')' | ' ')).collect();
                        let name = name.trim_start_matches("url");
                        if name.ends_with(".css") && !name.contains("resource:") {
                            if let Ok(t2) = std::fs::read_to_string(base.join(name)) {
                                scan_defs(&t2, &mut out);
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    return out;
                }
            }
        }
    }
    out
}
fn scan_defs(t: &str, out: &mut HashMap<String, Color>) {
    for l in t.lines() {
        let l = l.trim();
        if let Some(rest) = l.strip_prefix("@define-color") {
            let mut it = rest.trim().splitn(2, char::is_whitespace);
            if let (Some(n), Some(v)) = (it.next(), it.next()) {
                if let Some(c) = parse_color(v.trim().trim_end_matches(';')) {
                    out.entry(n.to_string()).or_insert(c);
                }
            }
        }
    }
}

pub fn is_dark_name(n: &str) -> bool {
    let l = n.to_lowercase();
    l.contains("dark") || l.ends_with("-black") || l.contains("noir")
}

pub fn resolve(mode: ThemeMode, raw: &Raw, scale: f32, portal_dark: Option<bool>) -> Theme {
    resolve_full(mode, raw, scale, portal_dark, "system", "auto", true)
}

pub fn resolve_full(mode: ThemeMode, raw: &Raw, scale: f32, portal_dark: Option<bool>, scheme: &str, accent_s: &str, bar_follow: bool) -> Theme {
    let sch = SCHEMES.iter().find(|x| x.id == scheme);
    let dark = match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::Auto => portal_dark.or(raw.prefer_dark).unwrap_or_else(|| is_dark_name(&raw.gtk_theme)),
    };
    let named = theme_colors(&raw.gtk_theme, dark);
    let pick = |names: &[&str]| names.iter().find_map(|n| named.get(*n).copied());
    // GNOME palette defaults (libadwaita 1.x)
    let (mut pbg, mut pfg, mut cardc, mut brd, mut dim) = if dark {
        (0xFF242424u32, 0xFFFFFFFFu32, 0xFF303030u32, 0xFF3D3D3Du32, 0xFFB0B0B0u32)
    } else {
        (0xFFFAFAFAu32, 0xFF2E3436u32, 0xFFFFFFFFu32, 0xFFD5D5D5u32, 0xFF6A6A6Au32)
    };
    if let Some(c) = pick(&["popover_bg_color", "theme_base_color", "window_bg_color", "theme_bg_color"]) {
        pbg = c | 0xFF000000;
        cardc = mix(pbg, if dark { 0xFFFFFFFF } else { 0xFF000000 }, 0.07);
        brd = mix(pbg, if dark { 0xFFFFFFFF } else { 0xFF000000 }, 0.14);
    }
    if let Some(c) = pick(&["popover_fg_color", "theme_text_color", "window_fg_color", "theme_fg_color"]) {
        pfg = c | 0xFF000000;
        dim = mix(pbg, pfg, 0.65);
    }
    let accent = pick(&["accent_bg_color", "theme_selected_bg_color", "accent_color"]).map(|c| c | 0xFF000000).unwrap_or(0xFF3584E4);
    let (fam, size) = parse_font(&raw.font);
    let _ = scale;
    let mut t = Theme {
        dark,
        panel_bg: 0xFF000000,
        panel_fg: 0xFFFFFFFF,
        mantle: mix(pbg, 0xFF000000, 0.3),
        scheme: scheme.to_string(),
        popup_bg: pbg,
        popup_fg: pfg,
        dim_fg: dim,
        card: cardc,
        hover: mix(pbg, if dark { 0xFFFFFFFF } else { 0xFF000000 }, 0.10),
        border: brd,
        accent,
        accent_fg: 0xFFFFFFFF,
        font_family: fam,
        font_size: size,
        gtk_theme: raw.gtk_theme.clone(),
        icon_theme: raw.icon_theme.clone(),
    };
    if let Some(sc) = sch {
        t.dark = sc.dark;
        t.popup_bg = sc.base;
        t.popup_fg = sc.text;
        t.dim_fg = sc.sub;
        t.card = sc.surface0;
        t.hover = mix(sc.base, sc.text, 0.10);
        t.border = sc.surface1;
        t.accent = sc.accent;
        t.accent_fg = sc.accent_fg;
        t.mantle = sc.mantle;
        t.panel_bg = sc.mantle;
        t.panel_fg = sc.text;
    } else if bar_follow && !t.dark {
        t.panel_bg = mix(t.popup_bg, 0xFFFFFFFF, 0.3);
        t.panel_fg = t.popup_fg;
        t.mantle = mix(t.popup_bg, 0xFF000000, 0.06);
    }
    if accent_s != "auto" {
        if let Some(a) = parse_accent(accent_s) {
            t.accent = a;
            t.accent_fg = if luminance(a) > 0.62 { 0xFF1A1A1A } else { 0xFFFFFFFF };
        }
    }
    t
}

fn parse_font(s: &str) -> (String, f32) {
    let s = s.trim();
    if s.is_empty() {
        return ("Cantarell".into(), 11.0);
    }
    let mut parts: Vec<&str> = s.split_whitespace().collect();
    let mut size = 11.0;
    if let Some(l) = parts.last() {
        if let Ok(v) = l.parse::<f32>() {
            size = v;
            parts.pop();
        }
    }
    while let Some(l) = parts.last() {
        let ll = l.to_lowercase();
        if ["bold", "italic", "oblique", "regular", "medium", "light", "semibold", "normal"].contains(&ll.as_str()) {
            parts.pop();
        } else {
            break;
        }
    }
    let fam = if parts.is_empty() { "Cantarell".to_string() } else { parts.join(" ") };
    (fam, size.clamp(6.0, 32.0))
}

/// Locate font files for a family; returns up to a few candidate paths (regular first, then bold) .
pub fn find_fonts(family: &str, bold: bool) -> Vec<PathBuf> {
    let key: String = family.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    let mut dirs = vec![PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/local/share/fonts"), home().join(".fonts"), home().join(".local/share/fonts")];
    let mut found: Vec<(i32, PathBuf)> = vec![];
    let mut depth_stack: Vec<(PathBuf, u32)> = dirs.drain(..).map(|d| (d, 0)).collect();
    while let Some((d, depth)) = depth_stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 5 {
                    depth_stack.push((p, depth + 1));
                }
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            if !(name.ends_with(".ttf") || name.ends_with(".ttc")) {
                continue;
            }
            let flat: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
            let has_key = !key.is_empty() && flat.contains(&key);
            let is_bold = flat.contains("bold") && !flat.contains("italic") && !flat.contains("oblique");
            let bad = flat.contains("italic") || flat.contains("oblique") || flat.contains("light") || flat.contains("thin") || flat.contains("black") || flat.contains("condensed") || flat.contains("mono") && !key.contains("mono");
            let mut score = 0;
            if has_key {
                score += 100;
            }
            if bad {
                score -= 50;
            }
            if bold == is_bold {
                score += 20;
            } else {
                score -= 20;
            }
            if flat.contains("regular") || flat.contains("book") {
                score += 5;
            }
            if has_key {
                found.push((score, p));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out: Vec<PathBuf> = found.into_iter().map(|f| f.1).take(3).collect();
    // universal fallbacks
    let fb: &[&str] = if bold {
        &["/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf", "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf"]
    } else {
        &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "/usr/share/fonts/truetype/freefont/FreeSans.ttf"]
    };
    for f in fb {
        out.push(PathBuf::from(f));
    }
    out
}
