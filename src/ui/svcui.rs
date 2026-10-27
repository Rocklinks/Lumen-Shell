//! UI for the D-Bus services: notification banners + cards, tray icons/menus, Keep Awake (Caffeine).
use super::*;
use crate::svc::{Effect, Notif, Services, TMenu};

pub struct Banner {
    pub pane: Pane,
    pub ids: Vec<u32>,
    pub y: i32,
}

pub fn rel_time(t: i64) -> String {
    let d = (crate::sys::now_epoch() - t).max(0);
    if d < 60 {
        "now".into()
    } else if d < 3600 {
        format!("{}m", d / 60)
    } else if d < 86400 {
        format!("{}h", d / 3600)
    } else {
        format!("{}d", d / 86400)
    }
}

impl Shell {
    pub fn init_services(&mut self) {
        match Services::start(self.cfg.notify_takeover) {
            Ok(s) => self.svc = Some(s),
            Err(e) => {
                if self.log {
                    eprintln!("lumen: no session bus services: {}", e);
                }
                self.svc = None;
            }
        }
    }

    pub fn svc_fd(&self) -> i32 {
        self.svc.as_ref().map_or(-1, |s| s.fd())
    }

    pub fn svc_poll(&mut self) {
        let fx = match self.svc.as_mut() {
            Some(s) => match s.poll() {
                Ok(f) => f,
                Err(e) => {
                    if self.log {
                        eprintln!("lumen: bus lost: {}", e);
                    }
                    self.svc = None;
                    if let Some(b) = &mut self.bar {
                        b.dirty = true;
                    }
                    return;
                }
            },
            None => return,
        };
        for e in fx {
            match e {
                Effect::NotifNew(_) | Effect::NotifGone(_) => {
                    self.update_banner();
                    if let Some(b) = &mut self.bar {
                        b.dirty = true;
                    }
                    if let Some(p) = &mut self.pop {
                        if p.kind == popup::Kind2::Calendar {
                            p.relayout = true;
                            p.pane.dirty = true;
                        }
                    }
                }
                Effect::Tray => {
                    let dirs: Vec<String> = self.svc.as_ref().map(|s| s.items.iter().map(|i| i.theme_path.clone()).collect()).unwrap_or_default();
                    for d in dirs {
                        self.icons.add_dir(&d);
                    }
                    if let Some(b) = &mut self.bar {
                        b.dirty = true;
                    }
                }
                Effect::MenuLoaded(i, root) => {
                    self.tray_menu = Some((i, root, vec![]));
                    self.show_tray_menu();
                }
                Effect::Inhibited(on) => {
                    self.caffeine = on;
                    if !on && self.caffeine_want {
                        // no screensaver service: fall back to xset on X11
                        self.caffeine_xset(true);
                    }
                    self.mark_all();
                }
                Effect::NameTaken => {
                    if self.cfg.notify_takeover {
                        self.jobs.run("notify_retry", &["sh", "-c", "pkill -x xfce4-notifyd; pkill -x dunst; pkill -x mako; pkill -x swaync; sleep 0.8; true"], None, 6000);
                    }
                }
                Effect::Error(m) => self.toast(&m),
            }
        }
    }

    // ---------------- Keep Awake ----------------
    pub fn set_caffeine(&mut self, on: bool, minutes: i32) {
        self.caffeine_want = on;
        self.caffeine_until = if on && minutes > 0 { Some(Instant::now() + Duration::from_secs(minutes as u64 * 60)) } else { None };
        if let Some(s) = &mut self.svc {
            s.inhibit(on);
        } else if on {
            self.caffeine_xset(true);
            self.caffeine = true;
        }
        if !on {
            self.caffeine = false;
            self.caffeine_xset(false);
        }
        self.mark_all();
    }

    pub fn caffeine_xset(&mut self, on: bool) {
        if std::env::var("DISPLAY").is_ok() && desktop::which("xset") {
            if on {
                self.spawn_cmd(&["xset", "s", "off", "-dpms"]);
                self.caffeine = true;
            } else {
                self.spawn_cmd(&["xset", "s", "on", "+dpms"]);
            }
        }
    }

    pub fn caffeine_tick(&mut self) {
        if let Some(t) = self.caffeine_until {
            if Instant::now() >= t {
                self.set_caffeine(false, 0);
            }
        }
        // auto keep-awake while a fullscreen window is active (like the Caffeine extension option)
        if self.cfg.caffeine_fullscreen {
            let fs = self.p.fullscreen_active();
            if fs && !self.caffeine_want && !self.caffeine_auto {
                self.caffeine_auto = true;
                self.set_caffeine(true, 0);
            } else if !fs && self.caffeine_auto {
                self.caffeine_auto = false;
                self.set_caffeine(false, 0);
            }
        }
    }

    // ---------------- notifications ----------------
    pub fn notifs(&self) -> Vec<Notif> {
        self.svc.as_ref().map(|s| s.notifs.clone()).unwrap_or_default()
    }

    pub fn dnd(&self) -> bool {
        self.svc.as_ref().map_or(false, |s| s.dnd)
    }

    pub fn unseen_notifs(&self) -> bool {
        self.svc.as_ref().map_or(false, |s| s.notifs.iter().any(|n| n.id > self.notif_seen))
    }

    pub fn mark_notifs_seen(&mut self) {
        if let Some(s) = &self.svc {
            self.notif_seen = s.notifs.iter().map(|n| n.id).max().unwrap_or(self.notif_seen).max(self.notif_seen);
        }
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
    }

    pub fn toggle_dnd(&mut self) {
        if let Some(s) = &mut self.svc {
            s.dnd = !s.dnd;
            if s.dnd {
                for n in s.notifs.iter_mut() {
                    if n.urgency < 2 {
                        n.banner_until = None;
                    }
                }
            }
        }
        self.update_banner();
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
        if let Some(p) = &mut self.pop {
            p.pane.dirty = true;
        }
    }

    pub fn notif_dismiss(&mut self, id: u32) {
        if let Some(s) = &mut self.svc {
            s.close(id, 2);
        }
        self.update_banner();
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
        if let Some(p) = &mut self.pop {
            p.relayout = true;
            p.pane.dirty = true;
        }
    }

    pub fn notif_action(&mut self, id: u32, key: &str) {
        if self.local_notif_action(id, key) {
            if let Some(b) = &mut self.bar {
                b.dirty = true;
            }
            return;
        }
        let resident = false;
        if let Some(s) = &mut self.svc {
            s.invoke(id, key);
            if !resident {
                s.close(id, 2);
            }
        }
        self.update_banner();
        if let Some(p) = &mut self.pop {
            p.relayout = true;
            p.pane.dirty = true;
        }
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
    }

    pub fn notif_clear(&mut self) {
        if let Some(s) = &mut self.svc {
            s.clear_all();
        }
        self.update_banner();
        if let Some(p) = &mut self.pop {
            p.relayout = true;
            p.pane.dirty = true;
        }
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
    }

    pub fn banner_expiry_tick(&mut self) {
        let now = Instant::now();
        let mut changed = false;
        if let Some(s) = &mut self.svc {
            let mut drop_ids = vec![];
            for n in s.notifs.iter_mut() {
                if let Some(t) = n.banner_until {
                    if now >= t {
                        n.banner_until = None;
                        changed = true;
                        if n.transient {
                            drop_ids.push(n.id);
                        }
                    }
                }
            }
            for id in drop_ids {
                s.close(id, 1);
            }
        }
        if changed {
            self.update_banner();
            if let Some(b) = &mut self.bar {
                b.dirty = true;
            }
        }
    }

    fn banner_ids(&self) -> Vec<u32> {
        if !self.cfg.notify_banners {
            return vec![];
        }
        let now = Instant::now();
        let mut v: Vec<&Notif> = self.svc.as_ref().map(|s| s.notifs.iter().filter(|n| n.banner_until.map_or(false, |t| t > now)).collect()).unwrap_or_default();
        v.sort_by(|a, b| b.id.cmp(&a.id));
        v.into_iter().take(3).map(|n| n.id).collect()
    }

    pub fn update_banner(&mut self) {
        let ids = self.banner_ids();
        if ids.is_empty() {
            if let Some(b) = self.banner.take() {
                self.p.destroy(b.pane.id);
            }
            return;
        }
        let o = self.out();
        let s = self.s;
        let w = (420.0 * s).min(o.w as f32 - 16.0) as i32;
        let mut scratch = Canvas::new(w as usize, (900.0 * s) as usize);
        let (h, _) = self.draw_banner_content(&mut scratch, &ids, w as f32, None);
        let h = h.ceil() as i32;
        let top = if self.cfg.bar_edge == Edge::Bottom { 8 } else { self.bar_total() + (8.0 * s) as i32 };
        let x = (o.w - w) / 2;
        let mut sp = SurfSpec::new(Kind::Popup);
        sp.x = x;
        sp.y = top;
        sp.w = w;
        sp.h = h;
        match &mut self.banner {
            Some(b) => {
                if b.pane.w != w || b.pane.h != h || b.y != top {
                    self.p.update(b.pane.id, &sp);
                    b.pane = Pane::new(b.pane.id, w, h);
                    b.y = top;
                }
                b.ids = ids;
                b.pane.dirty = true;
            }
            None => {
                let id = self.p.create(&sp);
                self.p.set_visible(id, true);
                self.banner = Some(Banner { pane: Pane::new(id, w, h), ids, y: top });
            }
        }
    }

    pub fn draw_banner(&mut self) {
        let mut b = match self.banner.take() {
            Some(b) => b,
            None => return,
        };
        b.pane.dirty = false;
        let w = b.pane.w as f32;
        let mut cv = std::mem::replace(&mut b.pane.cv, Canvas::new(1, 1));
        cv.clear();
        let hover = b.pane.hover;
        let (_, hits) = self.draw_banner_content(&mut cv, &b.ids, w, hover);
        b.pane.hits = hits;
        let id = b.pane.id;
        let bg = self.th.popup_bg;
        self.p.present(id, &cv, bg);
        b.pane.cv = cv;
        self.banner = Some(b);
    }

    fn draw_banner_content(&mut self, cv: &mut Canvas, ids: &[u32], w: f32, hover: Option<usize>) -> (f32, Vec<(R, Act)>) {
        let s = self.s;
        let th = self.th.clone();
        let all = self.notifs();
        let mut hits = vec![];
        let apps = self.apps.clone();
        let mut pt = Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s };
        let mut y = 0.0;
        let gap = 8.0 * s;
        for id in ids {
            if let Some(n) = all.iter().find(|n| n.id == *id) {
                let h = draw_notif_card(&mut pt, cv, &apps, n, 0.0, y, w, &th, true, &mut hits, hover);
                y += h + gap;
            }
        }
        (if y > gap { y - gap } else { y }.max(10.0), hits)
    }

    pub fn banner_pointer(&mut self, x: f32, y: f32, what: Ptr) {
        let act = {
            let b = self.banner.as_mut().unwrap();
            let h = b.pane.hit_at(x, y);
            if b.pane.hover != h {
                b.pane.hover = h;
                b.pane.dirty = true;
            }
            h.map(|i| b.pane.hits[i].1.clone())
        };
        if let Ptr::Down(1) = what {
            match act {
                Some(Act::NotifAction(id, i)) => {
                    let key = self.svc.as_ref().and_then(|s| s.notifs.iter().find(|n| n.id == id)).and_then(|n| n.actions.get(i)).map(|a| a.0.clone());
                    if let Some(k) = key {
                        self.notif_action(id, &k);
                    }
                }
                Some(Act::NotifOpen(id)) => {
                    let has_default = self.svc.as_ref().and_then(|s| s.notifs.iter().find(|n| n.id == id)).map_or(false, |n| n.actions.iter().any(|a| a.0 == "default"));
                    if has_default {
                        self.notif_action(id, "default");
                    } else {
                        self.hide_banner_of(id);
                    }
                }
                Some(Act::BannerClose(id)) => self.hide_banner_of(id),
                _ => {}
            }
        }
    }

    fn hide_banner_of(&mut self, id: u32) {
        if let Some(s) = &mut self.svc {
            if let Some(n) = s.notifs.iter_mut().find(|n| n.id == id) {
                n.banner_until = None;
            }
        }
        self.update_banner();
    }

    // ---------------- tray ----------------
    pub fn tray_visible(&self) -> Vec<usize> {
        self.svc.as_ref().map(|s| s.items.iter().enumerate().filter(|(_, i)| i.loaded && i.status != "Passive").map(|(k, _)| k).collect()).unwrap_or_default()
    }

    pub fn tray_click(&mut self, i: usize, b: u8, x: f32) {
        let (is_menu, has_menu) = match self.svc.as_ref().and_then(|s| s.items.get(i)) {
            Some(it) => (it.is_menu, !it.menu.is_empty() && it.menu != "/NO_DBUSMENU"),
            None => return,
        };
        let bt = self.bar_total();
        self.tray_anchor = x;
        match b {
            1 if !is_menu => {
                if let Some(s) = &mut self.svc {
                    s.tray_activate(i, x as i32, bt, "Activate");
                }
            }
            2 => {
                if let Some(s) = &mut self.svc {
                    s.tray_activate(i, x as i32, bt, "SecondaryActivate");
                }
            }
            _ => {
                if has_menu {
                    if let Some(s) = &mut self.svc {
                        s.tray_menu(i);
                    }
                } else if let Some(s) = &mut self.svc {
                    s.tray_activate(i, x as i32, bt, "ContextMenu");
                }
            }
        }
    }

    fn tray_entries(&self) -> Vec<dock::MenuEntry> {
        let (item, root, path) = match &self.tray_menu {
            Some(t) => t,
            None => return vec![],
        };
        let mut node = root;
        for &k in path {
            match node.children.iter().filter(|c| c.visible).nth(k) {
                Some(n) => node = n,
                None => break,
            }
        }
        let mut out: Vec<dock::MenuEntry> = vec![];
        if !path.is_empty() {
            out.push(dock::MenuEntry { label: "‹ Back".into(), act: dock::MenuAct::TrayBack, sep_before: false, enabled: true });
        }
        let mut sep = false;
        for (k, c) in node.children.iter().filter(|c| c.visible).enumerate() {
            if c.sep {
                sep = true;
                continue;
            }
            let mark = match (c.toggle, c.checked) {
                (1, true) => "✓ ",
                (2, true) => "● ",
                _ => "",
            };
            let sub = !c.children.is_empty() || c.has_sub;
            let label = format!("{}{}{}", mark, c.label, if sub { "  ›" } else { "" });
            let act = if sub { dock::MenuAct::TraySub(k) } else { dock::MenuAct::TrayClick(*item, c.id) };
            out.push(dock::MenuEntry { label, act, sep_before: sep && !out.is_empty(), enabled: c.enabled });
            sep = false;
        }
        if out.is_empty() {
            out.push(dock::MenuEntry { label: "(empty)".into(), act: dock::MenuAct::TrayBack, sep_before: false, enabled: false });
        }
        out
    }

    pub fn show_tray_menu(&mut self) {
        let entries = self.tray_entries();
        if let Some(p) = &mut self.pop {
            if p.kind == popup::Kind2::Menu {
                p.menu = entries;
                p.relayout = true;
                p.pane.dirty = true;
                return;
            }
        }
        self.close_popup();
        let ay = self.bar_total() as f32 + 6.0 * self.s;
        let ay = if self.cfg.bar_edge == Edge::Bottom { self.out().h as f32 - self.bar_total() as f32 - 6.0 * self.s } else { ay };
        self.open_popup(popup::Kind2::Menu, Some((entries, self.tray_anchor, ay)));
    }

    pub fn tray_menu_nav(&mut self, act: dock::MenuAct) -> bool {
        match act {
            dock::MenuAct::TraySub(k) => {
                if let Some(t) = &mut self.tray_menu {
                    t.2.push(k);
                }
                self.show_tray_menu();
                true
            }
            dock::MenuAct::TrayBack => {
                if let Some(t) = &mut self.tray_menu {
                    t.2.pop();
                }
                self.show_tray_menu();
                true
            }
            dock::MenuAct::TrayClick(i, id) => {
                if let Some(s) = &mut self.svc {
                    s.tray_menu_event(i, id);
                }
                self.tray_menu = None;
                false
            }
            _ => false,
        }
    }
}

/// One notification card. Returns its height. `compact` = banner style (with action buttons).
pub fn draw_notif_card(pt: &mut Paint, cv: &mut Canvas, apps: &[App], n: &Notif, x: f32, y: f32, w: f32, th: &Theme, banner: bool, hits: &mut Vec<(R, Act)>, hover: Option<usize>) -> f32 {
    let s = pt.s;
    let px = pt.px(1.0);
    let small = pt.px(0.84);
    let pad = 12.0 * s;
    let icon = 40.0 * s;
    let text_x = x + pad + icon + 10.0 * s;
    let text_w = w - (text_x - x) - pad - 22.0 * s;
    let body_lines = pt.wrap(&n.body, small, text_w, if banner { 3 } else { 2 });
    let mut h = pad + 18.0 * s + 22.0 * s + body_lines.len() as f32 * 17.0 * s + pad;
    h = h.max(pad * 2.0 + icon);
    let acts: Vec<(usize, &(String, String))> = n.actions.iter().enumerate().filter(|(_, a)| a.0 != "default" && !a.1.is_empty()).take(3).collect();
    if banner && !acts.is_empty() {
        h += 38.0 * s;
    }
    let idx_card = hits.len();
    let bg = if banner { th.popup_bg } else { th.card };
    let bgc = if hover == Some(idx_card) { mix(bg, th.popup_fg, 0.06) } else { bg };
    cv.fill_rrect(x, y, w, h, 16.0 * s, bgc);
    if banner {
        cv.stroke_rrect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, 16.0 * s, 1.0, with_alpha(th.border, 0.9));
    }
    hits.push(((x, y, w, h), if banner { Act::NotifOpen(n.id) } else { Act::Noop }));
    // icon
    if let Some(img) = &n.image {
        cv.draw_image(img, (x + pad) as i32, (y + pad) as i32, icon as i32, icon as i32);
    } else {
        let name = if n.icon.is_empty() {
            apps.iter().find(|a| !n.desktop_entry.is_empty() && a.id.eq_ignore_ascii_case(n.desktop_entry.trim_end_matches(".desktop"))).map(|a| a.icon.clone()).unwrap_or_default()
        } else {
            n.icon.clone()
        };
        let label = if n.app.is_empty() { &n.summary } else { &n.app };
        pt.app_icon(cv, &name, label, x + pad, y + pad, icon);
    }
    let when = rel_time(n.time);
    let hdr = if n.app.is_empty() { when } else { format!("{} · {}", n.app, when) };
    pt.text_fit(cv, &hdr, small, text_x, y + pad + 8.0 * s, text_w, th.dim_fg, false);
    pt.text_fit(cv, &n.summary, px, text_x, y + pad + 28.0 * s, text_w, th.popup_fg, true);
    for (i, l) in body_lines.iter().enumerate() {
        pt.text(cv, l, small, text_x, y + pad + 48.0 * s + i as f32 * 17.0 * s, th.popup_fg, false);
    }
    // close button
    let cr: R = (x + w - 30.0 * s, y + 8.0 * s, 22.0 * s, 22.0 * s);
    let ci = hits.len();
    if hover == Some(ci) {
        cv.fill_circle(cr.0 + 11.0 * s, cr.1 + 11.0 * s, 11.0 * s, th.hover);
    }
    pt.icon(cv, "window-close-symbolic", cr.0 + 4.0 * s, cr.1 + 4.0 * s, 14.0 * s, th.dim_fg);
    hits.push((cr, if banner { Act::BannerClose(n.id) } else { Act::NotifDismiss(n.id) }));
    if banner && !acts.is_empty() {
        let bw = (w - 2.0 * pad - 8.0 * s * (acts.len() as f32 - 1.0)) / acts.len() as f32;
        for (k, (ai, a)) in acts.iter().enumerate() {
            let r: R = (x + pad + k as f32 * (bw + 8.0 * s), y + h - 36.0 * s - 2.0 * s, bw, 30.0 * s);
            let bi = hits.len();
            cv.fill_rrect(r.0, r.1, r.2, r.3, 15.0 * s, if hover == Some(bi) { th.hover } else { mix(th.popup_bg, th.popup_fg, 0.08) });
            pt.text_fit(cv, &a.1, small, r.0 + 10.0 * s, r.1 + r.3 / 2.0, r.2 - 20.0 * s, th.popup_fg, true);
            hits.push((r, Act::NotifAction(n.id, *ai)));
        }
    }
    h
}

impl Shell {
    pub fn on_shortcut(&mut self, id: u32) {
        match id {
            1 => self.ipc_cmd("screenshot"),
            2 => self.ipc_cmd("shot-screen"),
            3 => self.ipc_cmd("shot-area"),
            4 => self.ipc_cmd("record"),
            5 => self.ipc_cmd("grid"),
            _ => {}
        }
    }
}
