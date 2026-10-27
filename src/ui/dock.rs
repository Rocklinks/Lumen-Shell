//! GNOME-style dock: pinned favorites + running apps, indicators, context menus, auto-hide.
use super::*;

#[derive(Clone, Debug)]
pub struct Item {
    pub app: Option<usize>,
    pub key: String,
    pub label: String,
    pub icon: String,
    pub wins: Vec<u64>,
    pub pinned: bool,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub enum MenuAct {
    New(usize, Option<usize>),
    Pin(String),
    Unpin(String),
    Quit(Vec<u64>),
    Focus(u64),
    TrayClick(usize, i32),
    TraySub(usize),
    TrayBack,
}
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub label: String,
    pub act: MenuAct,
    pub sep_before: bool,
    pub enabled: bool,
}
pub struct MenuTarget;

fn fav_matches(a: &App, fav: &str) -> bool {
    let f = fav.trim_end_matches(".desktop").to_lowercase();
    let id = a.id.to_lowercase();
    id == f || id.rsplit('.').next() == Some(f.as_str()) && !f.contains('.')
}

impl Shell {
    pub fn refresh_dock_items(&mut self) {
        let wins = self.p.windows();
        let mut items: Vec<Item> = vec![];
        for fav in &self.cfg.favorites {
            if let Some((i, a)) = self.apps.iter().enumerate().find(|(_, a)| fav_matches(a, fav)) {
                items.push(Item { app: Some(i), key: a.id.clone(), label: a.name.clone(), icon: a.icon.clone(), wins: vec![], pinned: true, active: false });
            }
        }
        for w in &wins {
            let m = desktop::match_app(&self.apps, &w.app_id).map(|a| a.id.clone());
            let idx = match &m {
                Some(id) => items.iter().position(|it| &it.key == id),
                None => items.iter().position(|it| it.app.is_none() && it.key == w.app_id),
            };
            let it = match idx {
                Some(i) => &mut items[i],
                None => {
                    let ai = m.as_ref().and_then(|id| self.apps.iter().position(|a| &a.id == id));
                    let (label, icon) = match ai.and_then(|i| self.apps.get(i)) {
                        Some(a) => (a.name.clone(), a.icon.clone()),
                        None => (if w.app_id.is_empty() { w.title.clone() } else { w.app_id.clone() }, w.app_id.to_lowercase()),
                    };
                    items.push(Item { app: ai, key: m.clone().unwrap_or_else(|| w.app_id.clone()), label, icon, wins: vec![], pinned: false, active: false });
                    items.last_mut().unwrap()
                }
            };
            it.wins.push(w.id);
            if w.active {
                it.active = true;
            }
        }
        self.dock_items = items;
    }

    pub fn dock_size(&self) -> (i32, i32) {
        let s = self.s;
        let icon = (self.cfg.dock_icon as f32 * s) as i32;
        let cell = icon + (14.0 * s) as i32;
        let n = self.dock_items.len() as i32 + 1;
        let sep = if self.dock_items.iter().any(|i| !i.pinned) { (10.0 * s) as i32 } else { 0 };
        let margin = if self.alpha { (8.0 * s) as i32 } else { 0 };
        let w = n * cell + (16.0 * s) as i32 + sep;
        let h = icon + (22.0 * s) as i32 + margin;
        (w, h)
    }

    pub fn refresh_dock_items_resize(&mut self) {
        if !self.cfg.dock_enabled {
            return;
        }
        let before = self.dock_size();
        self.refresh_dock_items();
        let after = self.dock_size();
        if before != after {
            let inset = self.dock_inset();
            if let Some(d) = &mut self.dock {
                let mut sp = SurfSpec::new(Kind::Panel);
                sp.edge = if self.cfg.dock_edge == Edge::Top { Edge::Top } else { Edge::Bottom };
                sp.h = after.1;
                sp.len = after.0;
                sp.exclusive = if self.cfg.dock_vis == Visibility::Always { after.1 } else { 0 };
                sp.inset = inset;
                sp.above_fullscreen = self.cfg.dock_over_fullscreen && self.cfg.dock_vis != Visibility::HideInFullscreen;
                d.w = after.0;
                d.h = after.1;
                d.cv = Canvas::new(after.0 as usize, after.1 as usize);
                let id = d.id;
                self.p.update(id, &sp);
            }
        }
        if let Some(d) = &mut self.dock {
            d.dirty = true;
        }
    }

    pub fn draw_dock(&mut self) {
        let mut pane = match self.dock.take() {
            Some(p) => p,
            None => return,
        };
        pane.dirty = false;
        pane.cv.clear();
        pane.hits.clear();
        let s = self.s;
        let th = self.th.clone();
        let margin = if self.alpha { 8.0 * s } else { 0.0 };
        let (w, h) = (pane.w as f32, pane.h as f32 - margin);
        let top_edge = self.cfg.dock_edge == Edge::Top;
        let y0 = if top_edge { margin } else { 0.0 };
        let op = if self.alpha { self.cfg.dock_opacity } else { 1.0 };
        let bgc = with_alpha(mix(th.popup_bg, if th.dark { 0xFF000000 } else { 0xFF303030 }, if th.dark { 0.25 } else { 0.0 }), op);
        let bgc = if th.dark { bgc } else { with_alpha(mix(0xFF2A2A2A, 0xFF000000, 0.2), op * 0.92) };
        if self.alpha {
            pane.cv.fill_rrect(0.0, y0, w, h, 20.0 * s, bgc);
            pane.cv.stroke_rrect(0.5, y0 + 0.5, w - 1.0, h - 1.0, 20.0 * s, 1.0, with_alpha(0xFFFFFFFF, 0.08));
        } else {
            pane.cv.fill_rect(0, 0, pane.w, pane.h, 0xFF1E1E1E);
        }
        let icon = self.cfg.dock_icon as f32 * s;
        let cell = icon + 14.0 * s;
        let hover = pane.hover;
        let items = self.dock_items.clone();
        let mut x = 8.0 * s;
        let mut hits: Vec<(R, Act)> = vec![];
        let mut pt = Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s };
        let cy = y0 + h / 2.0;
        let mut sep_done = false;
        for (i, it) in items.iter().enumerate() {
            if !it.pinned && !sep_done && i > 0 {
                pane.cv.fill_rect((x + 2.0 * s) as i32, (cy - icon / 2.0) as i32, 1, icon as i32, with_alpha(0xFFFFFFFF, 0.2));
                x += 10.0 * s;
                sep_done = true;
            }
            let r: R = (x, y0 + 4.0 * s, cell, h - 8.0 * s);
            let idx = hits.len();
            if hover == Some(idx) {
                pane.cv.fill_rrect(r.0, r.1, r.2, r.3, 14.0 * s, with_alpha(0xFFFFFFFF, 0.14));
            } else if it.active {
                pane.cv.fill_rrect(r.0, r.1, r.2, r.3, 14.0 * s, with_alpha(0xFFFFFFFF, 0.08));
            }
            pt.app_icon(&mut pane.cv, &it.icon, &it.label, x + 7.0 * s, cy - icon / 2.0 - 2.0 * s, icon);
            // running indicator dots
            let n = it.wins.len().min(3);
            for d in 0..n {
                let dx = r.0 + r.2 / 2.0 + (d as f32 - (n as f32 - 1.0) / 2.0) * 7.0 * s;
                pane.cv.fill_circle(dx, y0 + h - 7.0 * s, 2.2 * s, if it.active { th.accent } else { 0xFFE0E0E0 });
            }
            hits.push((r, Act::Dock(i)));
            x += cell;
        }
        // show applications button
        let r: R = (x, y0 + 4.0 * s, cell, h - 8.0 * s);
        let idx = hits.len();
        if hover == Some(idx) || self.grid.is_some() {
            pane.cv.fill_rrect(r.0, r.1, r.2, r.3, 14.0 * s, with_alpha(0xFFFFFFFF, 0.14));
        }
        pt.icon(&mut pane.cv, "view-app-grid-symbolic", x + (cell - icon * 0.6) / 2.0, cy - icon * 0.3 - 2.0 * s, icon * 0.6, 0xFFFFFFFF);
        hits.push((r, Act::ShowApps));
        pane.hits = hits;
        let id = pane.id;
        self.dock = Some(pane);
        let cvr = &self.dock.as_ref().unwrap().cv;
        self.p.present(id, cvr, 0xFF202020);
    }

    pub fn dock_pointer(&mut self, x: f32, y: f32, what: Ptr) {
        let hit = {
            let d = self.dock.as_mut().unwrap();
            let h = d.hit_at(x, y);
            let ch = match what {
                Ptr::Leave => d.hover.take().is_some(),
                _ => {
                    let c = d.hover != h;
                    d.hover = h;
                    c
                }
            };
            if ch {
                d.dirty = true;
            }
            h
        };
        match what {
            Ptr::Enter => self.dock_leave = None,
            Ptr::Leave => {
                if self.cfg.dock_vis == Visibility::AutoHide {
                    self.dock_leave = Some(Instant::now());
                }
            }
            Ptr::Down(b) => {
                let act = hit.map(|i| self.dock.as_ref().unwrap().hits[i].1.clone());
                match (b, act) {
                    (1, Some(Act::ShowApps)) => self.do_act(Act::ShowApps, x, y),
                    (1, Some(Act::Dock(i))) => self.dock_click(i, false),
                    (2, Some(Act::Dock(i))) => self.dock_click(i, true),
                    (3, Some(Act::Dock(i))) => self.dock_menu(i, x),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn dock_click(&mut self, i: usize, force_new: bool) {
        let it = match self.dock_items.get(i).cloned() {
            Some(it) => it,
            None => return,
        };
        if it.wins.is_empty() || force_new {
            match it.app {
                Some(a) => self.spawn_app(a, None),
                None => self.toast("Nothing to launch"),
            }
            return;
        }
        let ws = self.p.windows();
        let active = ws.iter().find(|w| w.active).map(|w| w.id);
        if it.wins.len() == 1 {
            let w = it.wins[0];
            if active == Some(w) {
                self.p.minimize(w);
            } else {
                self.p.activate(w);
            }
        } else {
            // cycle: activate the window after the active one
            let pos = it.wins.iter().position(|w| Some(*w) == active);
            let next = match pos {
                Some(p) => it.wins[(p + 1) % it.wins.len()],
                None => it.wins[0],
            };
            self.p.activate(next);
        }
    }

    fn dock_menu(&mut self, i: usize, x: f32) {
        let it = match self.dock_items.get(i).cloned() {
            Some(it) => it,
            None => return,
        };
        let mut m: Vec<MenuEntry> = vec![];
        let ws = self.p.windows();
        for wid in &it.wins {
            if let Some(w) = ws.iter().find(|w| w.id == *wid) {
                let t = if w.title.is_empty() { it.label.clone() } else { w.title.clone() };
                m.push(MenuEntry { label: t, act: MenuAct::Focus(*wid), sep_before: false, enabled: true });
            }
        }
        if let Some(a) = it.app {
            let app = self.apps[a].clone();
            m.push(MenuEntry { label: if it.wins.is_empty() { format!("Open {}", app.name) } else { "New Window".into() }, act: MenuAct::New(a, None), sep_before: !m.is_empty(), enabled: true });
            for (k, act) in app.actions.iter().enumerate() {
                m.push(MenuEntry { label: act.name.clone(), act: MenuAct::New(a, Some(k)), sep_before: k == 0, enabled: true });
            }
            let id = app.id.clone();
            m.push(MenuEntry { label: if it.pinned { "Unpin".into() } else { "Pin to Dash".into() }, act: if it.pinned { MenuAct::Unpin(id) } else { MenuAct::Pin(id) }, sep_before: true, enabled: true });
        }
        if !it.wins.is_empty() {
            m.push(MenuEntry { label: if it.wins.len() > 1 { "Quit (all windows)".into() } else { "Quit".into() }, act: MenuAct::Quit(it.wins.clone()), sep_before: true, enabled: true });
        }
        if m.is_empty() {
            return;
        }
        let o = self.out();
        let dock_len = self.dock.as_ref().map_or(0, |d| d.w);
        let left = (o.w - dock_len) / 2;
        let ax = left as f32 + x;
        let top = (o.h - self.dock.as_ref().map_or(0, |d| d.h)) as f32 - 6.0 * self.s;
        self.close_popup();
        self.open_popup(popup::Kind2::Menu, Some((m, ax, top)));
    }

    pub fn menu_act(&mut self, i: usize) {
        let ent = self.pop.as_ref().and_then(|p| p.menu.get(i)).cloned();
        if let Some(e) = &ent {
            if !e.enabled {
                return;
            }
            if matches!(e.act, MenuAct::TraySub(_) | MenuAct::TrayBack) {
                self.tray_menu_nav(e.act.clone());
                return;
            }
        }
        let act = ent.map(|e| e.act);
        self.close_popup();
        if let Some(a @ MenuAct::TrayClick(..)) = &act {
            self.tray_menu_nav(a.clone());
            return;
        }
        match act {
            Some(MenuAct::New(a, k)) => self.spawn_app(a, k),
            Some(MenuAct::Pin(id)) => {
                if !self.cfg.favorites.iter().any(|f| fav_matches(&self.apps.iter().find(|a| a.id == id).cloned().unwrap_or_default(), f)) {
                    self.cfg.favorites.push(id);
                }
                self.save_cfg_quiet();
                self.refresh_dock_items_resize();
            }
            Some(MenuAct::Unpin(id)) => {
                let app = self.apps.iter().find(|a| a.id == id).cloned().unwrap_or_default();
                self.cfg.favorites.retain(|f| !fav_matches(&app, f));
                self.save_cfg_quiet();
                self.refresh_dock_items_resize();
            }
            Some(MenuAct::Quit(ws)) => {
                for w in ws {
                    self.p.close_window(w);
                }
            }
            Some(MenuAct::Focus(w)) => self.p.activate(w),
            Some(MenuAct::TrayClick(..)) | Some(MenuAct::TraySub(_)) | Some(MenuAct::TrayBack) | None => {}
        }
    }

    pub fn save_cfg_quiet(&mut self) {
        self.cfg.save();
        self.cfg.mtime = std::fs::metadata(&self.cfg.path).and_then(|m| m.modified()).ok();
    }
}
