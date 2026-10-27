//! GNOME-style application grid: paged icons, type-to-search, keyboard navigation, context actions.
use super::*;

pub struct Grid {
    pub pane: Pane,
    pub query: String,
    pub page: usize,
    pub sel: usize,
    pub shown: Vec<usize>, // indices into apps
    pub cols: usize,
    pub rows: usize,
    pub x: i32,
    pub y: i32,
}

fn score(a: &App, q: &str) -> Option<i32> {
    let q = q.to_lowercase();
    let n = a.name.to_lowercase();
    if n == q {
        return Some(1000);
    }
    if n.starts_with(&q) {
        return Some(900 - n.len() as i32);
    }
    if n.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q)) {
        return Some(700);
    }
    if n.contains(&q) {
        return Some(500);
    }
    if a.generic.to_lowercase().contains(&q) || a.keywords.iter().any(|k| k.to_lowercase().contains(&q)) {
        return Some(300);
    }
    if a.id.to_lowercase().contains(&q) || a.exec.to_lowercase().split_whitespace().next().map_or(false, |e| e.contains(&q)) {
        return Some(200);
    }
    if a.comment.to_lowercase().contains(&q) {
        return Some(100);
    }
    // subsequence fuzzy
    let mut it = n.chars();
    if q.chars().all(|c| it.any(|x| x == c)) {
        return Some(50);
    }
    None
}

impl Shell {
    fn grid_rect(&self) -> (i32, i32, i32, i32) {
        let o = self.out();
        let top = if self.cfg.bar_edge == Edge::Bottom { 0 } else { self.bar_total() };
        let bottom_inset = if self.cfg.bar_edge == Edge::Bottom { self.bar_total() } else { 0 } + self.dock.as_ref().filter(|_| !self.dock_hidden).map_or(0, |d| d.h);
        (0, top, o.w, (o.h - top - bottom_inset).max(200))
    }

    pub fn open_grid(&mut self) {
        self.close_popup();
        let (x, y, w, h) = self.grid_rect();
        let mut sp = SurfSpec::new(Kind::Overlay);
        sp.x = x;
        sp.y = y;
        sp.w = w;
        sp.h = h;
        sp.keyboard = true;
        let id = self.p.create(&sp);
        let mut g = Grid { pane: Pane::new(id, w, h), query: String::new(), page: 0, sel: 0, shown: vec![], cols: 6, rows: 3, x, y };
        g.shown = (0..self.apps.len()).collect();
        self.p.set_visible(id, true);
        self.grid = Some(g);
        self.layout_grid();
        if let Some(b) = &mut self.bar {
            b.dirty = true;
        }
        if let Some(d) = &mut self.dock {
            d.dirty = true;
        }
    }

    pub fn close_grid(&mut self) {
        if let Some(g) = self.grid.take() {
            self.p.destroy(g.pane.id);
            self.restore_focus();
            if let Some(b) = &mut self.bar {
                b.dirty = true;
            }
            if let Some(d) = &mut self.dock {
                d.dirty = true;
            }
        }
    }
    pub fn close_grid_if_catcher(&mut self) {}

    fn layout_grid(&mut self) {
        let s = self.s;
        if let Some(g) = &mut self.grid {
            let w = g.pane.w as f32;
            let h = g.pane.h as f32;
            let cell_w = 150.0 * s;
            let cell_h = 140.0 * s;
            g.cols = ((w - 160.0 * s) / cell_w).floor().clamp(3.0, 9.0) as usize;
            g.rows = ((h - 190.0 * s) / cell_h).floor().clamp(1.0, 6.0) as usize;
            g.pane.dirty = true;
        }
    }

    pub fn grid_key(&mut self, sym: u32, text: &str, ctrl: bool) {
        let _ = ctrl;
        let apps_len = self.apps.len();
        let mut launch: Option<usize> = None;
        {
            let g = match &mut self.grid {
                Some(g) => g,
                None => return,
            };
            let per = g.cols * g.rows;
            let pages = ((g.shown.len() + per - 1) / per).max(1);
            match sym {
                0xff1b => {
                    if g.query.is_empty() {
                        self.close_grid();
                    } else {
                        g.query.clear();
                        g.shown = (0..apps_len).collect();
                        g.page = 0;
                        g.sel = 0;
                    }
                    return self.after_grid_change();
                }
                0xff0d | 0xff8d => {
                    launch = g.shown.get(g.sel).copied();
                }
                0xff08 => {
                    g.query.pop();
                }
                0xff51 => g.sel = g.sel.saturating_sub(1),
                0xff53 => g.sel = (g.sel + 1).min(g.shown.len().saturating_sub(1)),
                0xff52 => g.sel = g.sel.saturating_sub(g.cols),
                0xff54 => g.sel = (g.sel + g.cols).min(g.shown.len().saturating_sub(1)),
                0xff55 => g.sel = g.sel.saturating_sub(per),
                0xff56 => g.sel = (g.sel + per).min(g.shown.len().saturating_sub(1)),
                0xff50 => g.sel = 0,
                0xff57 => g.sel = g.shown.len().saturating_sub(1),
                _ => {
                    if !text.is_empty() && g.query.len() < 64 && !ctrl {
                        g.query.push_str(text);
                    } else {
                        return;
                    }
                }
            }
            let _ = pages;
        }
        if let Some(i) = launch {
            self.close_grid();
            self.spawn_app(i, None);
            return;
        }
        // re-filter when query changed
        if let Some(g) = &mut self.grid {
            if sym == 0xff08 || (!text.is_empty() && sym != 0xff0d) {
                if g.query.is_empty() {
                    g.shown = (0..self.apps.len()).collect();
                } else {
                    let mut v: Vec<(i32, usize)> = self.apps.iter().enumerate().filter_map(|(i, a)| score(a, &g.query).map(|sc| (sc, i))).collect();
                    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                    g.shown = v.into_iter().map(|x| x.1).collect();
                }
                g.sel = 0;
            }
            let per = (g.cols * g.rows).max(1);
            g.page = g.sel / per;
        }
        self.after_grid_change();
    }

    fn after_grid_change(&mut self) {
        if let Some(g) = &mut self.grid {
            g.pane.dirty = true;
        }
    }

    pub fn grid_pointer(&mut self, x: f32, y: f32, what: Ptr) {
        let (act, idx) = {
            let g = self.grid.as_mut().unwrap();
            let h = g.pane.hit_at(x, y);
            if let Ptr::Move = what {
                if g.pane.hover != h {
                    g.pane.hover = h;
                    g.pane.dirty = true;
                    if let Some(i) = h {
                        if let Act::GridApp(k) = g.pane.hits[i].1 {
                            let per = g.cols * g.rows;
                            g.sel = g.page * per + k;
                        }
                    }
                }
            }
            (h.map(|i| g.pane.hits[i].1.clone()), h)
        };
        let _ = idx;
        match what {
            Ptr::Scroll(d) => {
                let g = self.grid.as_mut().unwrap();
                let per = (g.cols * g.rows).max(1);
                let pages = ((g.shown.len() + per - 1) / per).max(1);
                let np = (g.page as i32 + d).clamp(0, pages as i32 - 1) as usize;
                if np != g.page {
                    g.page = np;
                    g.sel = np * per;
                    g.pane.dirty = true;
                }
            }
            Ptr::Down(b) => match act {
                Some(Act::GridApp(k)) => {
                    let (app, per) = {
                        let g = self.grid.as_ref().unwrap();
                        (g.shown.get(g.page * g.cols * g.rows + k).copied(), g.cols * g.rows)
                    };
                    let _ = per;
                    if let Some(a) = app {
                        if b == 3 {
                            self.grid_menu(a, x, y);
                        } else {
                            self.close_grid();
                            self.spawn_app(a, None);
                        }
                    }
                }
                Some(Act::GridPage(p)) => {
                    let g = self.grid.as_mut().unwrap();
                    g.page = p;
                    g.sel = p * g.cols * g.rows;
                    g.pane.dirty = true;
                }
                None => {
                    // click on empty space closes (like GNOME)
                    if b == 1 {
                        self.close_grid();
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn grid_menu(&mut self, app_idx: usize, x: f32, y: f32) {
        let app = self.apps[app_idx].clone();
        let mut m: Vec<dock::MenuEntry> = vec![dock::MenuEntry { label: "Open".into(), act: dock::MenuAct::New(app_idx, None), sep_before: false, enabled: true }];
        for (k, a) in app.actions.iter().enumerate() {
            m.push(dock::MenuEntry { label: a.name.clone(), act: dock::MenuAct::New(app_idx, Some(k)), sep_before: k == 0, enabled: true });
        }
        let pinned = self.cfg.favorites.iter().any(|f| f.trim_end_matches(".desktop").eq_ignore_ascii_case(&app.id));
        m.push(dock::MenuEntry { label: if pinned { "Unpin from Dash".into() } else { "Pin to Dash".into() }, act: if pinned { dock::MenuAct::Unpin(app.id.clone()) } else { dock::MenuAct::Pin(app.id.clone()) }, sep_before: true, enabled: true });
        let (gx, gy) = self.grid.as_ref().map_or((0, 0), |g| (g.x, g.y));
        // keep the grid; open menu popup above it (needs catcher but grid stays)
        let (mx, my) = (x + gx as f32, y + gy as f32 + 150.0 * self.s);
        self.open_popup(popup::Kind2::Menu, Some((m, mx, my)));
    }

    pub fn draw_grid(&mut self) {
        let mut g = match self.grid.take() {
            Some(g) => g,
            None => return,
        };
        g.pane.dirty = false;
        let s = self.s;
        let th = self.th.clone();
        let (w, h) = (g.pane.w as f32, g.pane.h as f32);
        let cv = &mut g.pane.cv;
        cv.clear();
        let dim_bg = if self.alpha { with_alpha(if th.dark { 0xFF000000 } else { 0xFF101010 }, 0.82) } else { mix(th.popup_bg, 0xFF000000, 0.55) };
        cv.fill_rect(0, 0, w as i32, h as i32, dim_bg);
        let fg = 0xFFFFFFFF;
        let mut pt = Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s };
        let px = pt.px(1.0);
        // search entry
        let sw = (420.0 * s).min(w - 40.0 * s);
        let sx = (w - sw) / 2.0;
        let sy = 28.0 * s;
        cv.fill_rrect(sx, sy, sw, 44.0 * s, 22.0 * s, with_alpha(0xFFFFFFFF, 0.14));
        pt.icon(cv, "edit-find-symbolic", sx + 16.0 * s, sy + 14.0 * s, 16.0 * s, with_alpha(fg, 0.8));
        if g.query.is_empty() {
            pt.text(cv, "Type to search", px * 1.05, sx + 44.0 * s, sy + 22.0 * s, with_alpha(fg, 0.55), false);
        } else {
            let tw = pt.text_fit(cv, &g.query, px * 1.05, sx + 44.0 * s, sy + 22.0 * s, sw - 70.0 * s, fg, false);
            cv.fill_rect((sx + 46.0 * s + tw) as i32, (sy + 12.0 * s) as i32, 2, (20.0 * s) as i32, th.accent);
        }
        let cell_w = 150.0 * s;
        let cell_h = 140.0 * s;
        let per = (g.cols * g.rows).max(1);
        let pages = ((g.shown.len() + per - 1) / per).max(1);
        if g.page >= pages {
            g.page = pages - 1;
        }
        let gw = g.cols as f32 * cell_w;
        let gx = (w - gw) / 2.0;
        let gy = sy + 44.0 * s + 40.0 * s;
        let start = g.page * per;
        let mut hits: Vec<(R, Act)> = vec![];
        let icon = 64.0 * s;
        for k in 0..per {
            let i = start + k;
            let ai = match g.shown.get(i) {
                Some(a) => *a,
                None => break,
            };
            let app = &self.apps[ai];
            let (col, row) = (k % g.cols, k / g.cols);
            let r: R = (gx + col as f32 * cell_w + 8.0 * s, gy + row as f32 * cell_h, cell_w - 16.0 * s, cell_h - 10.0 * s);
            let selected = g.sel == i;
            if selected || g.pane.hover.and_then(|hh| g.pane.hits.get(hh)).map(|x| x.1.clone()) == Some(Act::GridApp(k)) {
                cv.fill_rrect(r.0, r.1, r.2, r.3, 16.0 * s, with_alpha(0xFFFFFFFF, if selected { 0.16 } else { 0.10 }));
            }
            pt.app_icon(cv, &app.icon, &app.name, r.0 + (r.2 - icon) / 2.0, r.1 + 14.0 * s, icon);
            let label = pt.tx.fit(&app.name, px * 0.95, r.2 - 12.0 * s);
            pt.text_center(cv, &label, px * 0.95, r.0 + r.2 / 2.0, r.1 + 14.0 * s + icon + 24.0 * s, fg, false);
            hits.push((r, Act::GridApp(k)));
        }
        if g.shown.is_empty() {
            pt.text_center(cv, "No results", px * 1.2, w / 2.0, h / 2.0, with_alpha(fg, 0.6), false);
        }
        if pages > 1 {
            let dy = gy + g.rows as f32 * cell_h + 16.0 * s;
            let total = pages as f32 * 18.0 * s;
            for p in 0..pages {
                let dx = (w - total) / 2.0 + p as f32 * 18.0 * s + 9.0 * s;
                cv.fill_circle(dx, dy, 4.5 * s, with_alpha(0xFFFFFFFF, if p == g.page { 0.95 } else { 0.35 }));
                hits.push(((dx - 9.0 * s, dy - 9.0 * s, 18.0 * s, 18.0 * s), Act::GridPage(p)));
            }
        }
        g.pane.hits = hits;
        let id = g.pane.id;
        self.grid = Some(g);
        let cvr = &self.grid.as_ref().unwrap().pane.cv;
        self.p.present(id, cvr, mix(th.popup_bg, 0xFF000000, 0.55));
    }
}
