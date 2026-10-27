//! Shell core: state, surface management, event routing, timers.
pub mod bar;
pub mod dock;
pub mod draw;
pub mod grid;
pub mod popup;
pub mod settings;
pub mod shot;
pub mod svcui;

use crate::config::*;
use crate::desktop::{self, App};
use crate::gfx::*;
use crate::icons::Icons;
use crate::jobs::Jobs;
use crate::platform::*;
use crate::sysinfo;
use crate::theme::{self, Theme};
use draw::*;
use std::process::Child;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Debug)]
pub enum Act {
    Noop,
    Activities,
    Clock,
    Status,
    Workspace(usize),
    Dock(usize),
    ShowApps,
    CalPrev,
    CalNext,
    CalToday,
    Slider(u8),
    Tile(u8),
    TileArrow(u8),
    Back,
    WifiNet(usize),
    BtDev(usize),
    Profile(usize),
    PowerMenu,
    Power(u8),
    Settings,
    Lock,
    PwConnect,
    PwCancel,
    MenuItem(usize),
    GridApp(usize),
    GridPage(usize),
    VolMute,
    Workspaces,
    ShotMode(u8),
    ShotCapture,
    ShotClose,
    ShotRecord,
    ShotOpen,
    ShotInstant,
    RecordToggle,
    RecordStop,
    Capture(u8),
    LabwcInstall,
    LabwcRemove,
    SettingsSearch,
    Expand(u16),
    Edit(u16),
    Quit,
    Restart,
    DispSet(usize, u32, u16),
    Tray(usize),
    NotifDismiss(u32),
    NotifAction(u32, usize),
    NotifOpen(u32),
    NotifClear,
    Dnd,
    BannerClose(u32),
    CaffMin(i32),
    GpuMode(usize),
    WsSwitch(usize),
    WsAdd,
    WsRemove,
    Setting(u16, i32),
    SettingsPage(usize),
    SettingsClose,
    OpenQuick(popup::QPage),
    Cmd(String),
}

pub struct Pane {
    pub id: SurfId,
    pub w: i32,
    pub h: i32,
    pub cv: Canvas,
    pub hits: Vec<(R, Act)>,
    pub hover: Option<usize>,
    pub dirty: bool,
}

impl Pane {
    pub fn new(id: SurfId, w: i32, h: i32) -> Pane {
        Pane { id, w, h, cv: Canvas::new(w.max(1) as usize, h.max(1) as usize), hits: vec![], hover: None, dirty: true }
    }
    pub fn hit_at(&self, x: f32, y: f32) -> Option<usize> {
        self.hits.iter().rposition(|(r, a)| *a != Act::Noop && inside(*r, x, y))
    }
}

pub struct Shell {
    pub p: Box<dyn Platform>,
    pub cfg: Config,
    pub th: Theme,
    pub raw: theme::Raw,
    pub s: f32,
    pub tx: TextCtx,
    pub txb: TextCtx,
    pub icons: Icons,
    pub apps: Vec<App>,
    pub jobs: Jobs,
    pub children: Vec<Child>,
    pub bar: Option<Pane>,
    pub dock: Option<Pane>,
    pub hot: Option<SurfId>,
    pub pop: Option<popup::Popup>,
    pub catcher: Option<SurfId>,
    pub grid: Option<grid::Grid>,
    pub settings: Option<settings::Settings>,
    pub shot: Option<shot::ShotUi>,
    pub rec: Option<shot::Recorder>,
    pub shortcuts_ok: usize,
    pub svc: Option<crate::svc::Services>,
    pub banner: Option<svcui::Banner>,
    pub tray_menu: Option<(usize, crate::svc::TMenu, Vec<usize>)>,
    pub tray_anchor: f32,
    pub notif_seen: u32,
    pub display_revert: Option<(String, u32, u16, Instant)>,
    pub caffeine: bool,
    pub caffeine_want: bool,
    pub caffeine_auto: bool,
    pub caffeine_until: Option<Instant>,
    pub bat: sysinfo::Battery,
    pub net: sysinfo::Net,
    pub bl: Option<sysinfo::Backlight>,
    pub qs: popup::Qs,
    pub dock_items: Vec<dock::Item>,
    pub bar_hidden: bool,
    pub dock_hidden: bool,
    pub bar_leave: Option<Instant>,
    pub dock_leave: Option<Instant>,
    pub last_minute: i64,
    pub last_slow: Instant,
    pub last_cfg: Instant,
    pub last_reap: Instant,
    pub last_bt: Instant,
    pub last_light: Instant,
    pub drag: Option<(u8, f32, f32)>,
    pub rebuild: bool,
    pub quit: bool,
    pub alpha: bool,
    pub log: bool,
    pub ctx_menu: Option<dock::MenuTarget>,
    pub toast: Option<(String, Instant)>,
}

fn load_text(family: &str, bold: bool) -> TextCtx {
    let mut fonts = vec![];
    for p in theme::find_fonts(family, bold) {
        if let Ok(d) = std::fs::read(&p) {
            if let Some(f) = crate::ttf::Font::parse(d) {
                fonts.push(f);
                if fonts.len() >= 3 {
                    break;
                }
            }
        }
    }
    TextCtx::new(fonts)
}

impl Shell {
    pub fn new(p: Box<dyn Platform>) -> Shell {
        let cfg = Config::load();
        let raw = theme::read_raw();
        let th = theme::resolve_full(cfg.theme, &raw, cfg.ui_scale, None, &cfg.scheme, &cfg.accent, cfg.bar_follow_theme);
        let tx = load_text(&th.font_family, false);
        let txb = load_text(&th.font_family, true);
        let icons = Icons::new(&th.icon_theme);
        let apps = desktop::scan_apps();
        let now = Instant::now();
        let alpha = p.can_alpha();
        let mut sh = Shell {
            p,
            s: cfg.ui_scale,
            cfg,
            th,
            raw,
            tx,
            txb,
            icons,
            apps,
            jobs: Jobs::new(),
            children: vec![],
            bar: None,
            dock: None,
            hot: None,
            pop: None,
            catcher: None,
            grid: None,
            settings: None,
            shot: None,
            rec: None,
            shortcuts_ok: 0,
            svc: None,
            banner: None,
            tray_menu: None,
            tray_anchor: 0.0,
            notif_seen: 0,
            display_revert: None,
            caffeine: false,
            caffeine_want: false,
            caffeine_auto: false,
            caffeine_until: None,
            bat: sysinfo::battery(),
            net: sysinfo::network(),
            bl: sysinfo::Backlight::find(),
            qs: popup::Qs::new(),
            dock_items: vec![],
            bar_hidden: false,
            dock_hidden: false,
            bar_leave: None,
            dock_leave: None,
            last_minute: -1,
            last_slow: now,
            last_cfg: now,
            last_reap: now,
            last_bt: now,
            last_light: now,
            drag: None,
            rebuild: true,
            quit: false,
            alpha,
            log: std::env::var("LUMEN_DEBUG").is_ok(),
            ctx_menu: None,
            toast: None,
        };
        sh.qs.probe();
        sh.qs.cache_load();
        sh
    }

    /// Orderly shutdown: undo night light, release inhibit, destroy surfaces.
    pub fn shutdown(&mut self) {
        self.p.set_color_temp(6500);
        if let Some(s) = &mut self.svc {
            s.inhibit(false);
            let _ = s.poll();
        }
        self.caffeine_xset(false);
        self.close_popup();
        self.close_grid();
        self.close_settings();
        if let Some(b) = self.banner.take() {
            self.p.destroy(b.pane.id);
        }
        for id in [self.bar.take().map(|b| b.id), self.dock.take().map(|d| d.id), self.hot.take()].into_iter().flatten() {
            self.p.destroy(id);
        }
        self.p.flush();
    }

    pub fn out(&self) -> Output {
        self.p.outputs().first().cloned().unwrap_or(Output { x: 0, y: 0, w: 1280, h: 720, name: String::new(), primary: true })
    }

    pub fn paint(&mut self) -> Paint<'_> {
        Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s: self.s }
    }

    pub fn bar_h(&self) -> i32 {
        (self.cfg.bar_height as f32 * self.s).round() as i32
    }
    pub fn bar_margin(&self) -> i32 {
        if self.cfg.bar_floating && self.alpha { (6.0 * self.s) as i32 } else { 0 }
    }
    /// Offset for the dock when it shares an edge with the bar (X11 stacking).
    pub fn dock_inset(&self) -> i32 {
        let de = if self.cfg.dock_edge == Edge::Top { Edge::Top } else { Edge::Bottom };
        let be = if self.cfg.bar_edge == Edge::Bottom { Edge::Bottom } else { Edge::Top };
        if self.cfg.bar_enabled && de == be && self.cfg.bar_vis == Visibility::Always { self.bar_total() } else { 0 }
    }
    pub fn bar_total(&self) -> i32 {
        self.bar_h() + self.bar_margin() * 2
    }

    // ---------- surfaces ----------
    pub fn build_surfaces(&mut self) {
        self.close_popup();
        self.close_grid();
        let reopen_settings = self.settings.as_ref().map(|s| s.page);
        self.close_settings();
        if let Some(b) = self.banner.take() {
            self.p.destroy(b.pane.id);
        }
        for id in [self.bar.take().map(|b| b.id), self.dock.take().map(|d| d.id), self.hot.take()].into_iter().flatten() {
            self.p.destroy(id);
        }
        self.alpha = self.p.can_alpha();
        let o = self.out();
        if self.cfg.bar_enabled {
            let mut sp = SurfSpec::new(Kind::Panel);
            sp.edge = if self.cfg.bar_edge == Edge::Bottom { Edge::Bottom } else { Edge::Top };
            sp.h = self.bar_total();
            sp.exclusive = if self.cfg.bar_vis == Visibility::AutoHide { 0 } else { sp.h };
            sp.above_fullscreen = self.cfg.bar_over_fullscreen && self.cfg.bar_vis != Visibility::HideInFullscreen;
            let id = self.p.create(&sp);
            self.bar = Some(Pane::new(id, o.w, sp.h));
            self.bar_hidden = self.cfg.bar_vis == Visibility::AutoHide;
        }
        self.refresh_dock_items();
        if self.cfg.dock_enabled {
            let (len, h) = self.dock_size();
            let mut sp = SurfSpec::new(Kind::Panel);
            sp.edge = if self.cfg.dock_edge == Edge::Top { Edge::Top } else { Edge::Bottom };
            sp.h = h;
            sp.len = len;
            sp.exclusive = if self.cfg.dock_vis == Visibility::Always { h } else { 0 };
            sp.inset = self.dock_inset();
            sp.above_fullscreen = self.cfg.dock_over_fullscreen && self.cfg.dock_vis != Visibility::HideInFullscreen;
            let id = self.p.create(&sp);
            self.dock = Some(Pane::new(id, len, h));
            self.dock_hidden = self.cfg.dock_vis == Visibility::AutoHide;
        }
        // hot-edge trigger for auto-hide
        let want_hot_dock = self.cfg.dock_enabled && self.cfg.dock_vis == Visibility::AutoHide;
        if want_hot_dock {
            let mut sp = SurfSpec::new(Kind::Catcher);
            sp.x = 0;
            sp.w = o.w;
            sp.h = 2;
            sp.y = o.h - 2;
            self.hot = Some(self.p.create(&sp));
            if let Some(h) = self.hot {
                self.p.set_visible(h, true);
            }
        }
        if let Some(b) = &self.bar {
            if !self.bar_hidden {
                self.p.set_visible(b.id, true);
            }
        }
        if let Some(d) = &self.dock {
            if !self.dock_hidden {
                self.p.set_visible(d.id, true);
            }
        }
        self.mark_all();
        self.apply_visibility();
        if let Some(pg) = reopen_settings {
            self.open_settings(pg);
        }
    }

    pub fn mark_all(&mut self) {
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
        if let Some(d) = &mut self.dock {
            d.dirty = true;
        }
        if let Some(p) = &mut self.pop {
            p.pane.dirty = true;
        }
        if let Some(g) = &mut self.grid {
            g.pane.dirty = true;
        }
        if let Some(g) = &mut self.settings {
            g.pane.dirty = true;
        }
        if let Some(g) = &mut self.banner {
            g.pane.dirty = true;
        }
    }

    /// Show/hide panels depending on fullscreen state and auto-hide.
    pub fn apply_visibility(&mut self) {
        let fs = self.p.fullscreen_active();
        if let Some(b) = &self.bar {
            let hide_fs = self.cfg.bar_vis == Visibility::HideInFullscreen && fs;
            let vis = !self.bar_hidden && !hide_fs;
            self.p.set_visible(b.id, vis);
        }
        if let Some(d) = &self.dock {
            let hide_fs = self.cfg.dock_vis == Visibility::HideInFullscreen && fs;
            let vis = !self.dock_hidden && !hide_fs;
            self.p.set_visible(d.id, vis);
        }
    }

    pub fn reload_config(&mut self) {
        let old_edge = (self.cfg.bar_edge, self.cfg.dock_edge);
        let _ = old_edge;
        self.cfg = Config::load();
        self.s = self.cfg.ui_scale;
        self.reload_theme();
        self.rebuild = true;
    }

    pub fn reload_theme(&mut self) {
        self.raw = theme::read_raw();
        let th = theme::resolve_full(self.cfg.theme, &self.raw, self.cfg.ui_scale, None, &self.cfg.scheme, &self.cfg.accent, self.cfg.bar_follow_theme);
        let fam_changed = th.font_family != self.th.font_family;
        let icon_changed = th.icon_theme != self.th.icon_theme;
        self.th = th;
        if fam_changed {
            self.tx = load_text(&self.th.font_family, false);
            self.txb = load_text(&self.th.font_family, true);
        }
        if icon_changed {
            let t = self.th.icon_theme.clone();
            self.icons.set_theme(&t);
        }
        self.mark_all();
    }

    pub fn spawn_app(&mut self, idx: usize, action: Option<usize>) {
        if let Some(app) = self.apps.get(idx).cloned() {
            let act = action.and_then(|i| app.actions.get(i).cloned());
            let ok = desktop::launch(&app, act.as_ref(), &mut self.children);
            if !ok {
                self.toast(&format!("Could not start {}", app.name));
            }
        }
    }
    pub fn spawn_cmd(&mut self, argv: &[&str]) {
        use std::os::unix::process::CommandExt;
        if argv.is_empty() || !desktop::which(argv[0]) {
            self.toast(&format!("{} not found", argv.first().unwrap_or(&"command")));
            return;
        }
        let mut c = std::process::Command::new(argv[0]);
        c.args(&argv[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).process_group(0);
        if let Ok(ch) = c.spawn() {
            self.children.push(ch);
        }
    }
    pub fn toast(&mut self, m: &str) {
        self.toast = Some((m.to_string(), Instant::now()));
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
    }

    // ---------- events ----------
    pub fn handle(&mut self, ev: Ev) {
        if self.log {
            eprintln!("ev: {:?}", ev);
        }
        match ev {
            Ev::Pointer { surf, x, y, what } => self.on_pointer(surf, x, y, what),
            Ev::Key { surf, sym, text, ctrl, mods } => self.on_key(surf, sym, &text, ctrl, mods),
            Ev::Shortcut { id } => self.on_shortcut(id),
            Ev::Configure { surf, w, h } => {
                let mut resize = |pane: &mut Pane| {
                    if pane.id == surf && w > 0 && h > 0 && (pane.w != w || pane.h != h) {
                        pane.w = w;
                        pane.h = h;
                        pane.cv = Canvas::new(w as usize, h as usize);
                        pane.dirty = true;
                    }
                };
                if let Some(b) = &mut self.bar {
                    resize(b);
                }
                if let Some(d) = &mut self.dock {
                    resize(d);
                }
                if let Some(p) = &mut self.pop {
                    resize(&mut p.pane);
                }
                if let Some(g) = &mut self.grid {
                    resize(&mut g.pane);
                }
                if let Some(g) = &mut self.settings {
                    resize(&mut g.pane);
                }
                if let Some(g) = &mut self.banner {
                    resize(&mut g.pane);
                }
                if let Some(g) = &mut self.shot {
                    resize(&mut g.pane);
                }
            }
            Ev::Redraw { .. } => self.mark_all(),
            Ev::Closed { surf } => {
                if surf == 0 {
                    self.quit = true;
                }
            }
            Ev::OutputsChanged => self.rebuild = true,
            Ev::WindowsChanged => {
                self.refresh_dock_items_resize();
                if let Some(b) = &mut self.bar {
                    b.dirty = true;
                }
                self.apply_visibility();
            }
            Ev::ThemeHint => self.reload_theme(),
        }
    }

    fn on_pointer(&mut self, surf: SurfId, x: f32, y: f32, what: Ptr) {
        // slider drag capture
        if let Some((sid, x0, x1)) = self.drag {
            match what {
                Ptr::Move => {
                    let f = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                    self.slider_set(sid, f, false);
                    return;
                }
                Ptr::Up(_) => {
                    let f = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                    self.slider_set(sid, f, true);
                    self.drag = None;
                    return;
                }
                _ => {}
            }
        }
        if Some(surf) == self.hot {
            if what == Ptr::Enter || what == Ptr::Move {
                if self.dock_hidden {
                    self.dock_hidden = false;
                    self.apply_visibility();
                }
            }
            return;
        }
        if Some(surf) == self.catcher {
            if let Ptr::Down(_) = what {
                // click-away: dismiss popup; forward to bar if it landed on a widget
                let o = self.out();
                let _ = o;
                let act = self.bar.as_ref().and_then(|b| if y < b.h as f32 { b.hit_at(x, y).map(|i| b.hits[i].1.clone()) } else { None });
                self.close_popup();
                self.close_grid_if_catcher();
                if let Some(a) = act {
                    self.do_act(a, x, y);
                }
            }
            return;
        }
        if self.bar.as_ref().map(|b| b.id) == Some(surf) {
            let (hit, changed) = {
                let b = self.bar.as_mut().unwrap();
                let h = b.hit_at(x, y);
                let ch = match what {
                    Ptr::Leave => b.hover.take().is_some(),
                    _ => {
                        let c = b.hover != h;
                        b.hover = h;
                        c
                    }
                };
                (h, ch)
            };
            if changed {
                self.bar.as_mut().unwrap().dirty = true;
            }
            match what {
                Ptr::Enter => self.bar_leave = None,
                Ptr::Leave => {
                    if self.cfg.bar_vis == Visibility::AutoHide {
                        self.bar_leave = Some(Instant::now());
                    }
                }
                Ptr::Down(b) => {
                    if let Some(i) = hit {
                        let (r, a) = self.bar.as_ref().unwrap().hits[i].clone();
                        match a {
                            Act::Tray(k) => self.tray_click(k, b, r.0 + r.2 / 2.0),
                            _ if b == 1 => self.do_act(a, r.0 + r.2 / 2.0, y),
                            _ => {}
                        }
                    }
                }
                Ptr::Scroll(d) => {
                    if let Some(i) = hit {
                        if let Act::Tray(k) = self.bar.as_ref().unwrap().hits[i].1 {
                            if let Some(sv) = &mut self.svc {
                                sv.tray_scroll(k, d);
                            }
                        }
                        if let Act::Status = self.bar.as_ref().unwrap().hits[i].1 {
                            self.qs.scroll_volume(-d);
                            self.qs.apply_volume(&mut self.jobs);
                            self.bar.as_mut().unwrap().dirty = true;
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if self.shot.as_ref().map(|s| s.pane.id) == Some(surf) {
            self.shot_pointer(x, y, what);
            return;
        }
        if self.banner.as_ref().map(|b| b.pane.id) == Some(surf) {
            self.banner_pointer(x, y, what);
            return;
        }
        if self.dock.as_ref().map(|b| b.id) == Some(surf) {
            self.dock_pointer(x, y, what);
            return;
        }
        if self.pop.as_ref().map(|p| p.pane.id) == Some(surf) {
            self.popup_pointer(x, y, what);
            return;
        }
        if self.grid.as_ref().map(|g| g.pane.id) == Some(surf) {
            self.grid_pointer(x, y, what);
        } else if self.settings.as_ref().map(|g| g.pane.id) == Some(surf) {
            self.settings_pointer(x, y, what);
        }
    }

    pub fn do_act(&mut self, a: Act, x: f32, y: f32) {
        match a {
            Act::Activities | Act::ShowApps => {
                if self.grid.is_some() {
                    self.close_grid();
                } else {
                    self.open_grid();
                }
            }
            Act::Clock => {
                self.toggle_popup(popup::Kind2::Calendar);
                self.mark_notifs_seen();
            }
            Act::Workspaces => self.toggle_popup_at(popup::Kind2::Workspaces, x),
            Act::RecordStop => self.stop_record(),
            Act::Status => self.toggle_popup(popup::Kind2::Quick),
            Act::Workspace(i) => self.p.switch_workspace(i),
            _ => {
                let _ = (x, y);
            }
        }
    }

    fn on_key(&mut self, surf: SurfId, sym: u32, text: &str, ctrl: bool, mods: u8) {
        if self.shot.as_ref().map(|s| s.pane.id) == Some(surf) {
            self.shot_key(sym);
            return;
        }
        if self.settings.as_ref().map(|g| g.pane.id) == Some(surf) {
            if let Some(slot) = self.settings.as_ref().and_then(|s| s.capture) {
                if !(0xffe1..=0xffee).contains(&sym) {
                    if let Some(st) = &mut self.settings {
                        st.capture = None;
                        st.pane.dirty = true;
                    }
                    if sym != 0xff1b {
                        self.capture_accel(slot as usize, sym, mods);
                    }
                }
                return;
            }
            self.settings_key(sym, text);
        } else if self.grid.as_ref().map(|g| g.pane.id) == Some(surf) {
            self.grid_key(sym, text, ctrl);
        } else if self.pop.as_ref().map(|p| p.pane.id) == Some(surf) {
            self.popup_key(sym, text);
        }
    }

    // ---------- timers ----------
    pub fn next_timeout_ms(&self) -> i32 {
        let mut t = 1000i32;
        if self.cfg.clock_seconds {
            t = 500;
        } else {
            let now = sys_now_secs();
            t = t.min(((60 - now % 60) * 1000 + 50).min(1000) as i32);
        }
        if self.bar_leave.is_some() || self.dock_leave.is_some() || self.toast.is_some() {
            t = t.min(150);
        }
        t.max(30)
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.banner_expiry_tick();
        self.record_tick();
        self.display_revert_tick();
        self.caffeine_tick();
        // results from background commands
        for r in self.jobs.poll() {
            self.on_job(r);
        }
        // minute change
        let nowe = crate::sys::now_epoch();
        let minute = if self.cfg.clock_seconds { nowe } else { nowe / 60 };
        if minute != self.last_minute {
            self.last_minute = minute;
            if let Some(b) = &mut self.bar {
                b.dirty = true;
            }
            if let Some(p) = &mut self.pop {
                if p.kind == popup::Kind2::Calendar {
                    p.pane.dirty = true;
                }
            }
        }
        if now.duration_since(self.last_light) > Duration::from_secs(20) {
            self.last_light = now;
            self.qs.refresh_light(&mut self.jobs);
        }
        if now.duration_since(self.last_slow) > Duration::from_secs(10) {
            self.last_slow = now;
            self.qs.cache_save();
            let (b, n) = (sysinfo::battery(), sysinfo::network());
            if b != self.bat || n != self.net {
                self.bat = b;
                self.net = n;
                if let Some(bp) = &mut self.bar {
                    bp.dirty = true;
                }
            }
        }
        if let Some(p) = &self.pop {
            if p.kind == popup::Kind2::Quick && p.page == popup::QPage::Bt && now.duration_since(self.last_bt) > Duration::from_secs(2) {
                self.last_bt = now;
                self.jobs.run("bt_devs", &["bluetoothctl", "devices"], None, 4000);
                self.jobs.run("bt_conn", &["bluetoothctl", "devices", "Connected"], None, 4000);
                self.jobs.run("bt_paired", &["bluetoothctl", "devices", "Paired"], None, 4000);
            }
        }
        if now.duration_since(self.last_cfg) > Duration::from_secs(1) {
            self.last_cfg = now;
            if self.cfg.changed_on_disk() {
                self.reload_config();
            }
        }
        if now.duration_since(self.last_reap) > Duration::from_secs(2) {
            self.last_reap = now;
            self.children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        }
        // auto-hide timers
        if let Some(t) = self.bar_leave {
            if now.duration_since(t) > Duration::from_millis(600) && self.pop.is_none() {
                self.bar_leave = None;
                self.bar_hidden = true;
                self.apply_visibility();
            }
        }
        if let Some(t) = self.dock_leave {
            if now.duration_since(t) > Duration::from_millis(500) && self.pop.is_none() {
                self.dock_leave = None;
                self.dock_hidden = true;
                self.apply_visibility();
            }
        }
        if let Some((_, t)) = &self.toast {
            if now.duration_since(*t) > Duration::from_secs(3) {
                self.toast = None;
                if let Some(b) = &mut self.bar {
                    b.dirty = true;
                }
            }
        }
        if self.rebuild {
            self.rebuild = false;
            self.build_surfaces();
        }
    }

    pub fn render(&mut self) {
        if self.bar.as_ref().map_or(false, |b| b.dirty) {
            self.draw_bar();
        }
        if self.dock.as_ref().map_or(false, |b| b.dirty) {
            self.draw_dock();
        }
        if self.pop.as_ref().map_or(false, |p| p.pane.dirty) {
            self.draw_popup();
        }
        if self.banner.as_ref().map_or(false, |b| b.pane.dirty) {
            self.draw_banner();
        }
        if self.shot.as_ref().map_or(false, |b| b.pane.dirty) {
            self.draw_shot();
        }
        if self.grid.as_ref().map_or(false, |g| g.pane.dirty) {
            self.draw_grid();
        }
        if self.settings.as_ref().map_or(false, |g| g.pane.dirty) {
            self.draw_settings();
        }
        self.p.flush();
    }
}

pub fn sys_now_secs() -> i64 {
    crate::sys::now_epoch()
}
