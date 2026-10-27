//! GNOME-style top bar rendering.
use super::*;
use crate::sysinfo::Net;
use crate::gfx::Image;

pub fn clock_text(cfg: &Config, with_date: bool) -> String {
    let tm = crate::sys::local_time(crate::sys::now_epoch());
    let (h, ampm) = if cfg.clock_24h { (tm.hour, String::new()) } else { (if tm.hour % 12 == 0 { 12 } else { tm.hour % 12 }, if tm.hour < 12 { " AM".into() } else { " PM".into() }) };
    let time = if cfg.clock_seconds { format!("{:02}:{:02}:{:02}{}", h, tm.min, tm.sec, ampm) } else if cfg.clock_24h { format!("{:02}:{:02}", h, tm.min) } else { format!("{}:{:02}{}", h, tm.min, ampm) };
    if with_date && cfg.clock_date {
        format!("{} {} {}  {}", DAYS_S[(tm.wday.rem_euclid(7)) as usize], MONTHS_S[(tm.mon.rem_euclid(12)) as usize], tm.mday, time)
    } else {
        time
    }
}

pub fn battery_icon(b: &crate::sysinfo::Battery) -> String {
    let lvl = ((b.percent as f32 / 10.0).round() as i32 * 10).clamp(0, 100);
    if b.full || (b.charging && b.percent >= 100) {
        "battery-level-100-charged-symbolic".into()
    } else if b.charging {
        format!("battery-level-{}-charging-symbolic", lvl)
    } else {
        format!("battery-level-{}-symbolic", lvl)
    }
}

pub fn volume_icon(v: Option<f32>, muted: bool) -> &'static str {
    match v {
        _ if muted => "audio-volume-muted-symbolic",
        Some(x) if x <= 0.001 => "audio-volume-muted-symbolic",
        Some(x) if x < 0.34 => "audio-volume-low-symbolic",
        Some(x) if x < 0.67 => "audio-volume-medium-symbolic",
        Some(_) => "audio-volume-high-symbolic",
        None => "audio-volume-medium-symbolic",
    }
}

pub fn net_icon(n: &Net, signal: Option<i32>) -> &'static str {
    match n {
        Net::Wifi { up: true } => match signal.unwrap_or(70) {
            s if s >= 80 => "network-wireless-signal-excellent-symbolic",
            s if s >= 55 => "network-wireless-signal-good-symbolic",
            s if s >= 30 => "network-wireless-signal-ok-symbolic",
            _ => "network-wireless-signal-weak-symbolic",
        },
        Net::Wifi { up: false } => "network-wireless-offline-symbolic",
        Net::Wired { up: true } => "network-wired-symbolic",
        Net::Wired { up: false } => "network-wired-offline-symbolic",
        Net::None => "network-offline-symbolic",
    }
}

impl Shell {
    pub fn draw_bar(&mut self) {
        let mut pane = match self.bar.take() {
            Some(p) => p,
            None => return,
        };
        pane.dirty = false;
        pane.cv.clear();
        pane.hits.clear();
        let s = self.s;
        let (w, h) = (pane.w as f32, self.bar_h() as f32);
        let m = self.bar_margin() as f32;
        let alpha = if self.alpha { self.cfg.bar_opacity } else { 1.0 };
        let bg = with_alpha(self.th.panel_bg, alpha);
        let fg = self.th.panel_fg;
        if m > 0.0 {
            pane.cv.fill_rrect(m * 2.0, m, w - m * 4.0, h, h * 0.4, bg);
        } else {
            pane.cv.fill_rect(0, 0, pane.w, pane.h, bg);
        }
        let cy = m + h / 2.0;
        let pad = 14.0 * s;
        let gap = 2.0 * s;
        let pill_h = (h - 6.0 * s).max(16.0);
        let hover = pane.hover;
        let mut cfg = self.cfg.clone();
        let rec_label = self.rec_elapsed();
        if rec_label.is_some() {
            cfg.right.insert(0, "recording".to_string());
        }
        let ws = self.p.workspaces();
        let wins = self.p.windows();
        let active_title = wins.iter().find(|w| w.active).map(|w| if w.app_id.is_empty() { w.title.clone() } else { desktop::match_app(&self.apps, &w.app_id).map(|a| a.name.clone()).unwrap_or_else(|| w.app_id.clone()) });
        let grid_open = self.grid.is_some();
        let pop_kind = self.pop.as_ref().map(|p| p.kind);
        let bat = self.bat.clone();
        let net = self.net.clone();
        let vol = (self.qs.vol, self.qs.muted);
        let bt_on = self.qs.bt == Some(true);
        let night = self.cfg.night_on;
        let toast = self.toast.as_ref().map(|t| t.0.clone());
        let sig = self.qs.wifi_signal;
        let tray: Vec<(usize, String, Option<Image>, String)> = self.tray_visible().iter().filter_map(|&i| self.svc.as_ref().and_then(|sv| sv.items.get(i)).map(|it| (i, if it.status == "NeedsAttention" && !it.attention_icon.is_empty() { it.attention_icon.clone() } else { it.icon.clone() }, it.pixmap.clone(), if it.title.is_empty() { it.id.clone() } else { it.title.clone() }))).collect();
        let (unseen, dnd, caff) = (self.unseen_notifs(), self.dnd(), self.caffeine);
        let clock = clock_text(&cfg, true);
        let mut pt = Paint { tx: &mut self.tx, txb: &mut self.txb, icons: &mut self.icons, th: &self.th, s };
        let px = pt.px(1.0);
        let icon = 16.0 * s;

        // measure
        let measure = |pt: &mut Paint, name: &str| -> f32 {
            match name {
                "activities" => pt.text_w("Activities", px, true) + pad * 2.0,
                "workspaces" => (ws.0.len().max(1) as f32 - 1.0) * 14.0 * s + 30.0 * s + pad,
                "appmenu" => match &active_title {
                    Some(t) => {
                        let fit = pt.txb.fit(t, px, 220.0 * s);
                        pt.text_w(&fit, px, true) + pad * 2.0
                    }
                    None => 0.0,
                },
                "clock" => pt.text_w(&clock, px, true) + pad * 2.0 + if unseen || dnd { 20.0 * s } else { 0.0 },
                "recording" => pt.text_w(rec_label.as_deref().unwrap_or(""), px, true) + 38.0 * s,
                "tray" => if tray.is_empty() { 0.0 } else { tray.len() as f32 * (icon + 12.0 * s) + 6.0 * s },
                "status" => {
                    let mut n = 2.0; // network + volume
                    if caff { n += 1.0 }
                    if bat.present { n += 1.0 }
                    if bt_on { n += 1.0 }
                    if night { n += 1.0 }
                    let pct = if bat.present && cfg.battery_percent { pt.text_w(&format!("{}%", bat.percent), px * 0.95, true) + 4.0 * s } else { 0.0 };
                    n * icon + (n - 1.0) * 8.0 * s + pad * 1.3 + pct
                }
                "session" => 30.0 * s,
                _ => 0.0,
            }
        };
        let mut place: Vec<(String, f32, f32)> = vec![]; // name, x, w
        let side = 8.0 * s + m * 2.0;
        let mut x = side;
        for n in &cfg.left {
            let wd = measure(&mut pt, n);
            if wd > 0.0 {
                place.push((n.clone(), x, wd));
                x += wd + gap;
            }
        }
        let cw: f32 = cfg.center.iter().map(|n| measure(&mut pt, n)).filter(|w| *w > 0.0).sum::<f32>();
        let mut cx = (w - cw) / 2.0;
        for n in &cfg.center {
            let wd = measure(&mut pt, n);
            if wd > 0.0 {
                place.push((n.clone(), cx, wd));
                cx += wd + gap;
            }
        }
        let mut rx = w - side;
        for n in cfg.right.iter().rev() {
            let wd = measure(&mut pt, n);
            if wd > 0.0 {
                rx -= wd;
                place.push((n.clone(), rx, wd));
                rx -= gap;
            }
        }
        let mut hits: Vec<(R, Act)> = vec![];
        for (name, x, wd) in place {
            let act = match name.as_str() {
                "activities" => Act::Activities,
                "clock" => Act::Clock,
                "status" => Act::Status,
                "recording" => Act::RecordStop,
                "workspaces" => Act::Workspaces,
                _ => Act::Noop,
            };
            let r: R = (x, cy - pill_h / 2.0, wd, pill_h);
            let idx = hits.len();
            let is_active = match name.as_str() {
                "activities" => grid_open,
                "clock" => pop_kind == Some(popup::Kind2::Calendar),
                "status" => pop_kind == Some(popup::Kind2::Quick),
                "workspaces" => pop_kind == Some(popup::Kind2::Workspaces),
                _ => false,
            };
            if act != Act::Noop {
                if is_active {
                    pane.cv.fill_rrect(r.0, r.1, r.2, r.3, r.3 / 2.0, with_alpha(fg, 0.22));
                } else if hover.map_or(false, |hv| pane.hits.get(hv).map(|h| h.1.clone()) == Some(act.clone()) && false) {
                } else if hover == Some(idx) {
                    pane.cv.fill_rrect(r.0, r.1, r.2, r.3, r.3 / 2.0, with_alpha(fg, 0.12));
                }
            }
            match name.as_str() {
                "activities" => {
                    pt.text(&mut pane.cv, "Activities", px, x + pad, cy, fg, true);
                }
                "clock" => {
                    if let Some(t) = &toast {
                        pt.text_center(&mut pane.cv, t, px, x + wd / 2.0, cy, fg, true);
                    } else {
                        let tw = pt.text(&mut pane.cv, &clock, px, x + pad, cy, fg, true);
                        if dnd {
                            pt.icon(&mut pane.cv, "notifications-disabled-symbolic", x + pad + tw + 6.0 * s, cy - 7.0 * s, 14.0 * s, fg);
                        } else if unseen {
                            pane.cv.fill_circle(x + pad + tw + 12.0 * s, cy, 3.5 * s, fg);
                        }
                    }
                }
                "appmenu" => {
                    if let Some(t) = &active_title {
                        pt.text_fit(&mut pane.cv, t, px, x + pad, cy, 220.0 * s, fg, true);
                    }
                }
                "workspaces" => {
                    let n = ws.0.len().max(1);
                    let cur_i = if ws.0.is_empty() { 0 } else { ws.1.min(n - 1) };
                    let mut dx = x + pad / 2.0;
                    for i in 0..n {
                        let cur = i == cur_i;
                        let dw = if cur { 30.0 * s } else { 8.0 * s };
                        let dh = 8.0 * s;
                        pane.cv.fill_rrect(dx, cy - dh / 2.0, dw, dh, dh / 2.0, with_alpha(fg, if cur { 1.0 } else { 0.45 }));
                        dx += dw + 6.0 * s;
                    }
                }
                "recording" => {
                    pane.cv.fill_rrect(r.0, r.1, r.2, r.3, r.3 / 2.0, 0xFFC01C28);
                    pane.cv.fill_circle(r.0 + 15.0 * s, cy, 4.5 * s, 0xFFFFFFFF);
                    pt.text(&mut pane.cv, rec_label.as_deref().unwrap_or(""), px, r.0 + 28.0 * s, cy, 0xFFFFFFFF, true);
                }
                "tray" => {
                    let mut tx0 = x + 3.0 * s;
                    for (k, name, pix, label) in &tray {
                        let r: R = (tx0, cy - pill_h / 2.0, icon + 12.0 * s, pill_h);
                        let idx = hits.len();
                        if hover == Some(idx) {
                            pane.cv.fill_rrect(r.0, r.1, r.2, r.3, r.3 / 2.0, with_alpha(fg, 0.12));
                        }
                        let ix = tx0 + 6.0 * s;
                        let iy = cy - icon / 2.0;
                        match pix {
                            Some(img) => pane.cv.draw_image(img, ix as i32, iy as i32, icon as i32, icon as i32),
                            None => pt.app_icon(&mut pane.cv, name, label, ix, iy, icon),
                        }
                        hits.push((r, Act::Tray(*k)));
                        tx0 += icon + 12.0 * s;
                    }
                    continue;
                }
                "status" => {
                    let mut ix = x + pad * 0.65;
                    let iy = cy - icon / 2.0;
                    let mut names: Vec<String> = vec![];
                    if caff {
                        names.push("@cup".into());
                    }
                    names.push(net_icon(&net, sig).to_string());
                    if bt_on {
                        names.push("bluetooth-active-symbolic".into());
                    }
                    if night {
                        names.push("night-light-symbolic".into());
                    }
                    names.push(volume_icon(vol.0, vol.1).to_string());
                    if bat.present {
                        names.push(battery_icon(&bat));
                    }
                    let nn = names.len();
                    for (k, n) in names.into_iter().enumerate() {
                        pt.icon(&mut pane.cv, &n, ix, iy, icon, fg);
                        ix += icon + 8.0 * s;
                        if bat.present && cfg.battery_percent && k + 1 == nn {
                            pt.text(&mut pane.cv, &format!("{}%", bat.percent), px * 0.95, ix - 4.0 * s, cy, fg, true);
                        }
                    }
                }
                _ => {}
            }
            hits.push((r, act));
        }
        // hit indices must line up with hover indices: rebuild hover mapping by position
        pane.hits = hits;
        self.bar = Some(pane);
        let (id, bg_op) = {
            let b = self.bar.as_ref().unwrap();
            (b.id, self.th.panel_bg)
        };
        let cvref = &self.bar.as_ref().unwrap().cv;
        self.p.present(id, cvref, bg_op);
    }
}
