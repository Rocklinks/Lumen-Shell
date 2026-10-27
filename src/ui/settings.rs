//! GNOME-style Settings (libadwaita look): searchable sidebar, centered column of grouped cards,
//! switch/segmented/slider/combo/entry/navigation rows. Pages are pure data (see `page_groups`),
//! which also powers settings search. Works identically on X11 and Wayland; every change saves + applies live.
use super::*;

pub struct Settings {
    pub pane: Pane,
    pub page: usize,
    pub scroll: f32,
    pub side_scroll: f32,
    pub x: i32,
    pub y: i32,
    pub sinks: Vec<(String, String)>,
    pub default_sink: String,
    pub displays: Vec<DisplayOut>,
    pub disp_ok: bool,
    pub about: Vec<(String, String)>,
    pub query: String,
    pub searching: bool,
    pub edit: Option<(u16, String)>,
    pub expanded: Option<u16>,
    pub capture: Option<u8>,
    pub card: (f32, f32, f32, f32),
    pub max_scroll: f32,
}

pub struct PageDef {
    pub name: &'static str,
    pub icon: &'static str,
    pub kw: &'static str,
}

pub const PAGES: [PageDef; 18] = [
    PageDef { name: "Wi-Fi", icon: "network-wireless-symbolic", kw: "wireless network wlan internet connect" },
    PageDef { name: "Network", icon: "network-wired-symbolic", kw: "ethernet wired cable lan connection" },
    PageDef { name: "Bluetooth", icon: "bluetooth-symbolic", kw: "pair devices headphones" },
    PageDef { name: "Displays", icon: "video-display-symbolic", kw: "screen resolution refresh rate scale hdr orientation night light monitor" },
    PageDef { name: "Sound", icon: "audio-speakers-symbolic", kw: "volume audio output speaker mute" },
    PageDef { name: "Power", icon: "battery-level-80-symbolic", kw: "battery profile performance saver caffeine awake" },
    PageDef { name: "Multitasking", icon: "view-app-grid-symbolic", kw: "workspaces desktops" },
    PageDef { name: "Appearance", icon: "preferences-desktop-appearance-symbolic", kw: "theme dark light accent color scheme catppuccin cosmic style" },
    PageDef { name: "Notifications", icon: "preferences-system-notifications-symbolic", kw: "banners do not disturb dnd popups" },
    PageDef { name: "Date & Time", icon: "preferences-system-time-symbolic", kw: "clock 24-hour seconds timezone" },
    PageDef { name: "Top Bar", icon: "view-list-symbolic", kw: "panel widgets position height opacity transparency floating fullscreen autohide tray battery" },
    PageDef { name: "Dock", icon: "view-grid-symbolic", kw: "launcher icons favorites pinned size autohide" },
    PageDef { name: "Extensions", icon: "application-x-addon-symbolic", kw: "tray appindicator caffeine night light tweaks" },
    PageDef { name: "Tweaks", icon: "emblem-system-symbolic", kw: "autostart startup xfce4-panel session quit restart" },
    PageDef { name: "About", icon: "dialog-information-symbolic", kw: "system details hardware processor memory graphics disk os kernel" },
    PageDef { name: "GPU", icon: "computer-symbolic", kw: "graphics nvidia amd intel hybrid switch discrete supergfxctl envycontrol" },
    PageDef { name: "Screenshots", icon: "camera-photo-symbolic", kw: "screenshot screen recording record screencast folder videos mp4 capture" },
    PageDef { name: "Keyboard", icon: "input-keyboard-symbolic", kw: "shortcuts keybindings hotkeys print screen record labwc" },
];
pub const SIDE_GROUPS: [&[usize]; 5] = [&[0, 1, 2], &[3, 4, 5, 17, 15], &[6, 7, 8, 16, 9], &[10, 11, 12, 13], &[14]];
pub const WIDGETS: [(&str, &str); 6] = [("activities", "Activities button"), ("workspaces", "Workspaces"), ("appmenu", "Running app name"), ("clock", "Clock"), ("tray", "System tray"), ("status", "System status")];

fn widget_section(cfg: &Config, name: &str) -> i32 {
    if cfg.left.iter().any(|n| n == name) {
        1
    } else if cfg.center.iter().any(|n| n == name) {
        2
    } else if cfg.right.iter().any(|n| n == name) {
        3
    } else {
        0
    }
}

#[derive(Clone)]
pub enum RK {
    Info(String),
    Nav(String),
    Switch(bool, u16),
    Seg(Vec<(String, Act, bool)>),
    Slider(u8, f32, String),
    Entry(u16, String, String, bool),
    Combo(u16, String, bool),
    Mark(bool),
    Scheme([u32; 3], bool),
    Colors(Vec<(u32, Act, bool)>),
}

#[derive(Clone)]
pub struct Row {
    pub title: String,
    pub sub: Option<String>,
    pub icon: Option<String>,
    pub kind: RK,
    pub act: Option<Act>,
}

impl Row {
    fn new(title: &str, kind: RK) -> Row {
        Row { title: title.to_string(), sub: None, icon: None, kind, act: None }
    }
    fn sub(mut self, s: &str) -> Row {
        if !s.is_empty() {
            self.sub = Some(s.to_string());
        }
        self
    }
    fn icon(mut self, i: &str) -> Row {
        self.icon = Some(i.to_string());
        self
    }
    fn act(mut self, a: Act) -> Row {
        self.act = Some(a);
        self
    }
    fn nav(title: &str, suffix: &str, a: Act) -> Row {
        Row::new(title, RK::Nav(suffix.to_string())).act(a)
    }
    fn info(title: &str, v: &str) -> Row {
        Row::new(title, RK::Info(v.to_string()))
    }
    fn switch(title: &str, sub: &str, on: bool, id: u16) -> Row {
        Row::new(title, RK::Switch(on, id)).sub(sub)
    }
    fn seg(title: &str, opts: Vec<(String, Act, bool)>) -> Row {
        Row::new(title, RK::Seg(opts))
    }
    fn setting_seg(title: &str, id: u16, labels: &[&str], cur: i32) -> Row {
        Row::seg(title, labels.iter().enumerate().map(|(i, l)| (l.to_string(), Act::Setting(id, i as i32), cur == i as i32)).collect())
    }
    fn slider(title: &str, id: u8, frac: f32, text: &str) -> Row {
        Row::new(title, RK::Slider(id, frac, text.to_string()))
    }
    fn mark(title: &str, on: bool, a: Act) -> Row {
        Row::new(title, RK::Mark(on)).act(a)
    }
}

pub struct Group {
    pub title: String,
    pub desc: Option<String>,
    pub rows: Vec<Row>,
}
impl Group {
    fn new(t: &str) -> Group {
        Group { title: t.to_string(), desc: None, rows: vec![] }
    }
    fn desc(mut self, d: &str) -> Group {
        self.desc = Some(d.to_string());
        self
    }
    fn row(mut self, r: Row) -> Group {
        self.rows.push(r);
        self
    }
}

fn row_height(r: &Row, s: f32) -> f32 {
    (if r.sub.is_some() { 68.0 } else { 56.0 }) * s
}

fn vis_idx(v: Visibility) -> i32 {
    match v {
        Visibility::Always => 0,
        Visibility::AutoHide => 1,
        Visibility::HideInFullscreen => 2,
    }
}

impl Shell {
    /// All settings content as data. `st` carries page-local cached state (displays, sinks, expansion).
    pub fn page_groups(&self, p: usize, st: &Settings) -> Vec<Group> {
        let cfg = &self.cfg;
        let th = &self.th;
        let qs = &self.qs;
        let mut gs: Vec<Group> = vec![];
        match p {
            0 => {
                if !qs.has_nm {
                    return vec![Group::new("").row(Row::info("NetworkManager not found", "").sub("Wi-Fi control needs NetworkManager (nmcli)"))];
                }
                let on = qs.wifi == Some(true);
                let status = if qs.wifi == Some(false) { "Off".to_string() } else if qs.ssid.is_empty() { "Not connected".to_string() } else { format!("Connected to {}", qs.ssid) };
                gs.push(Group::new("").row(Row::switch("Wi-Fi", &status, on, 40)));
                if qs.pw_active {
                    let dots: String = "•".repeat(qs.pw_text.chars().count().min(40));
                    gs.push(Group::new("Password").desc(&format!("Enter the password for “{}”", qs.pw_ssid)).row(Row::new("Password", RK::Entry(0, dots, "".into(), true))).row(Row::nav("Connect", "", Act::PwConnect)).row(Row::nav("Cancel", "", Act::PwCancel)));
                } else if on {
                    let mut g = Group::new("Visible Networks");
                    if let Some(b) = qs.busy.as_ref().filter(|b| b.as_str() != "scan") {
                        g = g.desc(b);
                    }
                    for (i, n) in qs.nets.iter().take(14).enumerate() {
                        let ic = super::bar::net_icon(&crate::sysinfo::Net::Wifi { up: true }, Some(n.signal));
                        let suffix = if n.active { "Connected" } else if n.secure { "Secured" } else { "" };
                        g = g.row(Row::nav(&n.ssid, suffix, Act::WifiNet(i)).icon(ic));
                    }
                    if qs.nets.is_empty() {
                        g = g.row(Row::info(if qs.busy.as_deref() == Some("scan") { "Searching for networks…" } else { "No networks found" }, ""));
                    }
                    g = g.row(Row::nav("Rescan networks", "", Act::Setting(41, 0)));
                    gs.push(g);
                }
            }
            1 => {
                let wired = match self.net { crate::sysinfo::Net::Wired { up: true } => "Connected", crate::sysinfo::Net::Wired { up: false } => "Cable unplugged", _ => "No wired adapter detected" };
                gs.push(Group::new("Wired").row(Row::info("Ethernet", wired).icon("network-wired-symbolic")));
                if desktop::which("nm-connection-editor") {
                    gs.push(Group::new("Advanced").row(Row::nav("Network Connections…", "", Act::Cmd("nm-connection-editor".into()))));
                }
            }
            2 => {
                if !qs.has_bt {
                    return vec![Group::new("").row(Row::info("BlueZ not found", "").sub("Bluetooth control needs bluetoothctl"))];
                }
                let n_conn = qs.bt_devs.iter().filter(|d| d.connected).count();
                let status = if qs.bt != Some(true) { "Off".to_string() } else if n_conn > 0 { format!("{} connected", n_conn) } else { "On".to_string() };
                gs.push(Group::new("").row(Row::switch("Bluetooth", &status, qs.bt == Some(true), 42)));
                if qs.bt == Some(true) {
                    let mut g = Group::new("Devices");
                    if let Some(b) = qs.busy.as_ref().filter(|b| b.as_str() != "scan") {
                        g = g.desc(b);
                    }
                    for (i, d) in qs.bt_devs.iter().take(16).enumerate() {
                        let suffix = if d.connected { "Connected" } else if d.paired { "Paired" } else { "Pair" };
                        g = g.row(Row::nav(&d.name, suffix, Act::BtDev(i)).icon("bluetooth-active-symbolic"));
                    }
                    if qs.bt_devs.is_empty() {
                        g = g.row(Row::info("No devices yet", "").sub("Put a device in pairing mode, then scan"));
                    }
                    g = g.row(Row::nav("Scan for devices", "", Act::Setting(43, 0)));
                    gs.push(g);
                }
            }
            3 => {
                if let Some(left) = self.display_revert.as_ref().map(|r| 15u64.saturating_sub(r.3.elapsed().as_secs())) {
                    gs.push(Group::new("Keep these display settings?").desc(&format!("Reverting automatically in {} seconds", left)).row(Row::nav("Keep Changes", "", Act::Setting(50, 0))).row(Row::nav("Revert", "", Act::Setting(51, 0))));
                }
                if st.disp_ok && !st.displays.is_empty() {
                    for (oi, o) in st.displays.iter().enumerate().filter(|(_, o)| o.connected) {
                        let cur = o.modes.iter().find(|m| m.current).cloned();
                        let mut g = Group::new(&o.name);
                        let mut res: Vec<(i32, i32)> = vec![];
                        for m in &o.modes {
                            if !res.contains(&(m.w, m.h)) {
                                res.push((m.w, m.h));
                            }
                        }
                        res.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
                        res.truncate(12);
                        let rid = 300 + oi as u16 * 2;
                        let cur_res = cur.as_ref().map(|c| format!("{} × {}", c.w, c.h)).unwrap_or_default();
                        g = g.row(Row::new("Resolution", RK::Combo(rid, cur_res, st.expanded == Some(rid))).act(Act::Expand(rid)));
                        if st.expanded == Some(rid) {
                            for (w, h) in &res {
                                let best = o.modes.iter().filter(|m| m.w == *w && m.h == *h).min_by(|a, b| {
                                    let cr = cur.as_ref().map_or(60.0, |c| c.refresh);
                                    (a.refresh - cr).abs().partial_cmp(&(b.refresh - cr).abs()).unwrap_or(std::cmp::Ordering::Equal)
                                });
                                let pref = o.modes.iter().any(|m| m.w == *w && m.h == *h && m.preferred);
                                let is_cur = cur.as_ref().map_or(false, |c| c.w == *w && c.h == *h);
                                if let Some(b) = best {
                                    g = g.row(Row::mark(&format!("{} × {}{}", w, h, if pref { "  (Recommended)" } else { "" }), is_cur, Act::DispSet(oi, b.id, o.rotation)));
                                }
                            }
                        }
                        if let Some(c) = &cur {
                            let mut rates: Vec<&DisplayMode> = vec![];
                            for m in o.modes.iter().filter(|m| m.w == c.w && m.h == c.h) {
                                if !rates.iter().any(|x| (x.refresh - m.refresh).abs() < 0.05) {
                                    rates.push(m);
                                }
                            }
                            rates.sort_by(|a, b| b.refresh.partial_cmp(&a.refresh).unwrap_or(std::cmp::Ordering::Equal));
                            rates.truncate(6);
                            if rates.len() > 1 || c.refresh > 1.0 {
                                let opts: Vec<(String, Act, bool)> = rates.iter().map(|m| (format!("{:.0} Hz", m.refresh), Act::DispSet(oi, m.id, o.rotation), m.id == c.id)).collect();
                                g = g.row(Row::seg("Refresh Rate", opts));
                            }
                            let rots = [("Landscape", 1u16), ("Portrait Left", 2), ("Flipped", 4), ("Portrait Right", 8)];
                            g = g.row(Row::seg("Orientation", rots.iter().map(|(l, r)| (l.to_string(), Act::DispSet(oi, c.id, *r), o.rotation & 15 == *r)).collect()));
                        }
                        gs.push(g);
                    }
                } else {
                    let mut g = Group::new("Displays").desc("Resolution changes need an X11 session (RandR) for now");
                    for o in self.p.outputs() {
                        g = g.row(Row::info(if o.name.is_empty() { "Display" } else { &o.name }, &format!("{} × {}", o.w, o.h)));
                    }
                    gs.push(g);
                }
                let sc = [1.0f32, 1.25, 1.5, 2.0].iter().position(|v| (*v - cfg.ui_scale).abs() < 0.01).unwrap_or(0) as i32;
                gs.push(Group::new("Scale").desc("Scales the shell and, on XFCE, the font DPI").row(Row::setting_seg("Interface Scale", 2, &["100%", "125%", "150%", "200%"], sc)));
                gs.push(Group::new("Night Light").desc("Warmer colors in the evening reduce eye strain").row(Row::switch("Night Light", "", cfg.night_on, 21)).row(Row::slider("Color Temperature", 12, (cfg.night_temp as f32 - 1700.0) / 4800.0, &format!("{} K", cfg.night_temp))));
                gs.push(Group::new("HDR").row(Row::info("HDR", "Unavailable").sub("X11 and labwc have no HDR pipeline")));
            }
            4 => {
                if qs.audio == 0 {
                    return vec![Group::new("").row(Row::info("No audio control tool", "").sub("Needs pactl (PulseAudio/PipeWire) or wpctl"))];
                }
                gs.push(Group::new("Output").row(Row::slider("Output Volume", 0, qs.vol.unwrap_or(0.0).min(1.0), &format!("{}%", (qs.vol.unwrap_or(0.0) * 100.0).round() as i32))).row(Row::switch("Mute", "", qs.muted, 56)));
                if !st.sinks.is_empty() {
                    let mut g = Group::new("Output Device");
                    for (i, (n, _)) in st.sinks.iter().enumerate() {
                        g = g.row(Row::mark(n, *n == st.default_sink, Act::Setting(31, i as i32)).icon("audio-speakers-symbolic"));
                    }
                    gs.push(g);
                }
            }
            5 => {
                if qs.has_pp && !qs.profiles.is_empty() {
                    let opts: Vec<(String, Act, bool)> = qs.profiles.iter().enumerate().map(|(i, n)| (match n.as_str() { "power-saver" => "Power Saver".to_string(), "balanced" => "Balanced".into(), "performance" => "Performance".into(), o => o.to_string() }, Act::Setting(30, i as i32), qs.profile.as_deref() == Some(n.as_str()))).collect();
                    gs.push(Group::new("Power Mode").desc("Affects performance and battery life").row(Row::seg("Power Mode", opts)));
                }
                let b = &self.bat;
                gs.push(Group::new("Battery").row(Row::info("Battery", &if b.present { format!("{}%{}", b.percent, if b.charging { " · charging" } else if b.full { " · full" } else { "" }) } else { "No battery detected".into() })));
                let on = self.caffeine || self.caffeine_want;
                gs.push(Group::new("Power Saving").desc("Like the Caffeine extension").row(Row::switch("Keep Awake", "Prevent screen blanking and locking", on, 57)).row(Row::switch("Keep awake in fullscreen", "Automatic while a fullscreen app or game is active", cfg.caffeine_fullscreen, 52)));
            }
            6 => {
                let (names, _) = self.p.workspaces();
                let n = names.len();
                gs.push(Group::new("Workspaces").desc("Fixed number of workspaces").row(Row::setting_seg("Number of Workspaces", 160, &["1", "2", "3", "4", "6", "8"], [1usize, 2, 3, 4, 6, 8].iter().position(|v| *v == n).map_or(-1, |i| i as i32))));
                if n == 0 {
                    gs.push(Group::new("").row(Row::info("Workspaces unavailable", "").sub("This compositor does not expose workspaces to clients")));
                }
            }
            7 => {
                let mode = match cfg.theme { ThemeMode::Auto => 0, ThemeMode::Light => 1, ThemeMode::Dark => 2 };
                gs.push(Group::new("Style").row(Row::setting_seg("Style", 1, &["Auto", "Light", "Dark"], mode)).row(Row::switch("Top bar follows the theme", "Off = always black like GNOME", cfg.bar_follow_theme, 90)));
                let cols: Vec<(u32, Act, bool)> = crate::theme::ACCENTS.iter().enumerate().map(|(i, (n, c))| (*c, Act::Setting(80 + i as u16, 0), cfg.accent == *n)).chain(std::iter::once((th.accent, Act::Setting(89, 0), cfg.accent == "auto"))).collect();
                gs.push(Group::new("Accent Color").row(Row::new("Accent Color", RK::Colors(cols))));
                let mut g = Group::new("Color Scheme").desc("Catppuccin, COSMIC and more").row(Row::new("System theme", RK::Scheme([th.popup_bg, th.popup_fg, th.accent], cfg.scheme == "system")).act(Act::Setting(59, 0)));
                for (i, sc) in crate::theme::SCHEMES.iter().enumerate() {
                    g = g.row(Row::new(sc.name, RK::Scheme([sc.base, sc.text, sc.accent], cfg.scheme == sc.id)).act(Act::Setting(60 + i as u16, 0)));
                }
                gs.push(g);
            }
            8 => {
                let state = match self.svc.as_ref() { None => "Session bus unavailable", Some(sv) if sv.have_notif_name => "Active", Some(_) => "Another daemon owns the notification service" };
                gs.push(Group::new("").row(Row::switch("Notification Banners", "Pop-ups at the top of the screen", cfg.notify_banners, 53)).row(Row::switch("Do Not Disturb", "Banners are hidden; notifications still collect in the date menu", self.dnd(), 54)));
                gs.push(Group::new("Service").row(Row::switch("Replace other notification daemons", "Stops xfce4-notifyd, dunst, mako or swaync", cfg.notify_takeover, 55)).row(Row::info("Notification service", state)).row(Row::info("Tray (AppIndicator)", if self.svc.is_some() { "Active" } else { "Unavailable" })));
            }
            9 => {
                let tz = std::fs::read_link("/etc/localtime").ok().and_then(|p| p.to_str().map(|s| s.rsplit("zoneinfo/").next().unwrap_or(s).to_string())).unwrap_or_else(|| "System default".into());
                gs.push(Group::new("Clock").row(Row::switch("24-hour Time", "", cfg.clock_24h, 18)).row(Row::switch("Show Seconds", "", cfg.clock_seconds, 19)).row(Row::switch("Show Date in the Top Bar", "", cfg.clock_date, 20)));
                gs.push(Group::new("Current").row(Row::info("Now", &super::bar::clock_text(&Config { clock_date: true, clock_seconds: true, ..cfg.clone() }, true))).row(Row::info("Time Zone", &tz)));
            }
            10 => {
                gs.push(Group::new("Top Bar").row(Row::switch("Show Top Bar", "", cfg.bar_enabled, 3)).row(Row::setting_seg("Position", 4, &["Top", "Bottom"], if cfg.bar_edge == Edge::Bottom { 1 } else { 0 })).row(Row::slider("Height", 14, (cfg.bar_height as f32 - 24.0) / 40.0, &format!("{} px", cfg.bar_height))).row(Row::slider("Transparency", 10, cfg.bar_opacity, &format!("{}%", (cfg.bar_opacity * 100.0).round() as i32))).row(Row::switch("Floating Bar", "Rounded pill with margins (needs compositing on X11)", cfg.bar_floating, 5)).row(Row::setting_seg("Visibility", 6, &["Always", "Auto-hide", "Hide in fullscreen"], vis_idx(cfg.bar_vis))).row(Row::switch("Stay Above Fullscreen Windows", "Keeps the bar visible over games and video", cfg.bar_over_fullscreen, 7)).row(Row::switch("Show Battery Percentage", "", cfg.battery_percent, 91)));
                let mut g = Group::new("Widget Placement").desc("Move any widget to the left, center or right");
                for (i, (name, label)) in WIDGETS.iter().enumerate() {
                    g = g.row(Row::setting_seg(label, 100 + i as u16, &["Off", "Left", "Center", "Right"], widget_section(cfg, name)));
                }
                gs.push(g);
            }
            11 => {
                gs.push(Group::new("Dock").row(Row::switch("Show Dock", "", cfg.dock_enabled, 8)).row(Row::setting_seg("Position", 9, &["Bottom", "Top"], if cfg.dock_edge == Edge::Top { 1 } else { 0 })).row(Row::slider("Icon Size", 13, (cfg.dock_icon as f32 - 24.0) / 72.0, &format!("{} px", cfg.dock_icon))).row(Row::slider("Transparency", 11, cfg.dock_opacity, &format!("{}%", (cfg.dock_opacity * 100.0).round() as i32))).row(Row::setting_seg("Visibility", 16, &["Always", "Auto-hide", "Hide in fullscreen"], vis_idx(cfg.dock_vis))).row(Row::switch("Stay Above Fullscreen Windows", "", cfg.dock_over_fullscreen, 17)));
                let mut g = Group::new("Pinned Apps").desc("Right-click an app in the grid to pin it");
                for (i, f) in cfg.favorites.iter().enumerate() {
                    let name = self.apps.iter().find(|a| a.id.eq_ignore_ascii_case(f.trim_end_matches(".desktop"))).map(|a| a.name.clone()).unwrap_or_else(|| f.clone());
                    g = g.row(Row::nav(&name, "Remove", Act::Setting(200 + i as u16, 0)));
                }
                if cfg.favorites.is_empty() {
                    g = g.row(Row::info("No pinned apps", ""));
                }
                gs.push(g);
            }
            12 => {
                gs.push(Group::new("Built-in Extensions").desc("What GNOME needs extensions for is built in")
                    .row(Row::nav("Workspace Indicator", "Top Bar", Act::SettingsPage(10)))
                    .row(Row::nav("AppIndicator / KStatusNotifierItem", "Top Bar", Act::SettingsPage(10)))
                    .row(Row::nav("Notification Center", "Notifications", Act::SettingsPage(8)))
                    .row(Row::nav("Caffeine", "Power", Act::SettingsPage(5)))
                    .row(Row::nav("Night Light", "Displays", Act::SettingsPage(3)))
                    .row(Row::nav("GPU Profile Switcher", "GPU", Act::SettingsPage(15)))
                    .row(Row::nav("Color Schemes (User Themes)", "Appearance", Act::SettingsPage(7))));
            }
            13 => {
                gs.push(Group::new("Startup").row(Row::switch("Start lumen-shell at Login", "Adds an autostart entry", crate::lifecycle::autostart_enabled(), 150)).row(Row::switch("Replace xfce4-panel", "Stops xfce4-panel while running and starts it again when lumen exits", cfg.manage_panel, 151)));
                gs.push(Group::new("Session").row(Row::nav("Restart lumen-shell", "", Act::Restart)).row(Row::nav("Quit lumen-shell", "xfce4-panel comes back", Act::Quit)));
                gs.push(Group::new("Files").row(Row::info("Config", &cfg.path.to_string_lossy())));
            }
            14 => {
                let hw = ["Device Name", "Hardware Model", "Memory", "Processor", "Graphics", "Disk Capacity"];
                let mut g1 = Group::new("Hardware");
                let mut g2 = Group::new("Software");
                for (k, v) in &st.about {
                    if hw.contains(&k.as_str()) {
                        g1 = g1.row(Row::info(k, v));
                    } else {
                        g2 = g2.row(Row::info(k, v));
                    }
                }
                gs.push(g1);
                gs.push(g2);
            }
            15 => {
                if qs.gpu != 0 {
                    let cur = qs.gpu_mode.clone();
                    let opts: Vec<(String, Act, bool)> = qs.gpu_modes.iter().enumerate().map(|(i, m)| (m.clone(), Act::GpuMode(i), cur.eq_ignore_ascii_case(m))).collect();
                    gs.push(Group::new("Graphics Mode").desc("A log out or reboot is needed to apply").row(Row::seg("Mode", opts)).row(Row::info("Current", &cur)));
                } else {
                    gs.push(Group::new("Graphics").row(Row::info("No switchable GPU", "").sub("Install supergfxctl or envycontrol for hybrid-graphics switching")));
                }
                if let Some((_, v)) = st.about.iter().find(|(k, _)| k == "Graphics") {
                    gs.push(Group::new("Detected").row(Row::info("Graphics", v)));
                }
            }
            16 => {
                let tool = if std::env::var("WAYLAND_DISPLAY").is_ok() && self.p.name() == "wayland" { "wf-recorder" } else { "ffmpeg" };
                let have = desktop::which(tool);
                gs.push(Group::new("Screenshots").desc("Print opens the screenshot tool with Selection and Screen modes")
                    .row(Row::new("Save Location", RK::Entry(10, cfg.shot_dir.clone(), "~/Screenshots".into(), false)))
                    .row(Row::nav("Take a Screenshot…", "", Act::ShotOpen))
                    .row(Row::nav("Capture the Whole Screen", "", Act::ShotInstant)));
                gs.push(Group::new("Screen Recording").desc("Saved as MP4")
                    .row(Row::new("Save Location", RK::Entry(11, cfg.video_dir.clone(), "~/Videos".into(), false)))
                    .row(Row::setting_seg("Frame Rate", 170, &["15", "30", "60"], match cfg.record_fps { 15 => 0, 60 => 2, _ => 1 }))
                    .row(Row::switch("Record Audio", "Default sound input", cfg.record_audio, 180))
                    .row(Row::nav(if self.rec.is_some() { "Stop Recording" } else { "Start Recording" }, "", Act::RecordToggle))
                    .row(Row::info("Encoder", if have { tool } else { "Not installed" }).sub(if have { "" } else { "Install ffmpeg (X11) or wf-recorder (Wayland) to record" })));
            }
            17 => {
                let wl = self.p.name() == "wayland";
                let names = ["Screenshot Tool", "Screenshot of the Screen", "Screenshot of an Area", "Start / Stop Screen Recording", "Show Applications"];
                let mut g = Group::new("Shortcuts").desc(if wl { "Wayland has no global key grabs: install them into your labwc config below" } else { "Click a shortcut, then press the new keys (Esc cancels)" });
                for (i, n) in names.iter().enumerate() {
                    let cur = if st.capture == Some(i as u8) { "Press keys…".to_string() } else { cfg.sc[i].clone() };
                    g = g.row(Row::nav(n, &cur, Act::Capture(i as u8)));
                }
                gs.push(g);
                if wl {
                    let on = Shell::labwc_installed();
                    gs.push(Group::new("labwc").desc("Adds a marked block to ~/.config/labwc/rc.xml (backup kept as rc.xml.lumen.bak)")
                        .row(Row::nav("Install shortcuts into labwc", if on { "Installed" } else { "" }, Act::LabwcInstall))
                        .row(Row::nav("Remove shortcuts from labwc", "", Act::LabwcRemove)));
                } else {
                    gs.push(Group::new("Status").row(Row::info("Global shortcuts active", &format!("{} of 5", self.shortcuts_ok))));
                }
            }
            _ => {}
        }
        gs
    }

    fn search_groups(&self, st: &Settings) -> Vec<Group> {
        let q = st.query.to_lowercase();
        let mut g = Group::new("Search Results");
        for (pi, pd) in PAGES.iter().enumerate() {
            if pd.name.to_lowercase().contains(&q) || pd.kw.contains(&q) {
                g = g.row(Row::nav(pd.name, "", Act::SettingsPage(pi)).icon(pd.icon));
            }
        }
        for pi in 0..PAGES.len() {
            for gr in self.page_groups(pi, st) {
                for r in gr.rows {
                    if !r.title.is_empty() && r.title.to_lowercase().contains(&q) && !matches!(r.kind, RK::Mark(_)) {
                        g = g.row(Row::nav(&r.title, PAGES[pi].name, Act::SettingsPage(pi)));
                    }
                }
            }
        }
        if g.rows.is_empty() {
            g = g.row(Row::info("No results", "").sub(&format!("Nothing matches “{}”", st.query)));
        }
        vec![g]
    }
}

impl Shell {
    pub fn open_settings(&mut self, page: usize) {
        if self.settings.is_some() {
            if let Some(s) = &mut self.settings {
                s.page = page;
                s.pane.dirty = true;
            }
            self.settings_enter(page);
            return;
        }
        self.close_popup();
        self.close_grid();
        let o = self.out();
        let top = if self.cfg.bar_edge == Edge::Bottom { 0 } else { self.bar_total() };
        let mut sp = SurfSpec::new(Kind::Overlay);
        sp.x = 0;
        sp.y = top;
        sp.w = o.w;
        sp.h = (o.h - top).max(300);
        sp.keyboard = true;
        let id = self.p.create(&sp);
        self.p.set_visible(id, true);
        let mut st = Settings { pane: Pane::new(id, sp.w, sp.h), page, scroll: 0.0, side_scroll: 0.0, x: 0, y: top, sinks: vec![], default_sink: String::new(), displays: vec![], disp_ok: false, about: vec![], query: String::new(), searching: false, edit: None, expanded: None, capture: None, card: (0.0, 0.0, 0.0, 0.0), max_scroll: 0.0 };
        st.pane.dirty = true;
        self.settings = Some(st);
        self.qs.refresh_all(&mut self.jobs);
        if self.qs.audio == 1 {
            self.jobs.run("sinks", &["pactl", "list", "short", "sinks"], None, 3000);
            self.jobs.run("defsink", &["pactl", "get-default-sink"], None, 3000);
        }
        self.settings_enter(page);
    }

    pub fn settings_enter(&mut self, p: usize) {
        match p {
            0 => {
                if self.qs.has_nm {
                    self.qs.refresh_all(&mut self.jobs);
                    self.scan_wifi();
                }
            }
            2 => {
                if self.qs.has_bt {
                    self.qs.refresh_all(&mut self.jobs);
                    self.jobs.run("bt_scan", &["bluetoothctl", "--timeout", "8", "scan", "on"], None, 12000);
                }
            }
            3 => self.refresh_displays(),
            14 | 15 => {
                let a = crate::sysinfo::about();
                if let Some(s) = &mut self.settings {
                    s.about = a;
                }
            }
            _ => {}
        }
        if let Some(s) = &mut self.settings {
            s.pane.dirty = true;
        }
    }

    pub fn refresh_displays(&mut self) {
        let d = self.p.display_outputs();
        if let Some(s) = &mut self.settings {
            s.disp_ok = d.is_some();
            s.displays = d.unwrap_or_default();
            s.pane.dirty = true;
        }
    }

    pub fn disp_set(&mut self, oi: usize, mode: u32, rot: u16) {
        let cur = self.settings.as_ref().and_then(|s| s.displays.get(oi)).cloned();
        if let Some(o) = cur {
            let old_mode = o.modes.iter().find(|m| m.current).map_or(0, |m| m.id);
            if self.p.display_set_mode(&o.name, mode, rot) {
                if self.display_revert.is_none() {
                    self.display_revert = Some((o.name.clone(), old_mode, o.rotation, Instant::now()));
                }
                self.refresh_displays();
            } else {
                self.toast("Could not apply that display mode");
            }
        }
    }

    pub fn display_revert_tick(&mut self) {
        if let Some((name, mode, rot, t)) = self.display_revert.clone() {
            if t.elapsed() > Duration::from_secs(15) {
                self.display_revert = None;
                self.p.display_set_mode(&name, mode, rot);
                self.refresh_displays();
                self.toast("Display settings reverted");
            } else if let Some(s) = &mut self.settings {
                if s.page == 2 {
                    s.pane.dirty = true;
                }
            }
        }
    }

    pub fn close_settings(&mut self) {
        if let Some(s) = self.settings.take() {
            self.p.destroy(s.pane.id);
            self.restore_focus();
        }
    }


    pub fn settings_key(&mut self, sym: u32, text: &str) {
        if self.qs.pw_active {
            match sym {
                0xff1b => {
                    self.qs.pw_active = false;
                    self.qs.pw_text.clear();
                }
                0xff0d | 0xff8d => self.submit_password(),
                0xff08 => {
                    self.qs.pw_text.pop();
                }
                _ => {
                    if !text.is_empty() && self.qs.pw_text.len() < 128 {
                        self.qs.pw_text.push_str(text);
                    }
                }
            }
            if let Some(st) = &mut self.settings {
                st.pane.dirty = true;
            }
            return;
        }
        let st = match &mut self.settings {
            Some(s) => s,
            None => return,
        };
        st.pane.dirty = true;
        if let Some((id, buf)) = st.edit.as_mut() {
            match sym {
                0xff1b => st.edit = None,
                0xff0d | 0xff8d => {
                    let (i, b) = (*id, buf.clone());
                    st.edit = None;
                    self.apply_edit(i, &b);
                }
                0xff08 => {
                    buf.pop();
                }
                _ => {
                    if !text.is_empty() && buf.len() < 256 {
                        buf.push_str(text);
                    }
                }
            }
            return;
        }
        match sym {
            0xff1b => {
                if st.searching || !st.query.is_empty() {
                    st.searching = false;
                    st.query.clear();
                } else {
                    self.close_settings();
                }
            }
            0xff08 => {
                st.query.pop();
                if st.query.is_empty() {
                    st.searching = false;
                }
            }
            0xff55 | 0xff52 => st.scroll = (st.scroll - 80.0).max(0.0),
            0xff56 | 0xff54 => st.scroll = (st.scroll + 80.0).min(st.max_scroll),
            _ => {
                if !text.is_empty() && text != " " || (st.searching && text == " ") {
                    st.searching = true;
                    st.query.push_str(text);
                    st.scroll = 0.0;
                }
            }
        }
    }

    pub fn apply_edit(&mut self, id: u16, text: &str) {
        let t = text.trim();
        if t.is_empty() {
            return;
        }
        match id {
            10 => self.cfg.shot_dir = t.to_string(),
            11 => self.cfg.video_dir = t.to_string(),
            _ => return,
        }
        self.save_cfg_quiet();
    }

    pub fn settings_pointer(&mut self, x: f32, y: f32, what: Ptr) {
        let (act, card) = {
            let st = self.settings.as_mut().unwrap();
            let h = st.pane.hit_at(x, y);
            if st.pane.hover != h {
                st.pane.hover = h;
                st.pane.dirty = true;
            }
            (h.map(|i| (st.pane.hits[i].0, st.pane.hits[i].1.clone())), st.card)
        };
        let s = self.s;
        match (what, act) {
            (Ptr::Scroll(d), _) => {
                if let Some(st) = &mut self.settings {
                    if x < card.0 + 260.0 * s {
                        st.side_scroll = (st.side_scroll + d as f32 * 50.0 * s).max(0.0);
                    } else {
                        st.scroll = (st.scroll + d as f32 * 60.0 * s).clamp(0.0, st.max_scroll.max(0.0));
                    }
                    st.pane.dirty = true;
                }
            }
            (Ptr::Down(1), Some((r, Act::Slider(id)))) => {
                let (x0, x1) = (r.0 + 8.0 * s, r.0 + r.2 - 8.0 * s);
                self.drag = Some((id, x0, x1));
                let f = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                self.slider_set(id, f, false);
            }
            (Ptr::Down(1), Some((_, a))) => self.settings_act(a),
            (Ptr::Down(1), None) => {
                if x < card.0 || x > card.0 + card.2 || y < card.1 || y > card.1 + card.3 {
                    self.close_settings();
                }
            }
            _ => {}
        }
    }

    pub fn settings_act(&mut self, a: Act) {
        match a {
            Act::SettingsClose => self.close_settings(),
            Act::SettingsSearch => {
                if let Some(s) = &mut self.settings {
                    s.searching = !s.searching;
                    if !s.searching {
                        s.query.clear();
                    }
                    s.pane.dirty = true;
                }
            }
            Act::Expand(id) => {
                if let Some(s) = &mut self.settings {
                    s.expanded = if s.expanded == Some(id) { None } else { Some(id) };
                    s.pane.dirty = true;
                }
            }
            Act::Edit(id) => {
                let init = match id {
                    10 => self.cfg.shot_dir.clone(),
                    11 => self.cfg.video_dir.clone(),
                    _ => String::new(),
                };
                if let Some(s) = &mut self.settings {
                    s.edit = Some((id, init));
                    s.pane.dirty = true;
                }
            }
            Act::Quit => {
                self.quit = true;
            }
            Act::ShotOpen => {
                self.close_settings();
                self.open_screenshot(0);
            }
            Act::ShotInstant => {
                self.close_settings();
                self.shot_instant();
            }
            Act::RecordToggle => {
                self.record_toggle();
                if let Some(s) = &mut self.settings {
                    s.pane.dirty = true;
                }
            }
            Act::Capture(i) => {
                if let Some(s) = &mut self.settings {
                    s.capture = Some(i);
                    s.pane.dirty = true;
                }
            }
            Act::LabwcInstall => self.labwc_install(true),
            Act::LabwcRemove => self.labwc_install(false),
            Act::Restart => {
                self.shutdown();
                std::process::exit(101);
            }
            Act::SettingsPage(p) => {
                if let Some(s) = &mut self.settings {
                    s.page = p;
                    s.scroll = 0.0;
                    s.searching = false;
                    s.query.clear();
                    s.expanded = None;
                    s.pane.dirty = true;
                }
                self.settings_enter(p);
            }
            Act::WifiNet(_) | Act::PwConnect | Act::PwCancel | Act::BtDev(_) | Act::Profile(_) | Act::GpuMode(_) => {
                self.popup_act(a, (0.0, 0.0, 0.0, 0.0), 0.0);
                if let Some(s) = &mut self.settings {
                    s.pane.dirty = true;
                }
            }
            Act::DispSet(oi, m, r) => self.disp_set(oi, m, r),
            Act::Setting(id, v) => self.apply_setting(id, v),
            Act::OpenQuick(pg) => {
                self.close_settings();
                self.open_popup(popup::Kind2::Quick, None);
                if let Some(p) = &mut self.pop {
                    p.page = pg;
                    p.relayout = true;
                    p.pane.dirty = true;
                }
                if pg == popup::QPage::Wifi {
                    self.qs.busy = Some("scan".into());
                    self.jobs.run("wifi_list", &["nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "yes"], None, 15000);
                }
            }
            Act::Cmd(c) => {
                let v: Vec<&str> = c.split_whitespace().collect();
                self.spawn_cmd(&v);
            }
            _ => {}
        }
    }

    fn save_apply(&mut self, structural: bool) {
        self.save_cfg_quiet();
        if structural {
            self.rebuild = true;
        } else {
            self.mark_all();
        }
        if let Some(s) = &mut self.settings {
            s.pane.dirty = true;
        }
    }

    pub fn apply_setting(&mut self, id: u16, v: i32) {
        let b = v != 0;
        match id {
            1 => {
                self.cfg.theme = match v {
                    1 => ThemeMode::Light,
                    2 => ThemeMode::Dark,
                    _ => ThemeMode::Auto,
                };
                self.save_apply(false);
                self.reload_theme();
            }
            2 => {
                self.cfg.ui_scale = [1.0, 1.25, 1.5, 2.0][v.clamp(0, 3) as usize];
                self.s = self.cfg.ui_scale;
                self.save_apply(true);
                if std::env::var("DISPLAY").is_ok() && desktop::which("xfconf-query") {
                    let dpi = (96.0 * self.cfg.ui_scale * 1024.0) as i32 / 1024;
                    let d = dpi.to_string();
                    self.spawn_cmd(&["xfconf-query", "-c", "xsettings", "-p", "/Xft/DPI", "-n", "-t", "int", "-s", &d]);
                }
            }
            56 => self.popup_act(Act::VolMute, (0.0, 0.0, 0.0, 0.0), 0.0),
            57 => {
                let on = self.caffeine || self.caffeine_want;
                self.set_caffeine(!on, 0);
            }
            150 => {
                crate::lifecycle::set_autostart(b);
            }
            151 => {
                self.cfg.manage_panel = b;
                self.save_apply(false);
            }
            160..=176 => {
                self.p.set_workspace_count((id - 160) as usize);
            }
            170..=172 => {
                self.cfg.record_fps = [15, 30, 60][(id - 170) as usize];
                self.save_apply(false);
            }
            180 => {
                self.cfg.record_audio = b;
                self.save_apply(false);
            }
            40 => self.tile_click(0, false),
            41 => self.scan_wifi(),
            42 => self.tile_click(1, false),
            43 => self.jobs.run("bt_scan", &["bluetoothctl", "--timeout", "8", "scan", "on"], None, 12000),
            50 => {
                self.display_revert = None;
                if let Some(s) = &mut self.settings {
                    s.pane.dirty = true;
                }
            }
            51 => {
                if let Some((name, mode, rot, _)) = self.display_revert.take() {
                    self.p.display_set_mode(&name, mode, rot);
                    self.refresh_displays();
                }
            }
            52 => {
                self.cfg.caffeine_fullscreen = b;
                self.save_apply(false);
            }
            53 => {
                self.cfg.notify_banners = b;
                self.save_apply(false);
                self.update_banner();
            }
            54 => self.toggle_dnd(),
            55 => {
                self.cfg.notify_takeover = b;
                self.save_apply(false);
                if b {
                    self.jobs.run("notify_retry", &["sh", "-c", "pkill -x xfce4-notifyd; pkill -x dunst; pkill -x mako; pkill -x swaync; sleep 0.8; true"], None, 6000);
                }
            }
            59 => {
                self.cfg.scheme = "system".into();
                self.save_apply(false);
                self.reload_theme();
            }
            60..=79 => {
                if let Some(sc) = crate::theme::SCHEMES.get((id - 60) as usize) {
                    self.cfg.scheme = sc.id.to_string();
                    self.save_apply(false);
                    self.reload_theme();
                }
            }
            80..=89 => {
                let acc = if id == 89 { "auto".to_string() } else { crate::theme::ACCENTS.get((id - 80) as usize).map(|a| a.0.to_string()).unwrap_or_else(|| "auto".into()) };
                self.cfg.accent = acc;
                self.save_apply(false);
                self.reload_theme();
            }
            90 => {
                self.cfg.bar_follow_theme = b;
                self.save_apply(false);
                self.reload_theme();
            }
            91 => {
                self.cfg.battery_percent = b;
                self.save_apply(false);
            }
            3 => {
                self.cfg.bar_enabled = b;
                self.save_apply(true);
            }
            4 => {
                self.cfg.bar_edge = if v == 1 { Edge::Bottom } else { Edge::Top };
                self.save_apply(true);
            }
            5 => {
                self.cfg.bar_floating = b;
                self.save_apply(true);
            }
            6 => {
                self.cfg.bar_vis = [Visibility::Always, Visibility::AutoHide, Visibility::HideInFullscreen][v.clamp(0, 2) as usize];
                self.save_apply(true);
            }
            7 => {
                self.cfg.bar_over_fullscreen = b;
                self.save_apply(true);
            }
            8 => {
                self.cfg.dock_enabled = b;
                self.save_apply(true);
            }
            9 => {
                self.cfg.dock_edge = if v == 1 { Edge::Top } else { Edge::Bottom };
                self.save_apply(true);
            }
            16 => {
                self.cfg.dock_vis = [Visibility::Always, Visibility::AutoHide, Visibility::HideInFullscreen][v.clamp(0, 2) as usize];
                self.save_apply(true);
            }
            17 => {
                self.cfg.dock_over_fullscreen = b;
                self.save_apply(true);
            }
            18 => {
                self.cfg.clock_24h = b;
                self.save_apply(false);
            }
            19 => {
                self.cfg.clock_seconds = b;
                self.save_apply(false);
            }
            20 => {
                self.cfg.clock_date = b;
                self.save_apply(false);
            }
            21 => {
                self.cfg.night_on = b;
                self.save_apply(false);
                self.apply_night();
            }
            30 => {
                // power profile by index
                self.popup_act_profile(v as usize);
            }
            31 => {
                // default sink by index
                let name = self.settings.as_ref().and_then(|s| s.sinks.get(v as usize)).map(|s| s.0.clone());
                if let Some(n) = name {
                    self.jobs.run("setsink", &["pactl", "set-default-sink", &n], None, 3000);
                    if let Some(s) = &mut self.settings {
                        s.default_sink = n;
                    }
                    self.qs.refresh_vol(&mut self.jobs);
                }
            }
            100..=199 => {
                let wi = (id - 100) as usize;
                if let Some((name, _)) = WIDGETS.get(wi) {
                    for l in [&mut self.cfg.left, &mut self.cfg.center, &mut self.cfg.right] {
                        l.retain(|n| n != name);
                    }
                    let target = match v {
                        1 => Some(0),
                        2 => Some(1),
                        3 => Some(2),
                        _ => None,
                    };
                    if let Some(t) = target {
                        let canon: Vec<&str> = WIDGETS.iter().map(|w| w.0).collect();
                        let list = match t {
                            0 => &mut self.cfg.left,
                            1 => &mut self.cfg.center,
                            _ => &mut self.cfg.right,
                        };
                        let pos = list.iter().position(|n| canon.iter().position(|c| c == n).unwrap_or(99) > wi).unwrap_or(list.len());
                        list.insert(pos, name.to_string());
                    }
                    self.save_apply(false);
                }
            }
            200..=299 => {
                let i = (id - 200) as usize;
                if i < self.cfg.favorites.len() {
                    self.cfg.favorites.remove(i);
                    self.save_apply(true);
                }
            }
            _ => {}
        }
    }

    fn popup_act_profile(&mut self, i: usize) {
        if let Some(n) = self.qs.profiles.get(i).cloned() {
            self.jobs.run("pp_set", &["powerprofilesctl", "set", &n], None, 4000);
            self.qs.profile = Some(n);
            if let Some(s) = &mut self.settings {
                s.pane.dirty = true;
            }
        }
    }

    /// Live slider changes for settings sliders (id >= 10).
    pub fn settings_slider(&mut self, id: u8, f: f32, commit: bool) {
        match id {
            10 => self.cfg.bar_opacity = (f * 100.0).round() / 100.0,
            11 => self.cfg.dock_opacity = (f * 100.0).round() / 100.0,
            12 => {
                self.cfg.night_temp = (1700.0 + f * (6500.0 - 1700.0)).round() as i32;
                if self.cfg.night_on {
                    self.apply_night();
                }
            }
            13 => self.cfg.dock_icon = (24.0 + f * 72.0).round() as i32,
            14 => self.cfg.bar_height = (24.0 + f * 40.0).round() as i32,
            _ => {}
        }
        if commit {
            self.save_apply(matches!(id, 13 | 14));
        } else {
            self.mark_all();
            if let Some(s) = &mut self.settings {
                s.pane.dirty = true;
            }
        }
    }

    pub fn on_settings_job(&mut self, tag: &str, ok: bool, out: &str) {
        if let Some(s) = &mut self.settings {
            match tag {
                "sinks" if ok => {
                    s.sinks = out.lines().filter_map(|l| {
                        let f: Vec<&str> = l.split('\t').collect();
                        if f.len() >= 2 { Some((f[1].to_string(), f[1].to_string())) } else { None }
                    }).collect();
                }
                "defsink" if ok => s.default_sink = out.trim().to_string(),
                _ => {}
            }
            s.pane.dirty = true;
        }
    }

    pub fn ipc_cmd(&mut self, c: &str) {
        match c {
            "settings" => self.open_settings(0),
            "grid" => {
                if self.grid.is_some() {
                    self.close_grid();
                } else {
                    self.open_grid();
                }
            }
            "calendar" => self.toggle_popup(popup::Kind2::Calendar),
            "quick" => self.toggle_popup(popup::Kind2::Quick),
            c if c.starts_with("settings:") => {
                let n = c[9..].parse::<usize>().unwrap_or(0).min(PAGES.len() - 1);
                self.open_settings(n);
            }
            "screenshot" => self.open_screenshot(0),
            "shot-area" => self.open_screenshot(0),
            "shot-screen" => self.shot_instant(),
            "record" => self.record_toggle(),
            "reload" => self.reload_config(),
            "quit" => self.quit = true,
            _ => {}
        }
    }


    pub fn draw_settings(&mut self) {
        let mut st = match self.settings.take() {
            Some(s) => s,
            None => return,
        };
        st.pane.dirty = false;
        let s = self.s;
        let th = self.th.clone();
        let o = self.out();
        let (w, h) = (st.pane.w as f32, st.pane.h as f32);
        let (cw, ch) = ((1080.0 * s).min(o.w as f32 - 32.0 * s), (720.0 * s).min(h - 24.0 * s));
        let (cx, cy) = ((w - cw) / 2.0, (h - ch) / 2.0);
        st.card = (cx, cy, cw, ch);
        let side_w = 260.0 * s;
        let groups = if st.query.is_empty() { self.page_groups(st.page, &st) } else { self.search_groups(&st) };
        let outs_note = ();
        let _ = outs_note;
        let hover = st.pane.hover;
        let page = st.page;
        let searching = st.searching || !st.query.is_empty();
        let query = st.query.clone();
        let edit = st.edit.clone();
        let (scroll, side_scroll) = (st.scroll, st.side_scroll);
        let mut hits: Vec<(R, Act)> = vec![];
        let cv = &mut st.pane.cv;
        cv.clear();
        cv.fill_rect(0, 0, w as i32, h as i32, if self.alpha { with_alpha(0xFF000000, 0.55) } else { mix(th.popup_bg, 0xFF000000, 0.6) });
        if self.alpha {
            shadow(cv, cx, cy, cw, ch, 16.0 * s, 24.0 * s, 1.0);
            cv.fill_rrect(cx, cy, cw, ch, 16.0 * s, th.popup_bg);
        } else {
            cv.fill_rect(cx as i32, cy as i32, cw as i32, ch as i32, th.popup_bg);
        }
        // sidebar background (rounded on the left only)
        let sidebar_bg = mix(th.popup_bg, if th.dark { 0xFF000000 } else { 0xFF808080 }, if th.dark { 0.22 } else { 0.06 });
        let mut sb = Shape::for_rect(cx, cy, side_w, ch, 1);
        sb.rrect(cx, cy, side_w + 40.0 * s, ch, 16.0 * s);
        let (sx0, sw0) = (sb.x, sb.w);
        for (i, c) in sb.cov.iter_mut().enumerate() {
            if (sx0 + (i % sw0) as i32) as f32 >= cx + side_w {
                *c = 0.0;
            }
        }
        cv.paint(&sb, sidebar_bg);
        cv.fill_rect((cx + side_w) as i32, (cy + 8.0 * s) as i32, 1, (ch - 16.0 * s) as i32, with_alpha(th.border, 0.7));
        let mut pt = Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s };
        let px = pt.px(1.0);
        // ---------- sidebar ----------
        pt.text(cv, "Settings", pt.px(1.2), cx + 22.0 * s, cy + 32.0 * s, th.popup_fg, true);
        let sbtn: R = (cx + side_w - 52.0 * s, cy + 14.0 * s, 36.0 * s, 36.0 * s);
        let si = hits.len();
        if hover == Some(si) || searching {
            cv.fill_circle(sbtn.0 + 18.0 * s, sbtn.1 + 18.0 * s, 18.0 * s, th.hover);
        }
        pt.icon(cv, "edit-find-symbolic", sbtn.0 + 10.0 * s, sbtn.1 + 10.0 * s, 16.0 * s, th.popup_fg);
        hits.push((sbtn, Act::SettingsSearch));
        let mut sy = cy + 64.0 * s;
        if searching {
            let er: R = (cx + 14.0 * s, sy, side_w - 28.0 * s, 36.0 * s);
            cv.fill_rrect(er.0, er.1, er.2, er.3, 10.0 * s, th.card);
            cv.stroke_rrect(er.0, er.1, er.2, er.3, 10.0 * s, 2.0 * s, th.accent);
            let t = if query.is_empty() { "Search settings…".to_string() } else { query.clone() };
            let tw = pt.text_fit(cv, &t, px, er.0 + 12.0 * s, er.1 + er.3 / 2.0, er.2 - 24.0 * s, if query.is_empty() { th.dim_fg } else { th.popup_fg }, false);
            if !query.is_empty() {
                cv.fill_rect((er.0 + 14.0 * s + tw) as i32, (er.1 + 9.0 * s) as i32, 2, (18.0 * s) as i32, th.accent);
            }
            sy += 48.0 * s;
        }
        sy -= side_scroll;
        for grp in SIDE_GROUPS.iter() {
            for &pi in grp.iter() {
                let r: R = (cx + 10.0 * s, sy, side_w - 20.0 * s, 40.0 * s);
                let idx = hits.len();
                let on = page == pi && query.is_empty();
                if on {
                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, if th.dark { with_alpha(0xFFFFFFFF, 0.12) } else { with_alpha(0xFF000000, 0.09) });
                } else if hover == Some(idx) {
                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, with_alpha(th.popup_fg, 0.06));
                }
                if r.1 > cy + 56.0 * s && r.1 + r.3 < cy + ch {
                    pt.icon(cv, PAGES[pi].icon, r.0 + 12.0 * s, r.1 + 10.0 * s, 20.0 * s, th.popup_fg);
                    pt.text(cv, PAGES[pi].name, px, r.0 + 44.0 * s, r.1 + r.3 / 2.0, th.popup_fg, on);
                    hits.push((r, Act::SettingsPage(pi)));
                } else {
                    hits.push((r, Act::Noop));
                }
                sy += 40.0 * s;
            }
            sy += 10.0 * s;
            cv.fill_rect((cx + 20.0 * s) as i32, (sy - 5.0 * s) as i32, (side_w - 40.0 * s) as i32, 1, with_alpha(th.border, 0.5));
        }
        // ---------- header ----------
        let title = if query.is_empty() { PAGES[page].name } else { "Search" };
        let content_w = cw - side_w;
        pt.text_center(cv, title, pt.px(1.1), cx + side_w + content_w / 2.0, cy + 32.0 * s, th.popup_fg, true);
        let cr: R = (cx + cw - 52.0 * s, cy + 14.0 * s, 36.0 * s, 36.0 * s);
        let ci = hits.len();
        cv.fill_circle(cr.0 + 18.0 * s, cr.1 + 18.0 * s, 18.0 * s, if hover == Some(ci) { th.hover } else { with_alpha(th.popup_fg, 0.05) });
        pt.icon(cv, "window-close-symbolic", cr.0 + 10.0 * s, cr.1 + 10.0 * s, 16.0 * s, th.popup_fg);
        hits.push((cr, Act::SettingsClose));
        // ---------- content ----------
        let col_w = (content_w - 72.0 * s).min(680.0 * s);
        let col_x = (content_w - col_w) / 2.0;
        let clip_top = 60.0 * s;
        let clip_h = ch - clip_top - 8.0 * s;
        let mut sub = Canvas::new(content_w as usize, ch as usize);
        let base_idx = hits.len();
        let mut shits: Vec<(R, Act)> = vec![];
        let mut y = clip_top + 8.0 * s - scroll;
        let card_bg = th.card;
        let rb = with_alpha(th.border, 0.8);
        for g in &groups {
            if !g.title.is_empty() {
                pt.text(&mut sub, &g.title, px * 0.95, col_x + 6.0 * s, y + 12.0 * s, th.popup_fg, true);
                y += 28.0 * s;
            }
            if let Some(d) = &g.desc {
                pt.text_fit(&mut sub, d, px * 0.86, col_x + 6.0 * s, y + 6.0 * s, col_w - 12.0 * s, th.dim_fg, false);
                y += 24.0 * s;
            }
            if g.rows.is_empty() {
                continue;
            }
            let card_h: f32 = g.rows.iter().map(|r| row_height(r, s)).sum();
            sub.fill_rrect(col_x, y, col_w, card_h, 12.0 * s, card_bg);
            sub.stroke_rrect(col_x + 0.5, y + 0.5, col_w - 1.0, card_h - 1.0, 12.0 * s, 1.0, rb);
            let mut ry = y;
            for (ri, r) in g.rows.iter().enumerate() {
                let rh = row_height(r, s);
                let rect: R = (col_x, ry, col_w, rh);
                if ri > 0 {
                    sub.fill_rect((col_x + 14.0 * s) as i32, ry as i32, (col_w - 28.0 * s) as i32, 1, with_alpha(th.border, 0.55));
                }
                let right = col_x + col_w - 16.0 * s;
                let mut text_x = col_x + 16.0 * s;
                let clickable = r.act.is_some() || matches!(r.kind, RK::Switch(..));
                let idx = base_idx + shits.len();
                if clickable && hover == Some(idx) {
                    sub.fill_rect((col_x + 2.0 * s) as i32, (ry + 1.0) as i32, (col_w - 4.0 * s) as i32, (rh - 2.0) as i32, with_alpha(th.popup_fg, 0.05));
                }
                if let RK::Scheme(sw, _) = &r.kind {
                    for (k, c) in sw.iter().enumerate() {
                        sub.fill_circle(col_x + 28.0 * s + k as f32 * 13.0 * s, ry + rh / 2.0, 9.0 * s, *c);
                        sub.stroke_rrect(col_x + 19.0 * s + k as f32 * 13.0 * s, ry + rh / 2.0 - 9.0 * s, 18.0 * s, 18.0 * s, 9.0 * s, 1.0, with_alpha(th.border, 0.9));
                    }
                    text_x = col_x + 78.0 * s;
                } else if let Some(ic) = &r.icon {
                    pt.icon(&mut sub, ic, col_x + 16.0 * s, ry + rh / 2.0 - 10.0 * s, 20.0 * s, th.popup_fg);
                    text_x = col_x + 50.0 * s;
                }
                let (ty, sy2) = if r.sub.is_some() { (ry + rh * 0.37, ry + rh * 0.70) } else { (ry + rh / 2.0, 0.0) };
                // measure suffix width so the title doesn't collide
                let suffix_w = match &r.kind {
                    RK::Switch(..) => 56.0 * s,
                    RK::Seg(o) => o.iter().map(|x| pt.text_w(&x.0, px * 0.9, true) + 28.0 * s).sum::<f32>() + 4.0 * s * o.len() as f32,
                    RK::Slider(..) => 290.0 * s,
                    RK::Entry(..) => 270.0 * s,
                    RK::Colors(c) => c.len() as f32 * 34.0 * s,
                    RK::Info(v) | RK::Nav(v) | RK::Combo(_, v, _) => (pt.text_w(v, px * 0.92, false) + 40.0 * s).min(col_w * 0.5),
                    _ => 40.0 * s,
                };
                let title_w = (right - suffix_w - text_x - 8.0 * s).max(80.0 * s);
                if !r.title.is_empty() {
                    pt.text_fit(&mut sub, &r.title, px, text_x, ty, title_w, th.popup_fg, false);
                }
                if let Some(sb) = &r.sub {
                    pt.text_fit(&mut sub, sb, px * 0.84, text_x, sy2, title_w, th.dim_fg, false);
                }
                match &r.kind {
                    RK::Info(v) => {
                        pt.text_fit(&mut sub, v, px * 0.95, right - suffix_w + 8.0 * s, ry + rh / 2.0, suffix_w - 8.0 * s, th.dim_fg, false);
                        if let Some(a) = &r.act {
                            shits.push((rect, a.clone()));
                            continue_row(&mut ry, rh);
                            continue;
                        }
                    }
                    RK::Nav(v) | RK::Combo(_, v, _) => {
                        let open = matches!(&r.kind, RK::Combo(_, _, true));
                        let ic = if matches!(&r.kind, RK::Combo(..)) { if open { "pan-up-symbolic" } else { "pan-down-symbolic" } } else { "go-next-symbolic" };
                        pt.icon(&mut sub, ic, right - 16.0 * s, ry + rh / 2.0 - 8.0 * s, 16.0 * s, th.dim_fg);
                        let vw = pt.text_w(v, px * 0.92, false);
                        pt.text(&mut sub, v, px * 0.92, right - 24.0 * s - vw, ry + rh / 2.0, th.dim_fg, false);
                    }
                    RK::Mark(on) => {
                        if *on {
                            pt.icon(&mut sub, "object-select-symbolic", right - 16.0 * s, ry + rh / 2.0 - 8.0 * s, 16.0 * s, th.accent);
                        }
                    }
                    RK::Scheme(_, on) => {
                        if *on {
                            pt.icon(&mut sub, "object-select-symbolic", right - 16.0 * s, ry + rh / 2.0 - 8.0 * s, 16.0 * s, th.accent);
                        }
                    }
                    RK::Switch(on, id) => {
                        pt.switch(&mut sub, right - 40.0 * s, ry + rh / 2.0, *on, th.accent);
                        shits.push((rect, Act::Setting(*id, if *on { 0 } else { 1 })));
                        continue_row(&mut ry, rh);
                        continue;
                    }
                    RK::Seg(opts) => {
                        let pxs = px * 0.9;
                        let widths: Vec<f32> = opts.iter().map(|o| pt.text_w(&o.0, pxs, true) + 28.0 * s).collect();
                        let total: f32 = widths.iter().sum::<f32>() + 4.0 * s * (opts.len() as f32 - 1.0).max(0.0);
                        let mut bx = right - total;
                        let bh = 32.0 * s;
                        for (i, (lab, act, on)) in opts.iter().enumerate() {
                            let br: R = (bx, ry + (rh - bh) / 2.0, widths[i], bh);
                            let bi = base_idx + shits.len();
                            let base = if *on { th.accent } else { with_alpha(th.popup_fg, if th.dark { 0.10 } else { 0.07 }) };
                            let c = if hover == Some(bi) && !*on { with_alpha(th.popup_fg, 0.16) } else { base };
                            sub.fill_rrect(br.0, br.1, br.2, br.3, bh / 2.0, c);
                            pt.text_center(&mut sub, lab, pxs, br.0 + br.2 / 2.0, br.1 + br.3 / 2.0, if *on { th.accent_fg } else { th.popup_fg }, true);
                            shits.push((br, act.clone()));
                            bx += widths[i] + 4.0 * s;
                        }
                        continue_row(&mut ry, rh);
                        continue;
                    }
                    RK::Slider(id, frac, txt) => {
                        let tw = 56.0 * s;
                        let sr: R = (right - tw - 8.0 * s - 210.0 * s, ry + rh / 2.0 - 16.0 * s, 210.0 * s, 32.0 * s);
                        pt.slider(&mut sub, sr, *frac, th.accent, if th.dark { 0xFF454545 } else { 0xFFD0D0D0 });
                        pt.text_fit(&mut sub, txt, px * 0.88, right - tw, ry + rh / 2.0, tw, th.dim_fg, false);
                        shits.push((sr, Act::Slider(*id)));
                        continue_row(&mut ry, rh);
                        continue;
                    }
                    RK::Entry(id, txt, hint, secret) => {
                        let er: R = (right - 260.0 * s, ry + rh / 2.0 - 18.0 * s, 260.0 * s, 36.0 * s);
                        let editing = edit.as_ref().map_or(false, |e| e.0 == *id) || (*secret && self.qs.pw_active);
                        sub.fill_rrect(er.0, er.1, er.2, er.3, 9.0 * s, with_alpha(th.popup_fg, 0.06));
                        if editing {
                            sub.stroke_rrect(er.0, er.1, er.2, er.3, 9.0 * s, 2.0 * s, th.accent);
                        }
                        let shown = edit.as_ref().filter(|e| e.0 == *id).map(|e| e.1.clone()).unwrap_or_else(|| txt.clone());
                        let tw = pt.text_fit(&mut sub, if shown.is_empty() { hint } else { &shown }, px * 0.95, er.0 + 10.0 * s, er.1 + er.3 / 2.0, er.2 - 20.0 * s, if shown.is_empty() { th.dim_fg } else { th.popup_fg }, false);
                        if editing && !shown.is_empty() {
                            sub.fill_rect((er.0 + 12.0 * s + tw) as i32, (er.1 + 8.0 * s) as i32, 2, (20.0 * s) as i32, th.accent);
                        }
                        if !*secret {
                            shits.push((er, Act::Edit(*id)));
                        }
                        continue_row(&mut ry, rh);
                        continue;
                    }
                    RK::Colors(cols) => {
                        let mut ax = right - cols.len() as f32 * 34.0 * s;
                        for (c, act, on) in cols {
                            let cr: R = (ax, ry + rh / 2.0 - 15.0 * s, 30.0 * s, 30.0 * s);
                            let bi = base_idx + shits.len();
                            sub.fill_circle(cr.0 + 15.0 * s, cr.1 + 15.0 * s, 11.0 * s, *c);
                            if *on {
                                sub.stroke_rrect(cr.0 + 2.0 * s, cr.1 + 2.0 * s, 26.0 * s, 26.0 * s, 13.0 * s, 2.5 * s, th.popup_fg);
                            } else if hover == Some(bi) {
                                sub.stroke_rrect(cr.0 + 2.0 * s, cr.1 + 2.0 * s, 26.0 * s, 26.0 * s, 13.0 * s, 1.5 * s, th.dim_fg);
                            }
                            shits.push((cr, act.clone()));
                            ax += 34.0 * s;
                        }
                        continue_row(&mut ry, rh);
                        continue;
                    }
                }
                shits.push((rect, r.act.clone().unwrap_or(Act::Noop)));
                continue_row(&mut ry, rh);
            }
            y += card_h + 22.0 * s;
        }
        st.max_scroll = (y + scroll - (clip_top + clip_h)).max(0.0);
        if st.scroll > st.max_scroll {
            st.scroll = st.max_scroll;
        }
        // composite content into the card, clipped to the scroll viewport
        for yy in 0..(clip_h as usize) {
            let sy2 = yy + clip_top as usize;
            if sy2 >= sub.h {
                break;
            }
            for xx in 0..sub.w {
                let p = sub.px[sy2 * sub.w + xx];
                if p >> 24 != 0 {
                    let dx = (cx + side_w) as usize + xx;
                    let dy = (cy as usize) + sy2;
                    if dx < cv.w && dy < cv.h {
                        let i = dy * cv.w + dx;
                        let d = cv.px[i];
                        let sa = p >> 24;
                        let ia = 255 - sa;
                        let ch_ = |sv: u32, sh: u32| -> u32 { (((sv >> sh) & 255) + ((d >> sh) & 255) * ia / 255).min(255) };
                        cv.px[i] = (ch_(p, 24) << 24) | (ch_(p, 16) << 16) | (ch_(p, 8) << 8) | ch_(p, 0);
                    }
                }
            }
        }
        for (r, a) in shits {
            let rr: R = (r.0 + cx + side_w, r.1 + cy, r.2, r.3);
            if rr.1 + rr.3 > cy + clip_top && rr.1 < cy + clip_top + clip_h {
                hits.push((rr, a));
            } else {
                hits.push((rr, Act::Noop));
            }
        }
        st.pane.hits = hits;
        let id = st.pane.id;
        let bgc = mix(th.popup_bg, 0xFF000000, 0.5);
        self.p.present(id, &st.pane.cv, bgc);
        self.settings = Some(st);
    }
}

#[inline]
fn continue_row(ry: &mut f32, rh: f32) {
    *ry += rh;
}
