//! Popups: calendar, quick settings (volume/brightness/wifi/bluetooth/power profile/night light/dark style), menus.
use super::*;
use crate::jobs::{Jobs, JobResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind2 {
    Calendar,
    Quick,
    Menu,
    Workspaces,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum QPage {
    Main,
    Wifi,
    Bt,
    Profile,
    Power,
    Password,
    Gpu,
    Caffeine,
}

#[derive(Clone, Debug)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: i32,
    pub secure: bool,
    pub active: bool,
    pub known: bool,
}
#[derive(Clone, Debug)]
pub struct BtDev {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub paired: bool,
}

pub struct Qs {
    pub vol: Option<f32>,
    pub muted: bool,
    pub wifi: Option<bool>,
    pub wifi_signal: Option<i32>,
    pub ssid: String,
    pub nets: Vec<WifiNet>,
    pub known: Vec<String>,
    pub bt: Option<bool>,
    pub bt_devs: Vec<BtDev>,
    pub profile: Option<String>,
    pub profiles: Vec<String>,
    pub has_nm: bool,
    pub has_bt: bool,
    pub has_pp: bool,
    pub audio: u8, // 0 none, 1 pactl, 2 wpctl
    pub pw_ssid: String,
    pub pw_text: String,
    pub pw_active: bool,
    pub busy: Option<String>,
    pub gpu: u8, // 0 none, 1 supergfxctl, 2 envycontrol
    pub gpu_mode: String,
    pub gpu_modes: Vec<String>,
    pub last_vol_set: Instant,
    pub last_vol_poll: Instant,
    pub last_cache: String,
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or("").trim();
    let l = l.trim_start_matches("Error:").trim();
    l.chars().take(70).collect()
}

fn split_escaped(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(n) = it.next() {
                cur.push(n);
            }
        } else if c == ':' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").ok().filter(|s| !s.is_empty()).map(std::path::PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".cache"))).unwrap_or_else(|| "/tmp".into());
    base.join("lumen/state.ini")
}

impl Qs {
    /// Restore the last known device state so the UI is right on the very first frame.
    pub fn cache_load(&mut self) {
        let txt = match std::fs::read_to_string(cache_path()) {
            Ok(t) => t,
            Err(_) => return,
        };
        for l in txt.lines() {
            let (k, v) = match l.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            match k {
                "wifi" if self.has_nm => self.wifi = Some(v == "on"),
                "ssid" if self.has_nm => self.ssid = v.to_string(),
                "signal" if self.has_nm => self.wifi_signal = v.parse().ok(),
                "bt" if self.has_bt => self.bt = Some(v == "on"),
                "vol" if self.audio != 0 => self.vol = v.parse().ok().filter(|x: &f32| x.is_finite()),
                "muted" if self.audio != 0 => self.muted = v == "1",
                "profile" if self.has_pp => self.profile = Some(v.to_string()),
                "gpu" if self.gpu != 0 => self.gpu_mode = v.to_string(),
                "btdev" if self.has_bt => {
                    let f: Vec<&str> = v.splitn(4, '|').collect();
                    if f.len() == 4 {
                        self.bt_devs.push(BtDev { mac: f[0].to_string(), name: f[3].to_string(), connected: f[1] == "1", paired: f[2] == "1" });
                    }
                }
                _ => {}
            }
        }
    }

    fn cache_text(&self) -> String {
        let mut t = String::new();
        if let Some(w) = self.wifi {
            t += &format!("wifi={}\nssid={}\n", if w { "on" } else { "off" }, self.ssid);
            if let Some(s) = self.wifi_signal {
                t += &format!("signal={}\n", s);
            }
        }
        if let Some(b) = self.bt {
            t += &format!("bt={}\n", if b { "on" } else { "off" });
        }
        for d in self.bt_devs.iter().take(30) {
            t += &format!("btdev={}|{}|{}|{}\n", d.mac, d.connected as u8, d.paired as u8, d.name.replace(['\n', '\r'], " "));
        }
        if let Some(v) = self.vol {
            t += &format!("vol={:.3}\nmuted={}\n", v, self.muted as u8);
        }
        if let Some(p) = &self.profile {
            t += &format!("profile={}\n", p);
        }
        if !self.gpu_mode.is_empty() {
            t += &format!("gpu={}\n", self.gpu_mode);
        }
        t
    }

    /// Write the cache if anything changed since the last save.
    pub fn cache_save(&mut self) {
        let t = self.cache_text();
        if t == self.last_cache {
            return;
        }
        let p = cache_path();
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new("/tmp")));
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, &t).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
        self.last_cache = t;
    }

    /// Cheap periodic refresh of the things shown without opening any popup.
    pub fn refresh_light(&mut self, jobs: &mut Jobs) {
        self.refresh_vol(jobs);
        if self.has_nm {
            jobs.run("wifi_radio", &["nmcli", "radio", "wifi"], None, 4000);
            jobs.run("wifi_active", &["nmcli", "-t", "-f", "ACTIVE,SSID,SIGNAL", "dev", "wifi"], None, 6000);
        }
        if self.has_bt {
            jobs.run("bt_show", &["bluetoothctl", "show"], None, 4000);
            jobs.run("bt_devs", &["bluetoothctl", "devices"], None, 4000);
            jobs.run("bt_conn", &["bluetoothctl", "devices", "Connected"], None, 4000);
            jobs.run("bt_paired", &["bluetoothctl", "devices", "Paired"], None, 4000);
        }
    }

    pub fn new() -> Qs {
        Qs {
            vol: None,
            muted: false,
            wifi: None,
            wifi_signal: None,
            ssid: String::new(),
            nets: vec![],
            known: vec![],
            bt: None,
            bt_devs: vec![],
            profile: None,
            profiles: vec![],
            has_nm: false,
            has_bt: false,
            has_pp: false,
            audio: 0,
            pw_ssid: String::new(),
            pw_text: String::new(),
            pw_active: false,
            busy: None,
            gpu: 0,
            gpu_mode: String::new(),
            gpu_modes: vec![],
            last_vol_set: Instant::now() - Duration::from_secs(5),
            last_vol_poll: Instant::now() - Duration::from_secs(60),
            last_cache: String::new(),
        }
    }
    pub fn probe(&mut self) {
        self.has_nm = desktop::which("nmcli");
        self.has_bt = desktop::which("bluetoothctl");
        self.has_pp = desktop::which("powerprofilesctl");
        self.gpu = if desktop::which("supergfxctl") { 1 } else if desktop::which("envycontrol") { 2 } else { 0 };
        if self.gpu == 2 {
            self.gpu_modes = vec!["integrated".into(), "hybrid".into(), "nvidia".into()];
        }
        self.audio = if desktop::which("pactl") { 1 } else if desktop::which("wpctl") { 2 } else { 0 };
    }
    pub fn refresh_vol(&mut self, jobs: &mut Jobs) {
        self.last_vol_poll = Instant::now();
        match self.audio {
            1 => {
                jobs.run("vol", &["pactl", "get-sink-volume", "@DEFAULT_SINK@"], None, 3000);
                jobs.run("mute", &["pactl", "get-sink-mute", "@DEFAULT_SINK@"], None, 3000);
            }
            2 => jobs.run("wpvol", &["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], None, 3000),
            _ => {}
        }
    }
    pub fn refresh_all(&mut self, jobs: &mut Jobs) {
        self.refresh_vol(jobs);
        if self.has_nm {
            jobs.run("wifi_radio", &["nmcli", "radio", "wifi"], None, 4000);
            jobs.run("wifi_active", &["nmcli", "-t", "-f", "ACTIVE,SSID,SIGNAL", "dev", "wifi"], None, 6000);
            jobs.run("wifi_known", &["nmcli", "-t", "-f", "NAME", "connection", "show"], None, 4000);
        }
        if self.has_bt {
            jobs.run("bt_show", &["bluetoothctl", "show"], None, 4000);
            jobs.run("bt_devs", &["bluetoothctl", "devices"], None, 4000);
            jobs.run("bt_conn", &["bluetoothctl", "devices", "Connected"], None, 4000);
        }
        match self.gpu {
            1 => {
                jobs.run("gpu_get", &["supergfxctl", "-g"], None, 5000);
                jobs.run("gpu_list", &["supergfxctl", "-s"], None, 5000);
            }
            2 => jobs.run("gpu_get", &["envycontrol", "--query"], None, 5000),
            _ => {}
        }
        if self.has_bt {
            jobs.run("bt_paired", &["bluetoothctl", "devices", "Paired"], None, 4000);
        }
        if self.has_pp {
            jobs.run("pp_get", &["powerprofilesctl", "get"], None, 3000);
            jobs.run("pp_list", &["powerprofilesctl", "list"], None, 3000);
        }
    }
    pub fn scroll_volume(&mut self, dir: i32) {
        let v = self.vol.unwrap_or(0.5);
        self.vol = Some((v + dir as f32 * 0.05).clamp(0.0, 1.0));
        self.muted = false;
    }
    pub fn apply_volume(&mut self, jobs: &mut Jobs) {
        let v = self.vol.unwrap_or(0.0);
        self.last_vol_set = Instant::now();
        match self.audio {
            1 => jobs.run("setvol", &["pactl", "set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", (v * 100.0).round() as i32)], None, 3000),
            2 => jobs.run("setvol", &["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", v)], None, 3000),
            _ => {}
        }
    }
}

pub struct Popup {
    pub kind: Kind2,
    pub pane: Pane,
    pub x: i32,
    pub y: i32,
    pub cal: (i32, i32),
    pub page: QPage,
    pub menu: Vec<dock::MenuEntry>,
    pub relayout: bool,
    pub anchor_x: f32,
}

impl Shell {
    pub fn on_job(&mut self, r: JobResult) {
        let q = &mut self.qs;
        let t = r.tag.as_str();
        let (ok_, out_) = (r.ok, r.out.clone());
        let mut bar_dirty = false;
        match t {
            "vol" => {
                if r.ok && q.last_vol_set.elapsed() > Duration::from_millis(900) {
                    if let Some(i) = r.out.find('%') {
                        let digits: String = r.out[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
                        if let Ok(v) = digits.parse::<f32>() {
                            q.vol = Some((v / 100.0).clamp(0.0, 1.5));
                            bar_dirty = true;
                        }
                    }
                }
            }
            "mute" => {
                if r.ok {
                    q.muted = r.out.contains("yes");
                    bar_dirty = true;
                }
            }
            "wpvol" => {
                if r.ok && q.last_vol_set.elapsed() > Duration::from_millis(900) {
                    if let Some(v) = r.out.split_whitespace().nth(1).and_then(|s| s.parse::<f32>().ok()) {
                        q.vol = Some(v.clamp(0.0, 1.5));
                    }
                    q.muted = r.out.contains("MUTED");
                    bar_dirty = true;
                }
            }
            "wifi_radio" => {
                if r.ok {
                    q.wifi = Some(r.out.trim() == "enabled");
                }
            }
            "wifi_active" => {
                if r.ok {
                    q.ssid.clear();
                    q.wifi_signal = None;
                    for l in r.out.lines() {
                        let f = split_escaped(l);
                        if f.len() >= 3 && f[0] == "yes" {
                            q.ssid = f[1].clone();
                            q.wifi_signal = f[2].parse().ok();
                        }
                    }
                    bar_dirty = true;
                }
            }
            "wifi_known" => {
                if r.ok {
                    q.known = r.out.lines().map(|l| l.to_string()).collect();
                }
            }
            "wifi_list" => {
                if r.ok || !r.out.trim().is_empty() {
                    let mut v: Vec<WifiNet> = vec![];
                    for l in r.out.lines() {
                        let f = split_escaped(l);
                        if f.len() < 4 || f[1].is_empty() {
                            continue;
                        }
                        let n = WifiNet { ssid: f[1].clone(), signal: f[2].parse().unwrap_or(0), secure: !f[3].trim().is_empty() && f[3].trim() != "--", active: f[0] == "*", known: q.known.contains(&f[1]) };
                        if let Some(e) = v.iter_mut().find(|e| e.ssid == n.ssid) {
                            if n.signal > e.signal || n.active {
                                *e = n;
                            }
                        } else {
                            v.push(n);
                        }
                    }
                    v.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
                    q.nets = v;
                }
                if q.busy.as_deref() == Some("scan") {
                    q.busy = None;
                }
            }
            "wifi_connect" => {
                q.busy = None;
                if r.ok {
                    q.pw_text.clear();
                    self.toast("Connected");
                } else {
                    let m = first_line(&r.err);
                    self.toast(&format!("Wi-Fi: {}", if m.is_empty() { "connection failed".to_string() } else { m }));
                }
                self.qs.refresh_all(&mut self.jobs);
                self.qs.page_after_connect();
                if let Some(p) = &mut self.pop {
                    p.page = QPage::Wifi;
                    p.relayout = true;
                }
            }
            "bt_show" => {
                if r.ok {
                    q.bt = Some(r.out.lines().any(|l| l.trim() == "Powered: yes"));
                    bar_dirty = true;
                }
            }
            "bt_devs" => {
                if r.ok {
                    let conn: Vec<String> = q.bt_devs.iter().filter(|d| d.connected).map(|d| d.mac.clone()).collect();
                    let self_prev = q.bt_devs.clone();
                    q.bt_devs = r
                        .out
                        .lines()
                        .filter_map(|l| {
                            let mut it = l.splitn(3, ' ');
                            if it.next()? != "Device" {
                                return None;
                            }
                            let mac = it.next()?.to_string();
                            let name = it.next()?.to_string();
                            let paired = self_prev.iter().any(|d| d.mac == mac && d.paired);
                            Some(BtDev { connected: conn.contains(&mac), paired, mac, name })
                        })
                        .collect();
                }
            }
            "bt_paired" => {
                if r.ok {
                    let macs: Vec<String> = r.out.lines().filter_map(|l| l.split_whitespace().nth(1).map(|s| s.to_string())).collect();
                    for d in &mut q.bt_devs {
                        d.paired = macs.contains(&d.mac);
                    }
                }
            }
            "gpu_get" => {
                if r.ok {
                    q.gpu_mode = r.out.trim().trim_matches(|c| c == '[' || c == ']').to_string();
                    bar_dirty = false;
                }
            }
            "gpu_list" => {
                if r.ok {
                    let l: Vec<String> = r.out.trim().trim_matches(|c| c == '[' || c == ']').split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                    if !l.is_empty() {
                        q.gpu_modes = l;
                    }
                }
            }
            "gpu_set" => {
                q.busy = None;
                if r.ok {
                    self.toast("GPU mode changed - log out or reboot to apply");
                } else {
                    let m = first_line(&r.err);
                    self.toast(&format!("GPU switch failed: {}", m));
                }
                self.qs.refresh_all(&mut self.jobs);
            }
            "notify_retry" => {
                if let Some(sv) = &mut self.svc {
                    sv.request_notif_name();
                }
            }
            "wifi_rescan" => {
                q.busy = None;
                self.jobs.run("wifi_list", &["nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "no"], None, 10000);
                self.jobs.run("wifi_active", &["nmcli", "-t", "-f", "ACTIVE,SSID,SIGNAL", "dev", "wifi"], None, 6000);
            }
            "wifi_toggle" | "bt_power" | "pp_set" | "setsink" | "mutetoggle" | "wifi_disc" | "bt_pair" => {
                if !r.ok {
                    let m = first_line(&r.err);
                    self.toast(&format!("{} failed{}", t.replace('_', " "), if m.is_empty() { String::new() } else { format!(": {}", m) }));
                }
                self.qs.refresh_all(&mut self.jobs);
            }
            "bt_conn" => {
                if r.ok {
                    let macs: Vec<String> = r.out.lines().filter_map(|l| l.split_whitespace().nth(1).map(|s| s.to_string())).collect();
                    for d in &mut q.bt_devs {
                        d.connected = macs.contains(&d.mac);
                    }
                }
            }
            "bt_cmd" => {
                q.busy = None;
                if !r.ok {
                    let m = first_line(&r.err);
                    self.toast(&format!("Bluetooth: {}", if m.is_empty() { "action failed".to_string() } else { m }));
                }
                self.qs.refresh_all(&mut self.jobs);
            }
            "pp_get" => {
                if r.ok {
                    q.profile = Some(r.out.trim().to_string());
                }
            }
            "pp_list" => {
                if r.ok {
                    q.profiles = r.out.lines().filter_map(|l| {
                        let l = l.trim().trim_start_matches('*').trim();
                        if l.ends_with(':') && !l.contains(' ') { Some(l.trim_end_matches(':').to_string()) } else { None }
                    }).collect();
                }
            }
            _ => {}
        }
        if bar_dirty {
            if let Some(b) = &mut self.bar {
                b.dirty = true;
            }
        }
        self.on_settings_job(t, ok_, &out_);
        if let Some(p) = &mut self.pop {
            p.pane.dirty = true;
            if matches!(t, "wifi_list" | "bt_devs" | "pp_list" | "wifi_known") {
                p.relayout = true;
            }
        }
    }

    pub fn slider_set(&mut self, id: u8, f: f32, commit: bool) {
        match id {
            0 => {
                self.qs.vol = Some(f);
                self.qs.muted = false;
                if commit || self.qs.last_vol_set.elapsed() > Duration::from_millis(70) {
                    self.qs.apply_volume(&mut self.jobs);
                }
            }
            1 => {
                if let Some(bl) = &self.bl {
                    if !bl.set(f) && commit {
                        self.toast("Brightness: no permission (add user to 'video' group)");
                    }
                }
            }
            _ => self.settings_slider(id, f, commit),
        }
        if let Some(p) = &mut self.pop {
            p.pane.dirty = true;
        }
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
    }

    pub fn close_popup(&mut self) {
        let had_kb = self.pop.as_ref().map_or(false, |p| p.kind == Kind2::Quick);
        if let Some(p) = self.pop.take() {
            self.p.destroy(p.pane.id);
        }
        if let Some(c) = self.catcher.take() {
            self.p.destroy(c);
        }
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
        if had_kb {
            self.restore_focus();
        }
        self.qs.pw_text.clear();
    }

    pub fn restore_focus(&mut self) {
        if let Some(w) = self.p.windows().into_iter().find(|w| w.active) {
            self.p.activate(w.id);
        }
    }

    pub fn toggle_popup_at(&mut self, k: Kind2, ax: f32) {
        if self.pop.as_ref().map(|p| p.kind) == Some(k) {
            self.close_popup();
            return;
        }
        self.close_popup();
        self.open_popup(k, Some((vec![], ax, 0.0)));
    }

    pub fn toggle_popup(&mut self, k: Kind2) {
        if self.pop.as_ref().map(|p| p.kind) == Some(k) {
            self.close_popup();
            return;
        }
        self.close_popup();
        self.open_popup(k, None);
    }

    pub fn open_popup(&mut self, k: Kind2, menu: Option<(Vec<dock::MenuEntry>, f32, f32)>) {
        if self.grid.is_some() && k != Kind2::Menu {
            self.close_grid();
        }
        let o = self.out();
        let s = self.s;
        let pw = match k {
            Kind2::Calendar => if self.svc.is_some() { (700.0 * s).min(o.w as f32 - 16.0) } else { 340.0 * s },
            Kind2::Quick => 384.0 * s,
            Kind2::Menu => 240.0 * s,
            Kind2::Workspaces => (560.0 * s).min(o.w as f32 - 16.0),
        } as i32;
        let mut cat = SurfSpec::new(Kind::Catcher);
        cat.w = 0;
        let cid = self.p.create(&cat);
        self.p.set_visible(cid, true);
        self.catcher = Some(cid);
        let (menu_entries, ax, ay) = match menu {
            Some((m, x, y)) => (m, x, y),
            None => (vec![], 0.0, 0.0),
        };
        let tm = crate::sys::local_time(crate::sys::now_epoch());
        let mut sp = SurfSpec::new(Kind::Popup);
        sp.keyboard = k == Kind2::Quick;
        sp.w = pw;
        sp.h = (200.0 * s) as i32;
        let pane = Pane::new(0, pw, sp.h);
        let mut pop = Popup { kind: k, pane, x: 0, y: 0, cal: (tm.year + 1900, tm.mon + 1), page: QPage::Main, menu: menu_entries, relayout: true, anchor_x: ax };
        let _ = ay;
        // measure content height
        let mut scratch = Canvas::new(pw as usize, (1400.0 * s) as usize);
        let (ch, _) = self.draw_popup_content(&mut scratch, &pop, None);
        let ph = ch.ceil() as i32;
        let gap = 6.0 * s;
        let top = self.bar_total() as f32;
        let bar_bottom = self.cfg.bar_edge == Edge::Bottom;
        let px = match k {
            Kind2::Calendar => ((o.w - pw) / 2).max(0),
            Kind2::Quick => (o.w - pw - (8.0 * s) as i32).max(0),
            Kind2::Menu => (ax as i32 - pw / 2).clamp(4, (o.w - pw - 4).max(4)),
            Kind2::Workspaces => (ax as i32 - 24).clamp(4, (o.w - pw - 4).max(4)),
        };
        let py = match k {
            Kind2::Menu => if ay < o.h as f32 / 2.0 { ay as i32 } else { (ay as i32 - ph).max(4) },
            _ if bar_bottom => (o.h as f32 - top - gap) as i32 - ph,
            _ => (top + gap) as i32,
        };
        sp.x = px;
        sp.y = py;
        sp.h = ph;
        let id = self.p.create(&sp);
        pop.pane = Pane::new(id, pw, ph);
        pop.x = px;
        pop.y = py;
        pop.relayout = false;
        self.p.set_visible(id, true);
        self.pop = Some(pop);
        if k == Kind2::Quick {
            self.qs.refresh_all(&mut self.jobs);
        }
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
    }

    pub fn draw_popup(&mut self) {
        let mut pop = match self.pop.take() {
            Some(p) => p,
            None => return,
        };
        let s = self.s;
        if pop.relayout {
            pop.relayout = false;
            let mut scratch = Canvas::new(pop.pane.w as usize, (1400.0 * s) as usize);
            let (ch, _) = self.draw_popup_content(&mut scratch, &pop, None);
            let nh = ch.ceil() as i32;
            if nh != pop.pane.h {
                let mut sp = SurfSpec::new(Kind::Popup);
                sp.keyboard = pop.kind == Kind2::Quick;
                sp.w = pop.pane.w;
                sp.h = nh;
                sp.x = pop.x;
                sp.y = if pop.kind == Kind2::Menu { pop.y + pop.pane.h - nh } else { pop.y };
                pop.y = sp.y;
                self.p.update(pop.pane.id, &sp);
                pop.pane.w = sp.w;
                pop.pane.h = nh;
                pop.pane.cv = Canvas::new(sp.w as usize, nh as usize);
            }
        }
        pop.pane.dirty = false;
        let mut cv = std::mem::replace(&mut pop.pane.cv, Canvas::new(1, 1));
        cv.clear();
        let hover = pop.pane.hover;
        let (_, hits) = self.draw_popup_content(&mut cv, &pop, hover);
        pop.pane.hits = hits;
        let id = pop.pane.id;
        let bg = self.th.popup_bg;
        self.p.present(id, &cv, bg);
        pop.pane.cv = cv;
        self.pop = Some(pop);
    }

    pub fn popup_pointer(&mut self, x: f32, y: f32, what: Ptr) {
        let (hit, act) = {
            let p = self.pop.as_mut().unwrap();
            let h = p.pane.hit_at(x, y);
            let ch = match what {
                Ptr::Leave => p.pane.hover.take().is_some(),
                _ => {
                    let c = p.pane.hover != h;
                    p.pane.hover = h;
                    c
                }
            };
            if ch {
                p.pane.dirty = true;
            }
            (h, h.map(|i| (p.pane.hits[i].0, p.pane.hits[i].1.clone())))
        };
        let _ = hit;
        match (what, act) {
            (Ptr::Down(1), Some((r, a))) => self.popup_act(a, r, x),
            (Ptr::Scroll(d), Some((_, Act::Slider(0)))) => {
                self.qs.scroll_volume(-d);
                self.qs.apply_volume(&mut self.jobs);
                if let Some(p) = &mut self.pop {
                    p.pane.dirty = true;
                }
                if let Some(b) = &mut self.bar {
                    b.dirty = true;
                }
            }
            _ => {}
        }
    }

    pub fn popup_act(&mut self, a: Act, r: R, x: f32) {
        let s = self.s;
        match a {
            Act::CalPrev | Act::CalNext | Act::CalToday => {
                if let Some(p) = &mut self.pop {
                    let tm = crate::sys::local_time(crate::sys::now_epoch());
                    match a {
                        Act::CalPrev => {
                            p.cal.1 -= 1;
                            if p.cal.1 < 1 {
                                p.cal = (p.cal.0 - 1, 12);
                            }
                        }
                        Act::CalNext => {
                            p.cal.1 += 1;
                            if p.cal.1 > 12 {
                                p.cal = (p.cal.0 + 1, 1);
                            }
                        }
                        _ => p.cal = (tm.year + 1900, tm.mon + 1),
                    }
                    p.pane.dirty = true;
                }
            }
            Act::Slider(id) => {
                let x0 = r.0 + 8.0 * s;
                let x1 = r.0 + r.2 - 8.0 * s;
                self.drag = Some((id, x0, x1));
                let f = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                self.slider_set(id, f, false);
            }
            Act::VolMute => {
                match self.qs.audio {
                    1 => self.jobs.run("mutetoggle", &["pactl", "set-sink-mute", "@DEFAULT_SINK@", "toggle"], None, 3000),
                    2 => self.jobs.run("mutetoggle", &["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"], None, 3000),
                    _ => {}
                }
                self.qs.muted = !self.qs.muted;
                self.mark_all();
            }
            Act::Tile(t) | Act::TileArrow(t) => {
                let arrow = matches!(a, Act::TileArrow(_));
                self.tile_click(t, arrow);
            }
            Act::Back => {
                if let Some(p) = &mut self.pop {
                    p.page = QPage::Main;
                    p.relayout = true;
                    p.pane.dirty = true;
                }
                self.qs.pw_text.clear();
            }
            Act::WifiNet(i) => {
                if let Some(n) = self.qs.nets.get(i).cloned() {
                    if n.active {
                        return;
                    }
                    if n.secure && !n.known {
                        self.qs.pw_ssid = n.ssid.clone();
                        self.qs.pw_text.clear();
                        self.qs.pw_active = true;
                        if let Some(p) = &mut self.pop {
                            p.page = QPage::Password;
                            p.relayout = true;
                            p.pane.dirty = true;
                        }
                    } else {
                        self.qs.busy = Some(format!("Connecting to {}…", n.ssid));
                        self.jobs.run("wifi_connect", &["nmcli", "dev", "wifi", "connect", &n.ssid], None, 40000);
                        self.mark_all();
                    }
                }
            }
            Act::PwConnect => self.submit_password(),
            Act::PwCancel => {
                self.qs.pw_text.clear();
                self.qs.pw_active = false;
                if let Some(p) = &mut self.pop {
                    p.page = QPage::Wifi;
                    p.relayout = true;
                    p.pane.dirty = true;
                }
            }
            Act::BtDev(i) => {
                if let Some(d) = self.qs.bt_devs.get(i).cloned() {
                    self.qs.busy = Some(d.name.clone());
                    if d.paired || d.connected {
                        let cmd = if d.connected { "disconnect" } else { "connect" };
                        self.jobs.run("bt_cmd", &["bluetoothctl", cmd, &d.mac], None, 20000);
                    } else {
                        let sc = format!("bluetoothctl pair {m} && bluetoothctl trust {m} && bluetoothctl connect {m}", m = d.mac);
                        self.jobs.run("bt_cmd", &["sh", "-c", &sc], None, 45000);
                    }
                    self.mark_all();
                }
            }
            Act::Profile(i) => {
                if let Some(n) = self.qs.profiles.get(i).cloned() {
                    self.jobs.run("pp_set", &["powerprofilesctl", "set", &n], None, 4000);
                    self.qs.profile = Some(n);
                    self.mark_all();
                }
            }
            Act::CaffMin(m) => {
                self.close_popup();
                if m < 0 {
                    self.set_caffeine(false, 0);
                } else {
                    self.set_caffeine(true, m);
                }
            }
            Act::NotifDismiss(id) => self.notif_dismiss(id),
            Act::NotifClear => self.notif_clear(),
            Act::Dnd => self.toggle_dnd(),
            Act::NotifOpen(_) | Act::NotifAction(..) | Act::BannerClose(_) => {}
            Act::GpuMode(i) => {
                if let Some(m) = self.qs.gpu_modes.get(i).cloned() {
                    self.qs.busy = Some(format!("Switching to {}…", m));
                    match self.qs.gpu {
                        1 => self.jobs.run("gpu_set", &["supergfxctl", "-m", &m], None, 30000),
                        _ => {
                            let tool = if desktop::which("pkexec") { "pkexec" } else { "sudo" };
                            self.jobs.run("gpu_set", &[tool, "envycontrol", "--switch", &m], None, 120000)
                        }
                    }
                    self.mark_all();
                }
            }
            Act::PowerMenu => {
                if let Some(p) = &mut self.pop {
                    p.page = QPage::Power;
                    p.relayout = true;
                    p.pane.dirty = true;
                }
            }
            Act::Power(n) => {
                self.close_popup();
                match n {
                    0 => self.spawn_cmd(&["systemctl", "suspend"]),
                    1 => self.spawn_cmd(&["systemctl", "reboot"]),
                    2 => self.spawn_cmd(&["systemctl", "poweroff"]),
                    _ => {
                        if desktop::which("xfce4-session-logout") {
                            self.spawn_cmd(&["xfce4-session-logout", "--logout", "--fast"]);
                        } else {
                            self.spawn_cmd(&["loginctl", "terminate-user", &std::env::var("USER").unwrap_or_default()]);
                        }
                    }
                }
            }
            Act::Settings => {
                self.close_popup();
                self.open_settings(0);
            }
            Act::Lock => {
                self.close_popup();
                if desktop::which("xflock4") {
                    self.spawn_cmd(&["xflock4"]);
                } else {
                    self.spawn_cmd(&["loginctl", "lock-session"]);
                }
            }
            Act::MenuItem(i) => self.menu_act(i),
            Act::WsSwitch(i) => {
                self.p.switch_workspace(i);
                self.close_popup();
            }
            Act::WsAdd | Act::WsRemove => {
                let n = self.p.workspaces().0.len().max(1);
                let nn = if a == Act::WsAdd { (n + 1).min(16) } else { n.saturating_sub(1).max(1) };
                self.p.set_workspace_count(nn);
                if let Some(p) = &mut self.pop {
                    p.relayout = true;
                    p.pane.dirty = true;
                }
            }
            Act::ShowApps => {
                self.close_popup();
                self.open_grid();
            }
            Act::Cmd(c) => {
                self.close_popup();
                let v: Vec<&str> = c.split_whitespace().collect();
                self.spawn_cmd(&v);
            }
            _ => {}
        }
    }

    pub fn submit_password(&mut self) {
        let ssid = self.qs.pw_ssid.clone();
        let pw = self.qs.pw_text.clone();
        if pw.len() < 8 {
            self.toast("Password must be at least 8 characters");
            return;
        }
        self.qs.busy = Some(format!("Connecting to {}…", ssid));
        self.jobs.run("wifi_connect", &["nmcli", "dev", "wifi", "connect", &ssid, "password", &pw], None, 40000);
        self.qs.pw_text.clear();
        self.qs.pw_active = false;
        if let Some(p) = &mut self.pop {
            p.page = QPage::Wifi;
            p.relayout = true;
            p.pane.dirty = true;
        }
    }

    pub fn tile_click(&mut self, t: u8, arrow: bool) {
        match t {
            0 => {
                // wifi
                if arrow || self.qs.wifi == Some(false) && false {
                    self.goto_page(QPage::Wifi);
                    self.scan_wifi();
                } else {
                    let on = self.qs.wifi.unwrap_or(true);
                    self.jobs.run("wifi_toggle", &["nmcli", "radio", "wifi", if on { "off" } else { "on" }], None, 5000);
                    self.qs.wifi = Some(!on);
                    self.mark_all();
                }
            }
            1 => {
                if arrow {
                    self.goto_page(QPage::Bt);
                    self.jobs.run("bt_scan", &["bluetoothctl", "--timeout", "6", "scan", "on"], None, 9000);
                } else {
                    let on = self.qs.bt.unwrap_or(false);
                    self.jobs.run("bt_power", &["bluetoothctl", "power", if on { "off" } else { "on" }], None, 5000);
                    self.qs.bt = Some(!on);
                    self.mark_all();
                }
            }
            2 => self.goto_page(QPage::Profile),
            5 => self.goto_page(QPage::Gpu),
            6 => {
                if arrow {
                    self.goto_page(QPage::Caffeine);
                } else {
                    let on = self.caffeine || self.caffeine_want;
                    self.set_caffeine(!on, 0);
                }
            }
            3 => {
                self.cfg.night_on = !self.cfg.night_on;
                self.cfg.save();
                self.cfg.mtime = std::fs::metadata(&self.cfg.path).and_then(|m| m.modified()).ok();
                self.apply_night();
                self.mark_all();
            }
            4 => {
                self.cfg.theme = if self.th.dark { ThemeMode::Light } else { ThemeMode::Dark };
                self.cfg.save();
                self.cfg.mtime = std::fs::metadata(&self.cfg.path).and_then(|m| m.modified()).ok();
                self.reload_theme();
            }
            _ => {}
        }
    }

    pub fn apply_night(&mut self) {
        // Gamma control is backend specific; platforms without support simply ignore it.
        let k = if self.cfg.night_on { self.cfg.night_temp } else { 6500 };
        self.p.set_color_temp(k);
    }

    fn goto_page(&mut self, pg: QPage) {
        if let Some(p) = &mut self.pop {
            p.page = pg;
            p.relayout = true;
            p.pane.dirty = true;
        }
    }
    pub fn scan_wifi(&mut self) {
        self.qs.busy = Some("scan".into());
        // show cached results immediately, then rescan and refresh
        self.jobs.run("wifi_list", &["nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "no"], None, 10000);
        self.jobs.run("wifi_rescan", &["sh", "-c", "nmcli dev wifi rescan 2>/dev/null; sleep 2.5; true"], None, 15000);
    }

    pub fn popup_key(&mut self, sym: u32, text: &str) {
        let page = self.pop.as_ref().map(|p| p.page);
        if sym == 0xff1b {
            if page == Some(QPage::Password) {
                self.qs.pw_text.clear();
                self.goto_page(QPage::Wifi);
            } else {
                self.close_popup();
            }
            return;
        }
        if page == Some(QPage::Password) {
            match sym {
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
            if let Some(p) = &mut self.pop {
                p.pane.dirty = true;
            }
        }
    }
}

impl Qs {
    pub fn page_after_connect(&mut self) {}
}

impl Shell {
    /// Draw popup content into `cv`; returns (content height, hits). Same code path for measuring and drawing.
    pub fn draw_popup_content(&mut self, cv: &mut Canvas, pop: &Popup, hover: Option<usize>) -> (f32, Vec<(R, Act)>) {
        let s = self.s;
        let w = pop.pane.w as f32;
        let th = self.th.clone();
        let qs = &self.qs;
        let bat = self.bat.clone();
        let has_bl = self.bl.is_some();
        let bright = self.bl.as_ref().map(|b| b.get());
        let (night, dark) = (self.cfg.night_on, th.dark);
        let notifs = self.notifs();
        let dnd_on = self.dnd();
        let apps_c = self.apps.clone();
        let caff_on = self.caffeine || self.caffeine_want;
        let caff_left = self.caffeine_until.map(|t| t.saturating_duration_since(Instant::now()).as_secs() / 60 + 1);
        let ws_info = self.p.workspaces();
        let all_wins = self.p.windows();
        let has_ws_cmd = desktop::which("xfce4-workspace-settings");
        let mut hits: Vec<(R, Act)> = vec![];
        let mut pt = Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s };
        let px = pt.px(1.0);
        let small = pt.px(0.85);
        let pad = 16.0 * s;
        let fg = th.popup_fg;
        let dim = th.dim_fg;
        let mut y = pad;

        // helpers via closures can't borrow pt twice; use macros-like inline code
        let mut final_h;
        match pop.kind {
            Kind2::Calendar => {
                let cal_w = 340.0 * s;
                let wide = w > cal_w + 60.0 * s;
                let ox = if wide { w - cal_w } else { 0.0 };
                let h0 = hits.len();
                let mut subcv = Canvas::new(if wide { cal_w as usize } else { 1 }, if wide { cv.h } else { 1 });
                {
                let cv: &mut Canvas = if wide { &mut subcv } else { &mut *cv };
                let w = if wide { cal_w } else { w };
                let tm = crate::sys::local_time(crate::sys::now_epoch());
                let (cy_, cm) = pop.cal;
                let dstr = format!("{}, {} {}", DAYS[tm.wday.rem_euclid(7) as usize], tm.mday, MONTHS[tm.mon.rem_euclid(12) as usize]);
                pt.text(cv, &dstr, px * 1.35, pad, y + 14.0 * s, fg, true);
                y += 40.0 * s;
                let title = format!("{} {}", MONTHS[(cm - 1).clamp(0, 11) as usize], cy_);
                pt.text(cv, &title, px, pad, y + 14.0 * s, fg, true);
                let bw = 32.0 * s;
                for (i, (ic, act)) in [("go-previous-symbolic", Act::CalPrev), ("go-next-symbolic", Act::CalNext)].iter().enumerate() {
                    let r: R = (w - pad - bw * (2 - i) as f32 - 4.0 * s * (1 - i) as f32 * -1.0, y, bw, bw);
                    let r: R = (r.0, r.1, r.2, r.3);
                    let idx = hits.len();
                    if hover == Some(idx) {
                        cv.fill_rrect(r.0, r.1, r.2, r.3, r.3 / 2.0, th.hover);
                    }
                    pt.icon(cv, ic, r.0 + (bw - 16.0 * s) / 2.0, r.1 + (bw - 16.0 * s) / 2.0, 16.0 * s, fg);
                    hits.push((r, act.clone()));
                }
                y += 40.0 * s;
                let cellw = (w - 2.0 * pad) / 7.0;
                for i in 0..7 {
                    pt.text_center(cv, DAYS_S[i], small, pad + cellw * (i as f32 + 0.5), y + 10.0 * s, dim, true);
                }
                y += 26.0 * s;
                let first = weekday(cy_, cm, 1);
                let dim_n = days_in_month(cy_, cm);
                let (ty, tmn) = (tm.year + 1900, tm.mon + 1);
                let rows = (first + dim_n + 6) / 7;
                let rowh = 34.0 * s;
                for d in 1..=dim_n {
                    let slot = first + d - 1;
                    let (col, row) = (slot % 7, slot / 7);
                    let cx = pad + cellw * (col as f32 + 0.5);
                    let cyy = y + rowh * (row as f32 + 0.5);
                    let today = d == tm.mday && cy_ == ty && cm == tmn;
                    if today {
                        pt.th.accent;
                        cv.fill_circle(cx, cyy, 15.0 * s, th.accent);
                    }
                    pt.text_center(cv, &d.to_string(), px, cx, cyy, if today { th.accent_fg } else { fg }, today);
                }
                y += rowh * rows as f32 + 6.0 * s;
                if (cy_, cm) != (ty, tmn) {
                    let r: R = (pad, y, w - 2.0 * pad, 34.0 * s);
                    let idx = hits.len();
                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, if hover == Some(idx) { th.hover } else { th.card });
                    pt.text_center(cv, "Today", px, r.0 + r.2 / 2.0, r.1 + r.3 / 2.0, fg, true);
                    hits.push((r, Act::CalToday));
                    y += 42.0 * s;
                }
                final_h = y + pad - 6.0 * s;
                }
                if wide {
                    cv.draw_canvas(&subcv, ox as i32, 0, 1.0);
                    for hh in hits[h0..].iter_mut() {
                        hh.0 .0 += ox;
                    }
                    let (lx, lw) = (pad, w - cal_w - 2.0 * pad);
                    let mut ly = pad;
                    pt.text(cv, "Notifications", px * 1.1, lx, ly + 14.0 * s, fg, true);
                    if !notifs.is_empty() {
                        let r: R = (lx + lw - 74.0 * s, ly, 74.0 * s, 28.0 * s);
                        let idx = hits.len();
                        cv.fill_rrect(r.0, r.1, r.2, r.3, 14.0 * s, if hover == Some(idx) { th.hover } else { th.card });
                        pt.text_center(cv, "Clear", small, r.0 + r.2 / 2.0, r.1 + r.3 / 2.0, fg, true);
                        hits.push((r, Act::NotifClear));
                    }
                    ly += 44.0 * s;
                    if notifs.is_empty() {
                        pt.icon(cv, "preferences-system-notifications-symbolic", lx + lw / 2.0 - 24.0 * s, ly + 40.0 * s, 48.0 * s, with_alpha(dim, 0.6));
                        pt.text_center(cv, "No Notifications", px, lx + lw / 2.0, ly + 118.0 * s, dim, true);
                        ly += 150.0 * s;
                    } else {
                        for n in notifs.iter().rev().take(4) {
                            let hgt = super::svcui::draw_notif_card(&mut pt, cv, &apps_c, n, lx, ly, lw, &th, false, &mut hits, hover);
                            ly += hgt + 8.0 * s;
                        }
                        if notifs.len() > 4 {
                            pt.text(cv, &format!("+{} more", notifs.len() - 4), small, lx, ly + 10.0 * s, dim, false);
                            ly += 24.0 * s;
                        }
                    }
                    ly = ly.max(final_h - 50.0 * s);
                    let dr: R = (lx, ly, lw, 40.0 * s);
                    pt.text(cv, "Do Not Disturb", px, lx + 4.0 * s, ly + 20.0 * s, fg, false);
                    pt.switch(cv, lx + lw - 44.0 * s, ly + 20.0 * s, dnd_on, th.accent);
                    hits.push((dr, Act::Dnd));
                    ly += 50.0 * s;
                    final_h = final_h.max(ly + pad);
                }
            }
            Kind2::Menu => {
                let ih = 34.0 * s;
                y = 8.0 * s;
                for (i, e) in pop.menu.iter().enumerate() {
                    if e.sep_before && i > 0 {
                        cv.fill_rect((pad / 2.0) as i32, (y + 3.0 * s) as i32, (w - pad) as i32, 1, th.border);
                        y += 7.0 * s;
                    }
                    let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                    let idx = hits.len();
                    if hover == Some(idx) {
                        cv.fill_rrect(r.0, r.1, r.2, r.3, 8.0 * s, th.hover);
                    }
                    pt.text_fit(cv, &e.label, px, r.0 + 10.0 * s, r.1 + ih / 2.0, r.2 - 20.0 * s, if e.enabled { fg } else { dim }, false);
                    hits.push((r, if e.enabled { Act::MenuItem(i) } else { Act::Noop }));
                    y += ih;
                }
                final_h = y + 8.0 * s;
            }
            Kind2::Workspaces => {
                pt.text(cv, "Workspaces", px * 1.2, pad, y + 12.0 * s, fg, true);
                y += 40.0 * s;
                let (cw, ch, gap) = (122.0 * s, 88.0 * s, 10.0 * s);
                let per = (((w - 2.0 * pad + gap) / (cw + gap)).floor() as usize).max(1);
                let n = ws_info.0.len();
                if n == 0 {
                    pt.text(cv, "This compositor does not expose workspaces.", small, pad, y + 14.0 * s, dim, false);
                    y += 40.0 * s;
                }
                let total = if n == 0 { 0 } else { n + 1 };
                for i in 0..total {
                    let (col, row) = (i % per, i / per);
                    let r: R = (pad + col as f32 * (cw + gap), y + row as f32 * (ch + gap), cw, ch);
                    let idx = hits.len();
                    if i == n {
                        cv.fill_rrect(r.0, r.1, r.2, r.3, 14.0 * s, if hover == Some(idx) { th.hover } else { th.card });
                        pt.icon(cv, "list-add-symbolic", r.0 + r.2 / 2.0 - 10.0 * s, r.1 + r.3 / 2.0 - 10.0 * s, 20.0 * s, fg);
                        hits.push((r, Act::WsAdd));
                        continue;
                    }
                    let active = i == ws_info.1;
                    cv.fill_rrect(r.0, r.1, r.2, r.3, 14.0 * s, if hover == Some(idx) { th.hover } else if active { with_alpha(th.accent, 0.25) } else { th.card });
                    if active {
                        cv.stroke_rrect(r.0 + 1.0, r.1 + 1.0, r.2 - 2.0, r.3 - 2.0, 13.0 * s, 2.0 * s, th.accent);
                    }
                    pt.text_fit(cv, &ws_info.0[i], small, r.0 + 12.0 * s, r.1 + 18.0 * s, r.2 - 24.0 * s, fg, true);
                    let here: Vec<&WinInfo> = all_wins.iter().filter(|wi| wi.workspace == i as i32 || (wi.workspace < 0 && active)).collect();
                    for (k, wi) in here.iter().take(4).enumerate() {
                        let ic = desktop::match_app(&self.apps, &wi.app_id).map(|a| a.icon.clone()).unwrap_or_else(|| wi.app_id.to_lowercase());
                        pt.app_icon(cv, &ic, &wi.app_id, r.0 + 12.0 * s + k as f32 * 24.0 * s, r.1 + 34.0 * s, 20.0 * s);
                    }
                    let cnt = format!("{} window{}", here.len(), if here.len() == 1 { "" } else { "s" });
                    pt.text(cv, &cnt, small * 0.9, r.0 + 12.0 * s, r.1 + r.3 - 14.0 * s, dim, false);
                    hits.push((r, Act::WsSwitch(i)));
                }
                if n > 0 {
                    y += ((total + per - 1) / per) as f32 * (ch + gap);
                }
                y += 4.0 * s;
                let mut fx = pad;
                let mut btn = |pt: &mut Paint, cv: &mut Canvas, hits: &mut Vec<(R, Act)>, label: &str, act: Act, fx: &mut f32| {
                    let bw = pt.text_w(label, small, true) + 28.0 * s;
                    let r: R = (*fx, y, bw, 34.0 * s);
                    let idx = hits.len();
                    cv.fill_rrect(r.0, r.1, r.2, r.3, 17.0 * s, if hover == Some(idx) { th.hover } else { th.card });
                    pt.text_center(cv, label, small, r.0 + r.2 / 2.0, r.1 + r.3 / 2.0, fg, true);
                    hits.push((r, act));
                    *fx += bw + 8.0 * s;
                };
                if n > 1 {
                    btn(&mut pt, cv, &mut hits, "Remove last", Act::WsRemove, &mut fx);
                }
                if has_ws_cmd {
                    btn(&mut pt, cv, &mut hits, "Workspace settings", Act::Cmd("xfce4-workspace-settings".into()), &mut fx);
                }
                btn(&mut pt, cv, &mut hits, "Show apps", Act::ShowApps, &mut fx);
                y += 42.0 * s;
                final_h = y + pad - 6.0 * s;
            }
            Kind2::Quick => match pop.page {
                QPage::Main => {
                    // top row: battery + buttons
                    let bh = 36.0 * s;
                    if bat.present {
                        let ic = super::bar::battery_icon(&bat);
                        pt.icon(cv, &ic, pad, y + (bh - 18.0 * s) / 2.0, 18.0 * s, fg);
                        pt.text(cv, &format!("{}%", bat.percent), px, pad + 26.0 * s, y + bh / 2.0, fg, true);
                    }
                    let mut bx = w - pad - bh;
                    for (ic, act) in [("system-shutdown-symbolic", Act::PowerMenu), ("system-lock-screen-symbolic", Act::Lock), ("emblem-system-symbolic", Act::Settings)] {
                        let r: R = (bx, y, bh, bh);
                        let idx = hits.len();
                        cv.fill_circle(r.0 + bh / 2.0, r.1 + bh / 2.0, bh / 2.0, if hover == Some(idx) { th.hover } else { th.card });
                        pt.icon(cv, ic, r.0 + (bh - 16.0 * s) / 2.0, r.1 + (bh - 16.0 * s) / 2.0, 16.0 * s, fg);
                        hits.push((r, act));
                        bx -= bh + 8.0 * s;
                    }
                    y += bh + 12.0 * s;
                    // sliders
                    let sl = |pt: &mut Paint, cv: &mut Canvas, hits: &mut Vec<(R, Act)>, y: &mut f32, icon: &str, v: f32, id: u8, mute: bool| {
                        let rh = 36.0 * s;
                        let ir: R = (pad, *y, 32.0 * s, rh);
                        if mute {
                            hits.push((ir, Act::VolMute));
                        }
                        pt.icon(cv, icon, pad + 4.0 * s, *y + (rh - 18.0 * s) / 2.0, 18.0 * s, fg);
                        let sr: R = (pad + 40.0 * s, *y, w - 2.0 * pad - 40.0 * s, rh);
                        pt.slider(cv, sr, v, th.accent, if dark { 0xFF454545 } else { 0xFFD0D0D0 });
                        hits.push((sr, Act::Slider(id)));
                        *y += rh + 4.0 * s;
                    };
                    if qs.audio != 0 {
                        sl(&mut pt, cv, &mut hits, &mut y, super::bar::volume_icon(qs.vol, qs.muted), if qs.muted { 0.0 } else { qs.vol.unwrap_or(0.0).min(1.0) }, 0, true);
                    }
                    if has_bl {
                        sl(&mut pt, cv, &mut hits, &mut y, "display-brightness-symbolic", bright.unwrap_or(1.0), 1, false);
                    }
                    y += 8.0 * s;
                    // tiles
                    struct T {
                        id: u8,
                        icon: &'static str,
                        title: String,
                        sub: String,
                        on: bool,
                        arrow: bool,
                    }
                    let mut tiles: Vec<T> = vec![];
                    if qs.has_nm {
                        tiles.push(T { id: 0, icon: if qs.wifi == Some(false) { "network-wireless-offline-symbolic" } else { "network-wireless-symbolic" }, title: "Wi-Fi".into(), sub: if qs.wifi == Some(false) { "Off".into() } else if qs.ssid.is_empty() { "Not connected".into() } else { qs.ssid.clone() }, on: qs.wifi == Some(true), arrow: true });
                    }
                    if qs.has_bt {
                        let n = qs.bt_devs.iter().filter(|d| d.connected).count();
                        tiles.push(T { id: 1, icon: "bluetooth-active-symbolic", title: "Bluetooth".into(), sub: if qs.bt != Some(true) { "Off".into() } else if n > 0 { format!("{} connected", n) } else { "On".into() }, on: qs.bt == Some(true), arrow: true });
                    }
                    if qs.has_pp {
                        let p = qs.profile.clone().unwrap_or_default();
                        let picon = match p.as_str() {
                            "power-saver" => "power-profile-power-saver-symbolic",
                            "performance" => "power-profile-performance-symbolic",
                            _ => "power-profile-balanced-symbolic",
                        };
                        tiles.push(T { id: 2, icon: picon, title: "Power".into(), sub: pretty_profile(&p), on: !p.is_empty() && p != "balanced", arrow: true });
                    }
                    if qs.gpu != 0 && !qs.gpu_mode.is_empty() {
                        let gm = qs.gpu_mode.to_lowercase();
                        tiles.push(T { id: 5, icon: "video-display-symbolic", title: "GPU".into(), sub: qs.gpu_mode.clone(), on: gm.contains("nvidia") || gm.contains("dedicated") || gm.contains("discrete") || gm.contains("mux"), arrow: true });
                    }
                    tiles.push(T { id: 3, icon: "night-light-symbolic", title: "Night Light".into(), sub: if night { "On".into() } else { "Off".into() }, on: night, arrow: false });
                    tiles.push(T { id: 6, icon: "@cup", title: "Caffeine".into(), sub: if !caff_on { "Off".into() } else if let Some(m) = caff_left { format!("{} min left", m) } else { "On".into() }, on: caff_on, arrow: true });
                    tiles.push(T { id: 4, icon: "dark-mode-symbolic", title: "Dark Style".into(), sub: if dark { "On".into() } else { "Off".into() }, on: dark, arrow: false });
                    let gap = 8.0 * s;
                    let tw = (w - 2.0 * pad - gap) / 2.0;
                    let thh = 54.0 * s;
                    for (i, t) in tiles.iter().enumerate() {
                        let (col, row) = (i % 2, i / 2);
                        let r: R = (pad + col as f32 * (tw + gap), y + row as f32 * (thh + gap), tw, thh);
                        let idx = hits.len();
                        let base = if t.on { th.accent } else { th.card };
                        let col_ = if hover == Some(idx) || (t.arrow && matches!(hover.and_then(|h| hits.get(h)).map(|x| &x.1), Some(Act::TileArrow(k)) if *k == t.id)) { mix(base, if t.on { 0xFFFFFFFF } else { fg }, 0.12) } else { base };
                        cv.fill_rrect(r.0, r.1, r.2, r.3, thh / 2.0, col_);
                        let tfg = if t.on { th.accent_fg } else { fg };
                        pt.icon(cv, t.icon, r.0 + 16.0 * s, r.1 + (thh - 16.0 * s) / 2.0, 16.0 * s, tfg);
                        let avail = tw - 42.0 * s - if t.arrow { 34.0 * s } else { 14.0 * s };
                        pt.text_fit(cv, &t.title, px, r.0 + 42.0 * s, r.1 + thh * 0.36, avail, tfg, true);
                        pt.text_fit(cv, &t.sub, small, r.0 + 42.0 * s, r.1 + thh * 0.68, avail, with_alpha(tfg, 0.78), false);
                        if t.arrow {
                            let ar: R = (r.0 + r.2 - 40.0 * s, r.1, 40.0 * s, thh);
                            pt.icon(cv, "go-next-symbolic", ar.0 + 10.0 * s, r.1 + (thh - 16.0 * s) / 2.0, 16.0 * s, tfg);
                            hits.push((r, Act::Tile(t.id)));
                            hits.push((ar, Act::TileArrow(t.id)));
                        } else {
                            hits.push((r, Act::Tile(t.id)));
                        }
                    }
                    let rows = (tiles.len() + 1) / 2;
                    y += rows as f32 * (thh + gap);
                    final_h = y + pad - gap;
                }
                QPage::Wifi | QPage::Bt | QPage::Profile | QPage::Power | QPage::Password | QPage::Gpu | QPage::Caffeine => {
                    let title = match pop.page {
                        QPage::Wifi => "Wi-Fi",
                        QPage::Bt => "Bluetooth",
                        QPage::Profile => "Power Mode",
                        QPage::Gpu => "GPU",
                        QPage::Caffeine => "Keep Awake",
                        QPage::Power => "Power Off / Log Out",
                        _ => "Wi-Fi Password",
                    };
                    let r: R = (pad - 6.0 * s, y, 34.0 * s, 34.0 * s);
                    let idx = hits.len();
                    if hover == Some(idx) {
                        cv.fill_circle(r.0 + r.2 / 2.0, r.1 + r.3 / 2.0, r.2 / 2.0, th.hover);
                    }
                    pt.icon(cv, "go-previous-symbolic", r.0 + 9.0 * s, r.1 + 9.0 * s, 16.0 * s, fg);
                    hits.push((r, if pop.page == QPage::Password { Act::PwCancel } else { Act::Back }));
                    pt.text(cv, title, px * 1.1, pad + 34.0 * s, y + 17.0 * s, fg, true);
                    let sw_state = match pop.page {
                        QPage::Wifi => Some((qs.wifi == Some(true), 0u8)),
                        QPage::Bt => Some((qs.bt == Some(true), 1u8)),
                        _ => None,
                    };
                    if let Some((on, tid)) = sw_state {
                        let sr: R = (w - pad - 44.0 * s, y, 44.0 * s, 34.0 * s);
                        pt.switch(cv, sr.0, y + 17.0 * s, on, th.accent);
                        hits.push((sr, Act::Tile(tid)));
                    }
                    y += 46.0 * s;
                    cv.fill_rect(pad as i32, y as i32, (w - 2.0 * pad) as i32, 1, th.border);
                    y += 8.0 * s;
                    let ih = 44.0 * s;
                    let busy = qs.busy.clone();
                    if let Some(b) = busy.as_ref().filter(|b| b.as_str() != "scan") {
                        pt.text(cv, b, small, pad, y + 12.0 * s, dim, false);
                        y += 26.0 * s;
                    }
                    match pop.page {
                        QPage::Wifi => {
                            if qs.busy.as_deref() == Some("scan") && qs.nets.is_empty() {
                                pt.text(cv, "Scanning…", px, pad, y + ih / 2.0, dim, false);
                                y += ih;
                            }
                            for (i, n) in qs.nets.iter().take(9).enumerate() {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                if hover == Some(idx) {
                                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, th.hover);
                                }
                                let ic = super::bar::net_icon(&crate::sysinfo::Net::Wifi { up: true }, Some(n.signal));
                                pt.icon(cv, ic, r.0 + 12.0 * s, r.1 + (ih - 18.0 * s) / 2.0, 18.0 * s, fg);
                                pt.text_fit(cv, &n.ssid, px, r.0 + 42.0 * s, r.1 + ih / 2.0, r.2 - 42.0 * s - 56.0 * s, fg, n.active);
                                if n.active {
                                    pt.icon(cv, "object-select-symbolic", r.0 + r.2 - 34.0 * s, r.1 + (ih - 16.0 * s) / 2.0, 16.0 * s, fg);
                                } else if n.secure {
                                    pt.icon(cv, "channel-secure-symbolic", r.0 + r.2 - 34.0 * s, r.1 + (ih - 16.0 * s) / 2.0, 16.0 * s, dim);
                                }
                                hits.push((r, Act::WifiNet(i)));
                                y += ih;
                            }
                            if qs.nets.is_empty() && qs.busy.as_deref() != Some("scan") {
                                pt.text(cv, "No networks found", px, pad, y + ih / 2.0, dim, false);
                                y += ih;
                            }
                        }
                        QPage::Bt => {
                            if qs.bt != Some(true) {
                                pt.text(cv, "Bluetooth is off", px, pad, y + ih / 2.0, dim, false);
                                y += ih;
                            }
                            for (i, d) in qs.bt_devs.iter().take(9).enumerate() {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                if hover == Some(idx) {
                                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, th.hover);
                                }
                                pt.icon(cv, "bluetooth-active-symbolic", r.0 + 12.0 * s, r.1 + (ih - 18.0 * s) / 2.0, 18.0 * s, fg);
                                pt.text_fit(cv, &d.name, px, r.0 + 42.0 * s, r.1 + ih / 2.0, r.2 - 42.0 * s - 70.0 * s, fg, d.connected);
                                if d.connected {
                                    pt.text(cv, "Connected", small, r.0 + r.2 - 80.0 * s, r.1 + ih / 2.0, dim, false);
                                } else if !d.paired {
                                    pt.text(cv, "Pair", small, r.0 + r.2 - 44.0 * s, r.1 + ih / 2.0, th.accent, true);
                                }
                                hits.push((r, Act::BtDev(i)));
                                y += ih;
                            }
                        }
                        QPage::Caffeine => {
                            for (label, mins) in [("Until turned off", 0), ("5 minutes", 5), ("15 minutes", 15), ("30 minutes", 30), ("1 hour", 60), ("2 hours", 120)] {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                if hover == Some(idx) {
                                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, th.hover);
                                }
                                pt.text(cv, label, px, r.0 + 14.0 * s, r.1 + ih / 2.0, fg, false);
                                hits.push((r, Act::CaffMin(mins)));
                                y += ih;
                            }
                            if caff_on {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, if hover == Some(idx) { th.hover } else { th.card });
                                pt.text_center(cv, "Turn off", px, r.0 + r.2 / 2.0, r.1 + ih / 2.0, fg, true);
                                hits.push((r, Act::CaffMin(-1)));
                                y += ih;
                            }
                        }
                        QPage::Gpu => {
                            for (i, m) in qs.gpu_modes.iter().enumerate() {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                if hover == Some(idx) {
                                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, th.hover);
                                }
                                let cur = qs.gpu_mode.eq_ignore_ascii_case(m);
                                pt.text(cv, m, px, r.0 + 14.0 * s, r.1 + ih / 2.0, fg, cur);
                                if cur {
                                    pt.icon(cv, "object-select-symbolic", r.0 + r.2 - 34.0 * s, r.1 + (ih - 16.0 * s) / 2.0, 16.0 * s, fg);
                                }
                                hits.push((r, Act::GpuMode(i)));
                                y += ih;
                            }
                            pt.text(cv, "A log out or reboot is needed to apply.", small, pad, y + 12.0 * s, dim, false);
                            y += 28.0 * s;
                        }
                        QPage::Profile => {
                            for (i, n) in qs.profiles.iter().enumerate() {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                if hover == Some(idx) {
                                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, th.hover);
                                }
                                pt.text(cv, &pretty_profile(n), px, r.0 + 14.0 * s, r.1 + ih / 2.0, fg, qs.profile.as_deref() == Some(n.as_str()));
                                if qs.profile.as_deref() == Some(n.as_str()) {
                                    pt.icon(cv, "object-select-symbolic", r.0 + r.2 - 34.0 * s, r.1 + (ih - 16.0 * s) / 2.0, 16.0 * s, fg);
                                }
                                hits.push((r, Act::Profile(i)));
                                y += ih;
                            }
                        }
                        QPage::Power => {
                            for (i, (ic, label)) in [("media-playback-pause-symbolic", "Suspend"), ("view-refresh-symbolic", "Restart…"), ("system-shutdown-symbolic", "Power Off…"), ("system-log-out-symbolic", "Log Out…")].iter().enumerate() {
                                let r: R = (6.0 * s, y, w - 12.0 * s, ih);
                                let idx = hits.len();
                                if hover == Some(idx) {
                                    cv.fill_rrect(r.0, r.1, r.2, r.3, 10.0 * s, th.hover);
                                }
                                pt.icon(cv, ic, r.0 + 12.0 * s, r.1 + (ih - 18.0 * s) / 2.0, 18.0 * s, fg);
                                pt.text(cv, label, px, r.0 + 42.0 * s, r.1 + ih / 2.0, fg, false);
                                hits.push((r, Act::Power(i as u8)));
                                y += ih;
                            }
                        }
                        _ => {
                            pt.text_fit(cv, &format!("Enter the password for “{}”", qs.pw_ssid), small, pad, y + 10.0 * s, w - 2.0 * pad, dim, false);
                            y += 28.0 * s;
                            let fr: R = (pad, y, w - 2.0 * pad, 40.0 * s);
                            cv.fill_rrect(fr.0, fr.1, fr.2, fr.3, 10.0 * s, th.card);
                            cv.stroke_rrect(fr.0, fr.1, fr.2, fr.3, 10.0 * s, 2.0 * s, th.accent);
                            let dots: String = "•".repeat(qs.pw_text.chars().count().min(40));
                            pt.text(cv, &dots, px, fr.0 + 12.0 * s, fr.1 + fr.3 / 2.0, fg, false);
                            y += 52.0 * s;
                            let br: R = (pad, y, w - 2.0 * pad, 40.0 * s);
                            let idx = hits.len();
                            cv.fill_rrect(br.0, br.1, br.2, br.3, 20.0 * s, if hover == Some(idx) { mix(th.accent, 0xFFFFFFFF, 0.15) } else { th.accent });
                            pt.text_center(cv, "Connect", px, br.0 + br.2 / 2.0, br.1 + br.3 / 2.0, th.accent_fg, true);
                            hits.push((br, Act::PwConnect));
                            y += 48.0 * s;
                        }
                    }
                    final_h = y + 10.0 * s;
                }
            },
        }
        // frame (drawn last so it sits above content edges; background drawn first via compositing below)
        let h = final_h.max(40.0 * s);
        final_h = h;
        // background pass: draw behind by compositing a bg canvas underneath
        if cv.h >= h as usize {
            let mut bgc = Canvas::new(cv.w, cv.h);
            let radius = if pop.kind == Kind2::Menu { 12.0 * s } else { 20.0 * s };
            if self.alpha {
                bgc.fill_rrect(0.5, 0.5, w - 1.0, h - 1.0, radius, th.popup_bg);
                bgc.stroke_rrect(0.5, 0.5, w - 1.0, h - 1.0, radius, 1.0, with_alpha(th.border, 0.9));
            } else {
                bgc.fill_rect(0, 0, w as i32, h as i32, th.popup_bg);
                bgc.stroke_rrect(0.5, 0.5, w - 1.0, h - 1.0, 2.0, 1.0, th.border);
            }
            bgc.draw_canvas(cv, 0, 0, 1.0);
            *cv = bgc;
        }
        (final_h, hits)
    }
}

fn pretty_profile(p: &str) -> String {
    match p {
        "power-saver" => "Power Saver".into(),
        "balanced" => "Balanced".into(),
        "performance" => "Performance".into(),
        "" => "—".into(),
        o => o.to_string(),
    }
}
