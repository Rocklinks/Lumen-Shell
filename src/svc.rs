//! Session-bus services: freedesktop Notifications server, StatusNotifierWatcher/Host (AppIndicator / KStatusNotifierItem),
//! screensaver inhibit (Caffeine). Pure data + protocol logic; UI lives in ui/.
use crate::dbus::*;
use crate::gfx::Image;
use std::time::{Duration, Instant};

pub const NOTIF_PATH: &str = "/org/freedesktop/Notifications";
pub const NOTIF_IFACE: &str = "org.freedesktop.Notifications";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_IFACE: &str = "org.kde.StatusNotifierWatcher";

#[derive(Clone, Debug, Default)]
pub struct Notif {
    pub id: u32,
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: u8, // 0 low 1 normal 2 critical
    pub expire_ms: i32,
    pub time: i64,
    pub transient: bool,
    pub desktop_entry: String,
    pub data: String,
    /// While Some(t) and now < t the banner is on screen.
    pub banner_until: Option<Instant>,
    pub image: Option<Image>,
}

#[derive(Clone, Debug, Default)]
pub struct TrayItem {
    pub reg: String, // exactly what the client registered (used in signals)
    pub svc: String, // destination for calls
    pub owner: String, // unique name (for matching signals)
    pub path: String,
    pub id: String,
    pub title: String,
    pub icon: String,
    pub attention_icon: String,
    pub theme_path: String,
    pub pixmap: Option<Image>,
    pub status: String,
    pub menu: String,
    pub is_menu: bool,
    pub tooltip: String,
    pub loaded: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TMenu {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub sep: bool,
    pub toggle: u8, // 0 none, 1 check, 2 radio
    pub checked: bool,
    pub icon: String,
    pub children: Vec<TMenu>,
    pub has_sub: bool,
}

pub enum Effect {
    NotifNew(u32),
    NotifGone(u32),
    Tray,
    MenuLoaded(usize, TMenu),
    NameTaken,
    Inhibited(bool),
    Error(String),
}

pub struct Services {
    pub bus: Bus,
    pub notifs: Vec<Notif>,
    pub next_id: u32,
    pub dnd: bool,
    pub items: Vec<TrayItem>,
    pub have_notif_name: bool,
    pub have_watcher_name: bool,
    pub cookie: Option<u32>,
    pub inhibit_pending: bool,
}

pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut ent = String::new();
    let mut in_ent = false;
    for c in s.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
            }
            continue;
        }
        if in_ent {
            if c == ';' {
                out.push(match ent.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    _ => ' ',
                });
                ent.clear();
                in_ent = false;
            } else if ent.len() > 8 {
                out.push('&');
                out.push_str(&ent);
                ent.clear();
                in_ent = false;
            } else {
                ent.push(c);
            }
            continue;
        }
        match c {
            '<' => in_tag = true,
            '&' => in_ent = true,
            _ => out.push(c),
        }
    }
    out
}

fn pixmap_to_image(v: &Val) -> Option<Image> {
    // a(iiay) : pick the largest entry up to 128px
    let items = v.as_arr()?;
    let mut best: Option<(i64, Image)> = None;
    for it in items {
        let f = it.as_arr()?;
        let (w, h) = (f.first()?.as_i64()?, f.get(1)?.as_i64()?);
        let data = match f.get(2)? {
            Val::Arr(_, b) => b,
            _ => continue,
        };
        if w <= 0 || h <= 0 || w > 512 || h > 512 || data.len() < (w * h * 4) as usize {
            continue;
        }
        let mut px = Vec::with_capacity((w * h) as usize);
        for c in data.chunks_exact(4).take((w * h) as usize) {
            let g = |v: &Val| if let Val::U8(x) = v { *x as u32 } else { 0 };
            px.push((g(&c[0]) << 24) | (g(&c[1]) << 16) | (g(&c[2]) << 8) | g(&c[3]));
        }
        let score = if w <= 128 { w } else { 1000 - w };
        if best.as_ref().map_or(true, |b| score > b.0) {
            best = Some((score, Image { w: w as usize, h: h as usize, px }));
        }
    }
    best.map(|b| b.1)
}

fn parse_menu(v: &Val, depth: u32) -> Option<TMenu> {
    if depth > 6 {
        return None;
    }
    let st = match v {
        Val::Var(i) => i.as_arr()?,
        o => o.as_arr()?,
    };
    let id = st.first()?.as_i64()? as i32;
    let props = st.get(1)?;
    let mut m = TMenu { id, enabled: true, visible: true, ..Default::default() };
    if let Some(l) = props.dict_get("label").and_then(|v| v.as_str()) {
        // dbusmenu mnemonics: "_" prefix, "__" literal
        let mut o = String::new();
        let mut it = l.chars().peekable();
        while let Some(c) = it.next() {
            if c == '_' {
                if it.peek() == Some(&'_') {
                    o.push('_');
                    it.next();
                }
            } else {
                o.push(c);
            }
        }
        m.label = o;
    }
    m.sep = props.dict_get("type").and_then(|v| v.as_str()) == Some("separator");
    m.enabled = props.dict_get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    m.visible = props.dict_get("visible").and_then(|v| v.as_bool()).unwrap_or(true);
    m.icon = props.dict_get("icon-name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    m.toggle = match props.dict_get("toggle-type").and_then(|v| v.as_str()) {
        Some("checkmark") => 1,
        Some("radio") => 2,
        _ => 0,
    };
    m.checked = props.dict_get("toggle-state").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
    m.has_sub = props.dict_get("children-display").and_then(|v| v.as_str()) == Some("submenu");
    if let Some(Val::Arr(_, kids)) = st.get(2) {
        for k in kids {
            if let Some(c) = parse_menu(k, depth + 1) {
                m.children.push(c);
            }
        }
    }
    Some(m)
}

impl Services {
    pub fn start(take_over: bool) -> Result<Services, String> {
        let mut bus = Bus::connect()?;
        bus.add_match("type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged'");
        bus.add_match("type='signal',interface='org.kde.StatusNotifierItem'");
        bus.request_name("org.freedesktop.Notifications");
        bus.request_name("org.kde.StatusNotifierWatcher");
        bus.flush();
        let _ = take_over;
        Ok(Services { bus, notifs: vec![], next_id: 1, dnd: false, items: vec![], have_notif_name: false, have_watcher_name: false, cookie: None, inhibit_pending: false })
    }

    pub fn fd(&self) -> i32 {
        self.bus.fd()
    }

    pub fn poll(&mut self) -> Result<Vec<Effect>, String> {
        let evs = self.bus.poll()?;
        let mut fx = vec![];
        for e in evs {
            match e {
                Ev::Call(m) => self.on_call(m, &mut fx),
                Ev::Signal(m) => self.on_signal(m, &mut fx),
                Ev::Reply { tag, msg } => self.on_reply(&tag, msg, &mut fx),
            }
        }
        self.bus.flush();
        Ok(fx)
    }

    // ---------- incoming method calls ----------
    fn on_call(&mut self, m: Msg, fx: &mut Vec<Effect>) {
        match (m.iface.as_str(), m.member.as_str()) {
            ("org.freedesktop.DBus.Peer", "Ping") => self.bus.reply(&m, vec![]),
            ("org.freedesktop.DBus.Introspectable", "Introspect") => {
                let xml = if m.path == NOTIF_PATH {
                    "<node><interface name=\"org.freedesktop.Notifications\"><method name=\"GetCapabilities\"><arg type=\"as\" direction=\"out\"/></method><method name=\"Notify\"><arg type=\"s\" direction=\"in\"/><arg type=\"u\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"as\" direction=\"in\"/><arg type=\"a{sv}\" direction=\"in\"/><arg type=\"i\" direction=\"in\"/><arg type=\"u\" direction=\"out\"/></method><method name=\"CloseNotification\"><arg type=\"u\" direction=\"in\"/></method><method name=\"GetServerInformation\"><arg type=\"s\" direction=\"out\"/><arg type=\"s\" direction=\"out\"/><arg type=\"s\" direction=\"out\"/><arg type=\"s\" direction=\"out\"/></method><signal name=\"NotificationClosed\"><arg type=\"u\"/><arg type=\"u\"/></signal><signal name=\"ActionInvoked\"><arg type=\"u\"/><arg type=\"s\"/></signal></interface></node>"
                } else {
                    "<node><interface name=\"org.kde.StatusNotifierWatcher\"><method name=\"RegisterStatusNotifierItem\"><arg type=\"s\" direction=\"in\"/></method><method name=\"RegisterStatusNotifierHost\"><arg type=\"s\" direction=\"in\"/></method><property name=\"RegisteredStatusNotifierItems\" type=\"as\" access=\"read\"/><property name=\"IsStatusNotifierHostRegistered\" type=\"b\" access=\"read\"/><property name=\"ProtocolVersion\" type=\"i\" access=\"read\"/><signal name=\"StatusNotifierItemRegistered\"><arg type=\"s\"/></signal><signal name=\"StatusNotifierItemUnregistered\"><arg type=\"s\"/></signal><signal name=\"StatusNotifierHostRegistered\"/></interface></node>"
                };
                self.bus.reply(&m, vec![Val::s(xml)]);
            }
            (NOTIF_IFACE, "GetCapabilities") => self.bus.reply(&m, vec![arr_s(&["actions", "body", "body-markup", "icon-static", "persistence"])]),
            (NOTIF_IFACE, "GetServerInformation") => self.bus.reply(&m, vec![Val::s("lumen-shell"), Val::s("lumen"), Val::s("0.2"), Val::s("1.2")]),
            (NOTIF_IFACE, "CloseNotification") => {
                let id = m.body.first().and_then(|v| v.as_i64()).unwrap_or(0) as u32;
                self.close(id, 3);
                fx.push(Effect::NotifGone(id));
                self.bus.reply(&m, vec![]);
            }
            (NOTIF_IFACE, "Notify") => {
                if m.body.len() < 8 {
                    self.bus.error(&m, "org.freedesktop.DBus.Error.InvalidArgs", "Notify needs 8 arguments");
                    return;
                }
                let g = |i: usize| m.body.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let replaces = m.body[1].as_i64().unwrap_or(0) as u32;
                let mut actions = vec![];
                if let Some(a) = m.body[5].as_arr() {
                    for p in a.chunks(2) {
                        if let (Some(k), Some(l)) = (p.first().and_then(|v| v.as_str()), p.get(1).and_then(|v| v.as_str())) {
                            actions.push((k.to_string(), l.to_string()));
                        }
                    }
                }
                let hints = &m.body[6];
                let urgency = hints.dict_get("urgency").and_then(|v| v.as_i64()).unwrap_or(1).clamp(0, 2) as u8;
                let transient = hints.dict_get("transient").and_then(|v| v.as_bool()).unwrap_or(false);
                let desktop_entry = hints.dict_get("desktop-entry").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let mut icon = g(2);
                if icon.is_empty() {
                    icon = hints.dict_get("image-path").and_then(|v| v.as_str()).unwrap_or("").trim_start_matches("file://").to_string();
                }
                let image = hints.dict_get("image-data").or_else(|| hints.dict_get("image_data")).or_else(|| hints.dict_get("icon_data")).and_then(|v| {
                    let f = v.as_arr()?;
                    let (w, h, rs, alpha) = (f.first()?.as_i64()?, f.get(1)?.as_i64()?, f.get(2)?.as_i64()?, f.get(3)?.as_bool()?);
                    let ch = f.get(5)?.as_i64()?;
                    let data = match f.get(6)? {
                        Val::Arr(_, b) => b,
                        _ => return None,
                    };
                    if w <= 0 || h <= 0 || w > 512 || h > 512 || ch < 3 || rs < w * ch {
                        return None;
                    }
                    let mut px = Vec::with_capacity((w * h) as usize);
                    for y in 0..h {
                        for x in 0..w {
                            let o = (y * rs + x * ch) as usize;
                            let b = |k: usize| if let Some(Val::U8(v)) = data.get(o + k) { *v as u32 } else { 0 };
                            let a = if alpha && ch >= 4 { b(3) } else { 255 };
                            px.push((a << 24) | (b(0) << 16) | (b(1) << 8) | b(2));
                        }
                    }
                    Some(Image { w: w as usize, h: h as usize, px })
                });
                let expire = m.body[7].as_i64().unwrap_or(-1) as i32;
                let id = if replaces != 0 && self.notifs.iter().any(|n| n.id == replaces) {
                    replaces
                } else {
                    let i = self.next_id;
                    self.next_id = self.next_id.wrapping_add(1).max(1);
                    i
                };
                let n = Notif {
                    id,
                    app: g(0),
                    icon,
                    summary: strip_markup(&g(3)),
                    body: strip_markup(&g(4)),
                    actions,
                    urgency,
                    expire_ms: expire,
                    time: crate::sys::now_epoch(),
                    transient,
                    desktop_entry,
                    data: String::new(),
                    banner_until: if self.dnd && urgency < 2 {
                        None
                    } else {
                        Some(Instant::now() + if urgency == 2 { Duration::from_secs(3600) } else if expire > 0 { Duration::from_millis(expire as u64).max(Duration::from_secs(4)).min(Duration::from_secs(20)) } else { Duration::from_secs(6) })
                    },
                    image,
                };
                if let Some(old) = self.notifs.iter_mut().find(|x| x.id == id) {
                    *old = n;
                } else {
                    self.notifs.push(n);
                    if self.notifs.len() > 200 {
                        self.notifs.remove(0);
                    }
                }
                fx.push(Effect::NotifNew(id));
                self.bus.reply(&m, vec![Val::U32(id)]);
            }
            ("org.freedesktop.DBus.Properties", "Get") if m.path == WATCHER_PATH => {
                let p = m.body.get(1).and_then(|v| v.as_str()).unwrap_or("");
                match self.watcher_prop(p) {
                    Some(v) => self.bus.reply(&m, vec![Val::Var(Box::new(v))]),
                    None => self.bus.error(&m, "org.freedesktop.DBus.Error.UnknownProperty", p),
                }
            }
            ("org.freedesktop.DBus.Properties", "GetAll") if m.path == WATCHER_PATH => {
                let items: Vec<(&str, Val)> = ["RegisteredStatusNotifierItems", "IsStatusNotifierHostRegistered", "ProtocolVersion"].iter().filter_map(|k| self.watcher_prop(k).map(|v| (*k, v))).collect();
                self.bus.reply(&m, vec![dict_sv(items)]);
            }
            (WATCHER_IFACE, "RegisterStatusNotifierItem") => {
                let arg = m.body.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
                let (svc, path) = if arg.starts_with('/') {
                    (m.sender.clone(), arg.clone())
                } else if let Some(i) = arg.find('/') {
                    (arg[..i].to_string(), arg[i..].to_string())
                } else {
                    (arg.clone(), "/StatusNotifierItem".to_string())
                };
                if svc.is_empty() {
                    self.bus.error(&m, "org.freedesktop.DBus.Error.InvalidArgs", "empty service");
                    return;
                }
                let reg = if arg.starts_with('/') { format!("{}{}", m.sender, arg) } else { arg.clone() };
                if !self.items.iter().any(|i| i.reg == reg) {
                    let owner = if svc.starts_with(':') { svc.clone() } else { String::new() };
                    self.items.push(TrayItem { reg: reg.clone(), svc: svc.clone(), owner, path, status: "Active".into(), ..Default::default() });
                    self.bus.signal(WATCHER_PATH, WATCHER_IFACE, "StatusNotifierItemRegistered", vec![Val::Str(reg.clone())]);
                    if !svc.starts_with(':') {
                        self.bus.send_call(&format!("owner:{}", reg), "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner", vec![Val::Str(svc.clone())]);
                    }
                    self.fetch_item(&reg);
                }
                self.bus.reply(&m, vec![]);
            }
            (WATCHER_IFACE, "RegisterStatusNotifierHost") => self.bus.reply(&m, vec![]),
            _ => {
                if m.flags & 1 == 0 {
                    self.bus.error(&m, "org.freedesktop.DBus.Error.UnknownMethod", &format!("{}.{}", m.iface, m.member));
                }
            }
        }
    }

    fn watcher_prop(&self, name: &str) -> Option<Val> {
        Some(match name {
            "RegisteredStatusNotifierItems" => Val::Arr("s".into(), self.items.iter().map(|i| Val::Str(i.reg.clone())).collect()),
            "IsStatusNotifierHostRegistered" => Val::Bool(true),
            "ProtocolVersion" => Val::I32(0),
            _ => return None,
        })
    }

    fn fetch_item(&mut self, reg: &str) {
        if let Some(it) = self.items.iter().find(|i| i.reg == reg) {
            let (svc, path) = (it.svc.clone(), it.path.clone());
            self.bus.send_call(&format!("item:{}", reg), &svc, &path, "org.freedesktop.DBus.Properties", "GetAll", vec![Val::s("org.kde.StatusNotifierItem")]);
        }
    }

    // ---------- signals ----------
    fn on_signal(&mut self, m: Msg, fx: &mut Vec<Effect>) {
        match (m.iface.as_str(), m.member.as_str()) {
            ("org.freedesktop.DBus", "NameOwnerChanged") => {
                let name = m.body.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
                let new = m.body.get(2).and_then(|v| v.as_str()).unwrap_or("x");
                if name == "org.freedesktop.Notifications" && new == self.bus.unique {
                    self.have_notif_name = true;
                }
                if new.is_empty() {
                    let gone: Vec<String> = self.items.iter().filter(|i| i.svc == name || i.owner == name).map(|i| i.reg.clone()).collect();
                    for r in gone {
                        self.items.retain(|i| i.reg != r);
                        self.bus.signal(WATCHER_PATH, WATCHER_IFACE, "StatusNotifierItemUnregistered", vec![Val::Str(r)]);
                        fx.push(Effect::Tray);
                    }
                }
            }
            ("org.kde.StatusNotifierItem", _) => {
                let reg = self.items.iter().find(|i| i.owner == m.sender || (i.owner.is_empty() && i.svc == m.sender)).map(|i| i.reg.clone());
                if let Some(r) = reg {
                    self.fetch_item(&r);
                }
            }
            _ => {}
        }
    }

    // ---------- replies ----------
    fn on_reply(&mut self, tag: &str, m: Msg, fx: &mut Vec<Effect>) {
        if let Some(n) = tag.strip_prefix("reqname:") {
            let code = m.body.first().and_then(|v| v.as_i64()).unwrap_or(0);
            let ok = code == 1 || code == 4;
            if n == "org.freedesktop.Notifications" {
                self.have_notif_name = ok;
                if !ok {
                    fx.push(Effect::NameTaken);
                }
            } else if n == "org.kde.StatusNotifierWatcher" {
                self.have_watcher_name = ok;
            }
            return;
        }
        if let Some(reg) = tag.strip_prefix("owner:") {
            if let Some(o) = m.body.first().and_then(|v| v.as_str()) {
                if let Some(it) = self.items.iter_mut().find(|i| i.reg == reg) {
                    it.owner = o.to_string();
                }
            }
            return;
        }
        if let Some(reg) = tag.strip_prefix("item:") {
            if m.mtype == 3 {
                // item vanished / not an SNI: drop it
                self.items.retain(|i| i.reg != reg);
                fx.push(Effect::Tray);
                return;
            }
            let props = match m.body.first() {
                Some(p) => p.clone(),
                None => return,
            };
            if let Some(it) = self.items.iter_mut().find(|i| i.reg == reg) {
                let s = |k: &str| props.dict_get(k).and_then(|v| v.as_str()).map(|x| x.to_string());
                it.id = s("Id").unwrap_or_default();
                it.title = s("Title").unwrap_or_default();
                it.icon = s("IconName").unwrap_or_default();
                it.attention_icon = s("AttentionIconName").unwrap_or_default();
                it.theme_path = s("IconThemePath").unwrap_or_default();
                it.status = s("Status").unwrap_or_else(|| "Active".into());
                it.menu = s("Menu").unwrap_or_default();
                it.is_menu = props.dict_get("ItemIsMenu").and_then(|v| v.as_bool()).unwrap_or(false);
                it.pixmap = props.dict_get("IconPixmap").and_then(pixmap_to_image);
                if let Some(tt) = props.dict_get("ToolTip").and_then(|v| v.as_arr()) {
                    it.tooltip = tt.get(2).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if it.tooltip.is_empty() {
                        it.tooltip = tt.get(3).and_then(|v| v.as_str()).map(strip_markup).unwrap_or_default();
                    }
                }
                it.loaded = true;
                fx.push(Effect::Tray);
            }
            return;
        }
        if let Some(rest) = tag.strip_prefix("menu:") {
            if let Ok(idx) = rest.parse::<usize>() {
                if m.mtype == 2 {
                    if let Some(layout) = m.body.get(1) {
                        if let Some(root) = parse_menu(layout, 0) {
                            fx.push(Effect::MenuLoaded(idx, root));
                        }
                    }
                }
            }
            return;
        }
        if tag == "inhibit" {
            self.inhibit_pending = false;
            if m.mtype == 2 {
                self.cookie = m.body.first().and_then(|v| v.as_i64()).map(|c| c as u32);
                fx.push(Effect::Inhibited(true));
            } else {
                fx.push(Effect::Error("Keep-awake: no screensaver service answered".into()));
                fx.push(Effect::Inhibited(false));
            }
        }
    }

    // ---------- actions ----------
    pub fn request_notif_name(&mut self) {
        self.bus.request_name("org.freedesktop.Notifications");
    }

    pub fn close(&mut self, id: u32, reason: u32) {
        if self.notifs.iter().any(|n| n.id == id) {
            self.notifs.retain(|n| n.id != id);
            self.bus.signal(NOTIF_PATH, NOTIF_IFACE, "NotificationClosed", vec![Val::U32(id), Val::U32(reason)]);
        }
    }
    pub fn invoke(&mut self, id: u32, key: &str) {
        self.bus.signal(NOTIF_PATH, NOTIF_IFACE, "ActionInvoked", vec![Val::U32(id), Val::s(key)]);
    }
    pub fn clear_all(&mut self) {
        let ids: Vec<u32> = self.notifs.iter().map(|n| n.id).collect();
        for id in ids {
            self.close(id, 2);
        }
    }

    pub fn tray_activate(&mut self, i: usize, x: i32, y: i32, member: &str) {
        if let Some(it) = self.items.get(i) {
            let (svc, path) = (it.svc.clone(), it.path.clone());
            self.bus.send_call("", &svc, &path, "org.kde.StatusNotifierItem", member, vec![Val::I32(x), Val::I32(y)]);
        }
    }
    pub fn tray_scroll(&mut self, i: usize, delta: i32) {
        if let Some(it) = self.items.get(i) {
            let (svc, path) = (it.svc.clone(), it.path.clone());
            self.bus.send_call("", &svc, &path, "org.kde.StatusNotifierItem", "Scroll", vec![Val::I32(delta), Val::s("vertical")]);
        }
    }
    pub fn tray_menu(&mut self, i: usize) {
        if let Some(it) = self.items.get(i) {
            if it.menu.is_empty() || it.menu == "/NO_DBUSMENU" {
                return;
            }
            let (svc, menu) = (it.svc.clone(), it.menu.clone());
            self.bus.send_call("", &svc, &menu, "com.canonical.dbusmenu", "AboutToShow", vec![Val::I32(0)]);
            self.bus.send_call(&format!("menu:{}", i), &svc, &menu, "com.canonical.dbusmenu", "GetLayout", vec![Val::I32(0), Val::I32(-1), Val::Arr("s".into(), vec![])]);
        }
    }
    pub fn tray_menu_event(&mut self, i: usize, id: i32) {
        if let Some(it) = self.items.get(i) {
            let (svc, menu) = (it.svc.clone(), it.menu.clone());
            self.bus.send_call("", &svc, &menu, "com.canonical.dbusmenu", "Event", vec![Val::I32(id), Val::s("clicked"), Val::Var(Box::new(Val::I32(0))), Val::U32(crate::sys::now_epoch() as u32)]);
        }
    }

    pub fn inhibit(&mut self, on: bool) {
        if on && self.cookie.is_none() && !self.inhibit_pending {
            self.inhibit_pending = true;
            self.bus.send_call("inhibit", "org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver", "org.freedesktop.ScreenSaver", "Inhibit", vec![Val::s("lumen-shell"), Val::s("Keep awake")]);
        } else if !on {
            if let Some(c) = self.cookie.take() {
                self.bus.send_call("", "org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver", "org.freedesktop.ScreenSaver", "UnInhibit", vec![Val::U32(c)]);
            }
        }
    }
}
