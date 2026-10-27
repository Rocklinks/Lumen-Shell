//! Pure-Rust X11 wire-protocol backend (no Xlib/xcb). Panels are managed dock windows (strut-reserving);
//! "above fullscreen" panels get a second override-redirect twin that is raised only while a fullscreen
//! window is active. Popups/overlays are override-redirect windows.
use crate::gfx::{Canvas, Color};
use crate::platform::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const ATOM_CARDINAL: u32 = 6;
const ATOM_ATOM: u32 = 4;
const ATOM_WINDOW: u32 = 33;
const ATOM_STRING: u32 = 31;
const ATOM_WM_NORMAL_HINTS: u32 = 40;
const ATOM_WM_SIZE_HINTS: u32 = 41;
const ATOM_WM_NAME: u32 = 39;
const ATOM_WM_CLASS: u32 = 67;

const EM_KEY_PRESS: u32 = 1;
const EM_KEY_RELEASE: u32 = 2;
const EM_BUTTON_PRESS: u32 = 4;
const EM_BUTTON_RELEASE: u32 = 8;
const EM_ENTER: u32 = 0x10;
const EM_LEAVE: u32 = 0x20;
const EM_MOTION: u32 = 0x40;
const EM_EXPOSURE: u32 = 0x8000;
const EM_STRUCTURE: u32 = 0x20000;
const EM_SUBSTRUCTURE_NOTIFY: u32 = 0x80000;
const EM_SUBSTRUCTURE_REDIRECT: u32 = 0x100000;
const EM_PROPERTY: u32 = 0x400000;

struct Surf {
    win: u32,
    twin: Option<u32>, // override-redirect twin (above-fullscreen)
    gc: u32,
    tgc: u32,
    depth: u8,
    spec: SurfSpec,
    rect: (i32, i32, i32, i32),
    mapped: bool,
    want_visible: bool,
    last: Option<(Vec<u32>, usize, usize, Color)>,
    alpha: bool,
}

pub struct X11 {
    sock: UnixStream,
    outbuf: Vec<u8>,
    inbuf: Vec<u8>,
    seq: u16,
    base: u32,
    mask: u32,
    shift: u32,
    counter: u32,
    root: u32,
    root_w: i32,
    root_h: i32,
    root_visual: u32,
    root_depth: u8,
    argb_visual: Option<u32>,
    argb_cmap: u32,
    max_req: usize,
    min_kc: u8,
    max_kc: u8,
    keysyms: Vec<u32>,
    per_kc: usize,
    atoms: HashMap<String, u32>,
    surfs: HashMap<SurfId, Surf>,
    by_win: HashMap<u32, SurfId>,
    next_surf: SurfId,
    pending: Vec<Ev>,
    outputs: Vec<Output>,
    randr_major: u8,
    randr_event: u8,
    windows: Vec<WinInfo>,
    watched: Vec<u32>,
    active: u32,
    cur_desktop: usize,
    desktops: Vec<String>,
    comp_cached: bool,
    comp_checked: Instant,
    fs_active: bool,
    dirty_win: bool,
    dirty_desk: bool,
    dirty_out: bool,
    grabs: Vec<(u8, u8, u32)>, // keycode, mods(shift|ctrl|alt|super as xmask), id
}

type Res<T> = Result<T, String>;

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

impl X11 {
    pub fn connect() -> Res<X11> {
        let disp = std::env::var("DISPLAY").map_err(|_| "no DISPLAY")?;
        let (host, rest) = match disp.rsplit_once(':') {
            Some((h, r)) => (h.to_string(), r.to_string()),
            None => return Err("bad DISPLAY".into()),
        };
        let dnum: u32 = rest.split('.').next().unwrap_or("0").parse().map_err(|_| "bad display number")?;
        if !host.is_empty() && !host.starts_with('/') && host != "unix" {
            return Err("TCP X11 not supported".into());
        }
        let sock = UnixStream::connect(format!("/tmp/.X11-unix/X{}", dnum)).map_err(|e| format!("connect: {}", e))?;
        sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let (aname, adata) = read_xauth(dnum);
        let mut req = vec![b'l', 0, 11, 0, 0, 0];
        req.extend_from_slice(&(aname.len() as u16).to_le_bytes());
        req.extend_from_slice(&(adata.len() as u16).to_le_bytes());
        req.extend_from_slice(&[0, 0]);
        req.extend_from_slice(&aname);
        req.resize(pad4(req.len()), 0);
        req.extend_from_slice(&adata);
        req.resize(pad4(req.len()), 0);
        let mut s = sock;
        s.write_all(&req).map_err(|e| e.to_string())?;
        let mut head = [0u8; 8];
        s.read_exact(&mut head).map_err(|e| format!("setup: {}", e))?;
        let extra = u16::from_le_bytes([head[6], head[7]]) as usize * 4;
        let mut body = vec![0u8; extra];
        s.read_exact(&mut body).map_err(|e| format!("setup: {}", e))?;
        if head[0] != 1 {
            return Err(format!("X server refused: {}", String::from_utf8_lossy(&body[..(head[1] as usize).min(body.len())])));
        }
        // body offsets are (spec offset - 8)
        let u32l = |o: usize| u32::from_le_bytes([body[o], body[o + 1], body[o + 2], body[o + 3]]);
        let u16l = |o: usize| u16::from_le_bytes([body[o], body[o + 1]]);
        if body.len() < 40 {
            return Err("short setup".into());
        }
        let base = u32l(4);
        let mask = u32l(8);
        let vlen = u16l(16) as usize;
        let max_req = u16l(18) as usize * 4;
        let nroots = body[20] as usize;
        let nfmt = body[21] as usize;
        let min_kc = body[26];
        let max_kc = body[27];
        let mut off = 32 + pad4(vlen) + nfmt * 8;
        if nroots == 0 || off + 40 > body.len() {
            return Err("no screens".into());
        }
        let root = u32l(off);
        let root_w = u16l(off + 20) as i32;
        let root_h = u16l(off + 22) as i32;
        let root_visual = u32l(off + 32);
        let root_depth = body[off + 38];
        let ndepths = body[off + 39] as usize;
        off += 40;
        let mut argb = None;
        for _ in 0..ndepths {
            if off + 8 > body.len() {
                break;
            }
            let depth = body[off];
            let nv = u16l(off + 2) as usize;
            off += 8;
            for _ in 0..nv {
                if off + 24 > body.len() {
                    break;
                }
                let vid = u32l(off);
                let class = body[off + 4];
                let (rm, gm, bm) = (u32l(off + 8), u32l(off + 12), u32l(off + 16));
                if depth == 32 && class == 4 && rm == 0xff0000 && gm == 0xff00 && bm == 0xff && argb.is_none() {
                    argb = Some(vid);
                }
                off += 24;
            }
        }
        let mut x = X11 {
            sock: s,
            outbuf: Vec::new(),
            inbuf: Vec::new(),
            seq: 0,
            base,
            mask,
            shift: mask.trailing_zeros(),
            counter: 1,
            root,
            root_w,
            root_h,
            root_visual,
            root_depth,
            argb_visual: argb,
            argb_cmap: 0,
            max_req: max_req.max(16384),
            min_kc,
            max_kc,
            keysyms: vec![],
            per_kc: 0,
            atoms: HashMap::new(),
            surfs: HashMap::new(),
            by_win: HashMap::new(),
            next_surf: 1,
            pending: vec![],
            outputs: vec![],
            randr_major: 0,
            randr_event: 0,
            windows: vec![],
            watched: vec![],
            active: 0,
            cur_desktop: 0,
            desktops: vec![],
            comp_cached: false,
            comp_checked: Instant::now() - Duration::from_secs(60),
            fs_active: false,
            dirty_win: false,
            dirty_desk: false,
            dirty_out: false,
            grabs: vec![],
        };
        if let Some(v) = x.argb_visual {
            x.argb_cmap = x.alloc_id();
            let mut b = Vec::new();
            b.extend_from_slice(&x.argb_cmap.to_le_bytes());
            b.extend_from_slice(&x.root.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
            x.req(78, 0, &b);
        }
        for n in ["_NET_CURRENT_DESKTOP", "_NET_NUMBER_OF_DESKTOPS", "_NET_DESKTOP_NAMES", "_NET_CLIENT_LIST", "_NET_ACTIVE_WINDOW", "_NET_CLIENT_LIST_STACKING", "_XSETTINGS_SETTINGS", "RESOURCE_MANAGER", "_NET_WM_NAME", "_NET_WM_STATE", "_NET_WM_DESKTOP", "_NET_WM_WINDOW_TYPE"] {
            x.intern(n);
        }
        x.atoms.insert("WM_NAME".into(), ATOM_WM_NAME);
        x.atoms.insert("WM_CLASS".into(), ATOM_WM_CLASS);
        x.load_keymap();
        x.init_randr();
        x.refresh_outputs();
        // watch root for WM state
        x.change_attrs(root, &[(0x800, EM_PROPERTY | EM_SUBSTRUCTURE_NOTIFY | EM_STRUCTURE)]);
        x.refresh_desktops();
        x.refresh_windows();
        x.flush();
        x.sock.set_nonblocking(true).ok();
        Ok(x)
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.base | ((self.counter << self.shift) & self.mask);
        self.counter += 1;
        id
    }

    fn req(&mut self, opcode: u8, data: u8, body: &[u8]) -> u16 {
        let total = pad4(4 + body.len());
        let words = (total / 4) as u16;
        self.outbuf.push(opcode);
        self.outbuf.push(data);
        self.outbuf.extend_from_slice(&words.to_le_bytes());
        self.outbuf.extend_from_slice(body);
        self.outbuf.resize(self.outbuf.len() + (total - 4 - body.len()), 0);
        self.seq = self.seq.wrapping_add(1);
        if self.outbuf.len() > 1 << 20 {
            self.flush_inner();
        }
        self.seq
    }

    fn flush_inner(&mut self) {
        if self.outbuf.is_empty() {
            return;
        }
        let _ = self.sock.set_nonblocking(false);
        let r = self.sock.write_all(&self.outbuf);
        let _ = self.sock.set_nonblocking(true);
        if r.is_err() {
            self.pending.push(Ev::Closed { surf: 0 });
        }
        self.outbuf.clear();
    }

    /// Read one complete message (error/reply/event) blocking up to the read timeout.
    fn read_msg(&mut self, block: bool) -> Option<Vec<u8>> {
        loop {
            if self.inbuf.len() >= 32 {
                let mut total = 32;
                if self.inbuf[0] == 1 || self.inbuf[0] == 35 {
                    let l = u32::from_le_bytes([self.inbuf[4], self.inbuf[5], self.inbuf[6], self.inbuf[7]]) as usize;
                    total = 32 + l.min(1 << 24) * 4;
                }
                if self.inbuf.len() >= total {
                    let rest = self.inbuf.split_off(total);
                    let m = std::mem::replace(&mut self.inbuf, rest);
                    return Some(m);
                }
            }
            let mut tmp = [0u8; 16384];
            if block {
                let _ = self.sock.set_nonblocking(false);
            }
            let r = self.sock.read(&mut tmp);
            if block {
                let _ = self.sock.set_nonblocking(true);
            }
            match r {
                Ok(0) => {
                    self.pending.push(Ev::Closed { surf: 0 });
                    return None;
                }
                Ok(n) => self.inbuf.extend_from_slice(&tmp[..n]),
                Err(_) => return None,
            }
        }
    }

    /// Send and wait for the reply to `seq`. Events received meanwhile are processed.
    fn wait_reply(&mut self, seq: u16) -> Option<Vec<u8>> {
        self.flush_inner();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let m = self.read_msg(true)?;
            match m[0] {
                1 => {
                    if u16::from_le_bytes([m[2], m[3]]) == seq {
                        return Some(m);
                    }
                }
                0 => {
                    if u16::from_le_bytes([m[2], m[3]]) == seq {
                        return None;
                    }
                }
                _ => self.handle_event(&m),
            }
        }
        None
    }

    pub fn intern(&mut self, name: &str) -> u32 {
        if let Some(&a) = self.atoms.get(name) {
            return a;
        }
        let mut b = Vec::new();
        b.extend_from_slice(&(name.len() as u16).to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(name.as_bytes());
        let s = self.req(16, 0, &b);
        let a = self.wait_reply(s).map_or(0, |r| u32::from_le_bytes([r[8], r[9], r[10], r[11]]));
        self.atoms.insert(name.to_string(), a);
        a
    }

    /// Returns (format, type, bytes).
    fn get_prop(&mut self, win: u32, prop: u32, ty: u32, max_words: u32) -> Option<(u8, u32, Vec<u8>)> {
        let mut b = Vec::new();
        b.extend_from_slice(&win.to_le_bytes());
        b.extend_from_slice(&prop.to_le_bytes());
        b.extend_from_slice(&ty.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&max_words.to_le_bytes());
        let s = self.req(20, 0, &b);
        let r = self.wait_reply(s)?;
        let fmt = r[1];
        let rty = u32::from_le_bytes([r[8], r[9], r[10], r[11]]);
        let n = u32::from_le_bytes([r[16], r[17], r[18], r[19]]) as usize;
        let bytes = n * (fmt as usize / 8);
        if fmt == 0 || r.len() < 32 + bytes {
            return None;
        }
        Some((fmt, rty, r[32..32 + bytes].to_vec()))
    }

    fn prop_u32s(&mut self, win: u32, name: &str, ty: u32) -> Vec<u32> {
        let a = self.intern(name);
        match self.get_prop(win, a, ty, 4096) {
            Some((32, _, d)) => d.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => vec![],
        }
    }

    fn prop_string(&mut self, win: u32, name: &str) -> Option<String> {
        let a = self.intern(name);
        let u = self.intern("UTF8_STRING");
        let (_, _, d) = self.get_prop(win, a, u, 1024)?;
        Some(String::from_utf8_lossy(&d).to_string())
    }

    fn change_prop(&mut self, win: u32, prop: u32, ty: u32, fmt: u8, data: &[u8]) {
        let mut b = Vec::new();
        b.extend_from_slice(&win.to_le_bytes());
        b.extend_from_slice(&prop.to_le_bytes());
        b.extend_from_slice(&ty.to_le_bytes());
        b.push(fmt);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&((data.len() * 8 / fmt as usize) as u32).to_le_bytes());
        b.extend_from_slice(data);
        self.req(18, 0, &b);
    }
    fn set_u32s(&mut self, win: u32, name: &str, ty: u32, vals: &[u32]) {
        let a = self.intern(name);
        let d: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.change_prop(win, a, ty, 32, &d);
    }
    fn change_attrs(&mut self, win: u32, vals: &[(u32, u32)]) {
        let mut mask = 0;
        let mut b = Vec::new();
        b.extend_from_slice(&win.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        let mut sorted = vals.to_vec();
        sorted.sort();
        for (m, v) in sorted {
            mask |= m;
            b.extend_from_slice(&v.to_le_bytes());
        }
        b[4..8].copy_from_slice(&mask.to_le_bytes());
        self.req(2, 0, &b);
    }

    fn load_keymap(&mut self) {
        let count = self.max_kc.saturating_sub(self.min_kc) as u32 + 1;
        let b = [self.min_kc, count as u8, 0, 0];
        let s = self.req(101, 0, &b);
        if let Some(r) = self.wait_reply(s) {
            self.per_kc = r[1] as usize;
            self.keysyms = r[32..].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        }
    }

    fn init_randr(&mut self) {
        let name = b"RANDR";
        let mut b = Vec::new();
        b.extend_from_slice(&(name.len() as u16).to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(name);
        let s = self.req(98, 0, &b);
        if let Some(r) = self.wait_reply(s) {
            if r[8] != 0 {
                self.randr_major = r[9];
                self.randr_event = r[10];
                let mut v = Vec::new();
                v.extend_from_slice(&1u32.to_le_bytes());
                v.extend_from_slice(&5u32.to_le_bytes());
                let s = self.req(self.randr_major, 0, &v);
                let _ = self.wait_reply(s);
                let mut sel = Vec::new();
                sel.extend_from_slice(&self.root.to_le_bytes());
                sel.extend_from_slice(&[1, 0, 0, 0]); // RRScreenChangeNotifyMask
                self.req(self.randr_major, 4, &sel);
            }
        }
    }

    fn refresh_outputs(&mut self) {
        let mut outs = vec![];
        if self.randr_major != 0 {
            let mut b = Vec::new();
            b.extend_from_slice(&self.root.to_le_bytes());
            b.extend_from_slice(&[1, 0, 0, 0]);
            let s = self.req(self.randr_major, 42, &b);
            if let Some(r) = self.wait_reply(s) {
                let n = u32::from_le_bytes([r[12], r[13], r[14], r[15]]) as usize;
                let mut o = 32;
                for _ in 0..n {
                    if o + 24 > r.len() {
                        break;
                    }
                    let atom = u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
                    let primary = r[o + 4] != 0;
                    let nout = u16::from_le_bytes([r[o + 6], r[o + 7]]) as usize;
                    let x = i16::from_le_bytes([r[o + 8], r[o + 9]]) as i32;
                    let y = i16::from_le_bytes([r[o + 10], r[o + 11]]) as i32;
                    let w = u16::from_le_bytes([r[o + 12], r[o + 13]]) as i32;
                    let h = u16::from_le_bytes([r[o + 14], r[o + 15]]) as i32;
                    let name = self.atom_name(atom);
                    if w > 0 && h > 0 {
                        outs.push(Output { x, y, w, h, name, primary });
                    }
                    o += 24 + nout * 4;
                }
            }
        }
        if outs.is_empty() {
            outs.push(Output { x: 0, y: 0, w: self.root_w, h: self.root_h, name: "screen".into(), primary: true });
        }
        outs.sort_by_key(|o| (!o.primary, o.x, o.y));
        self.outputs = outs;
    }

    fn atom_name(&mut self, a: u32) -> String {
        let s = self.req(17, 0, &a.to_le_bytes());
        match self.wait_reply(s) {
            Some(r) => {
                let n = u16::from_le_bytes([r[8], r[9]]) as usize;
                String::from_utf8_lossy(&r[32..(32 + n).min(r.len())]).to_string()
            }
            None => "?".into(),
        }
    }

    fn refresh_desktops(&mut self) {
        let root = self.root;
        let n = self.prop_u32s(root, "_NET_NUMBER_OF_DESKTOPS", ATOM_CARDINAL).first().copied().unwrap_or(1).clamp(1, 64) as usize;
        self.cur_desktop = self.prop_u32s(root, "_NET_CURRENT_DESKTOP", ATOM_CARDINAL).first().copied().unwrap_or(0) as usize;
        let names = self.prop_string(root, "_NET_DESKTOP_NAMES").unwrap_or_default();
        let mut v: Vec<String> = names.split('\0').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
        while v.len() < n {
            v.push(format!("{}", v.len() + 1));
        }
        v.truncate(n);
        self.desktops = v;
    }

    fn refresh_windows(&mut self) {
        let root = self.root;
        let ids = self.prop_u32s(root, "_NET_CLIENT_LIST", ATOM_WINDOW);
        self.active = self.prop_u32s(root, "_NET_ACTIVE_WINDOW", ATOM_WINDOW).first().copied().unwrap_or(0);
        let own: Vec<u32> = self.by_win.keys().copied().collect();
        let mut out = vec![];
        let (t_dock, t_desk, t_notif, t_tip, t_menu, t_dd, t_popup, t_combo, t_splash, t_dnd) = (
            self.intern("_NET_WM_WINDOW_TYPE_DOCK"),
            self.intern("_NET_WM_WINDOW_TYPE_DESKTOP"),
            self.intern("_NET_WM_WINDOW_TYPE_NOTIFICATION"),
            self.intern("_NET_WM_WINDOW_TYPE_TOOLTIP"),
            self.intern("_NET_WM_WINDOW_TYPE_MENU"),
            self.intern("_NET_WM_WINDOW_TYPE_DROPDOWN_MENU"),
            self.intern("_NET_WM_WINDOW_TYPE_POPUP_MENU"),
            self.intern("_NET_WM_WINDOW_TYPE_COMBO"),
            self.intern("_NET_WM_WINDOW_TYPE_SPLASH"),
            self.intern("_NET_WM_WINDOW_TYPE_DND"),
        );
        let skip_types = [t_dock, t_desk, t_notif, t_tip, t_menu, t_dd, t_popup, t_combo, t_splash, t_dnd];
        let (s_skip, s_hidden, s_full, s_mv, s_mh) = (
            self.intern("_NET_WM_STATE_SKIP_TASKBAR"),
            self.intern("_NET_WM_STATE_HIDDEN"),
            self.intern("_NET_WM_STATE_FULLSCREEN"),
            self.intern("_NET_WM_STATE_MAXIMIZED_VERT"),
            self.intern("_NET_WM_STATE_MAXIMIZED_HORZ"),
        );
        let mut new_watch = vec![];
        for w in ids.iter().take(200) {
            if own.contains(w) {
                continue;
            }
            new_watch.push(*w);
            let types = self.prop_u32s(*w, "_NET_WM_WINDOW_TYPE", ATOM_ATOM);
            if types.iter().any(|t| skip_types.contains(t)) {
                continue;
            }
            let state = self.prop_u32s(*w, "_NET_WM_STATE", ATOM_ATOM);
            if state.contains(&s_skip) {
                continue;
            }
            let title = self.prop_string(*w, "_NET_WM_NAME").filter(|s| !s.is_empty()).or_else(|| {
                let (_, _, d) = self.get_prop(*w, ATOM_WM_NAME, ATOM_STRING, 256)?;
                Some(String::from_utf8_lossy(&d).to_string())
            });
            let class = self.get_prop(*w, ATOM_WM_CLASS, ATOM_STRING, 256).map(|(_, _, d)| {
                let parts: Vec<String> = d.split(|&c| c == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).to_string()).collect();
                parts.get(1).or(parts.first()).cloned().unwrap_or_default()
            });
            let desk = self.prop_u32s(*w, "_NET_WM_DESKTOP", ATOM_CARDINAL).first().copied().unwrap_or(0);
            out.push(WinInfo {
                id: *w as u64,
                title: title.unwrap_or_default(),
                app_id: class.unwrap_or_default(),
                active: *w == self.active,
                minimized: state.contains(&s_hidden),
                fullscreen: state.contains(&s_full),
                maximized: state.contains(&s_mv) && state.contains(&s_mh),
                workspace: if desk == 0xFFFFFFFF { -1 } else { desk as i32 },
            });
        }
        // subscribe to property changes on new clients
        let fresh: Vec<u32> = new_watch.iter().copied().filter(|w| !self.watched.contains(w)).collect();
        for w in fresh {
            self.change_attrs(w, &[(0x800, EM_PROPERTY | EM_STRUCTURE)]);
        }
        self.watched = new_watch;
        let changed = out != self.windows;
        self.windows = out;
        let cd = self.cur_desktop as i32;
        let fs = self.windows.iter().any(|w| w.fullscreen && !w.minimized && (w.workspace == cd || w.workspace == -1));
        let fs_changed = fs != self.fs_active;
        self.fs_active = fs;
        if changed || fs_changed {
            self.pending.push(Ev::WindowsChanged);
        }
        self.apply_fullscreen_overlay();
    }

    fn surf_alpha_possible(&self) -> bool {
        self.argb_visual.is_some() && self.comp_cached
    }

    fn compositing_now(&mut self) -> bool {
        let a = self.intern("_NET_WM_CM_S0");
        let s = self.req(23, 0, &a.to_le_bytes());
        self.wait_reply(s).map_or(false, |r| u32::from_le_bytes([r[8], r[9], r[10], r[11]]) != 0)
    }

    fn rect_for(&self, sp: &SurfSpec) -> (i32, i32, i32, i32) {
        let o = self.outputs.get(sp.out).or_else(|| self.outputs.first()).cloned().unwrap_or(Output { x: 0, y: 0, w: self.root_w, h: self.root_h, name: String::new(), primary: true });
        match sp.kind {
            Kind::Panel => {
                let (len_h, len_v) = (if sp.len > 0 { sp.len } else { o.w }, if sp.len > 0 { sp.len } else { o.h });
                match sp.edge {
                    Edge::Top => (o.x + (o.w - len_h) / 2, o.y + sp.inset, len_h, sp.h),
                    Edge::Bottom => (o.x + (o.w - len_h) / 2, o.y + o.h - sp.h - sp.inset, len_h, sp.h),
                    Edge::Left => (o.x, o.y + (o.h - len_v) / 2, sp.w, len_v),
                    Edge::Right => (o.x + o.w - sp.w, o.y + (o.h - len_v) / 2, sp.w, len_v),
                }
            }
            Kind::Popup => (o.x + sp.x, o.y + sp.y, sp.w.max(1), sp.h.max(1)),
            Kind::Overlay => (o.x + sp.x, o.y + sp.y, if sp.w > 0 { sp.w } else { o.w }, if sp.h > 0 { sp.h } else { o.h }),
            Kind::Catcher => (o.x, o.y, o.w, o.h),
        }
    }

    fn make_window(&mut self, rect: (i32, i32, i32, i32), alpha: bool, override_redirect: bool, input_only: bool, events: u32) -> (u32, u32, u8) {
        let id = self.alloc_id();
        let (depth, visual, class): (u8, u32, u16) = if input_only {
            (0, 0, 2)
        } else if alpha {
            (32, self.argb_visual.unwrap_or(self.root_visual), 1)
        } else {
            (self.root_depth, self.root_visual, 1)
        };
        let mut vals: Vec<(u32, u32)> = vec![(0x1, 0), (0x8, 0), (0x200, override_redirect as u32), (0x800, events)];
        if alpha && !input_only {
            vals.push((0x2000, self.argb_cmap));
        }
        if input_only {
            vals.retain(|v| v.0 != 0x1 && v.0 != 0x8);
        }
        vals.sort();
        let mut mask = 0;
        let mut b = Vec::new();
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&self.root.to_le_bytes());
        b.extend_from_slice(&(rect.0 as i16).to_le_bytes());
        b.extend_from_slice(&(rect.1 as i16).to_le_bytes());
        b.extend_from_slice(&(rect.2.clamp(1, 16000) as u16).to_le_bytes());
        b.extend_from_slice(&(rect.3.clamp(1, 16000) as u16).to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&class.to_le_bytes());
        b.extend_from_slice(&visual.to_le_bytes());
        let mpos = b.len();
        b.extend_from_slice(&0u32.to_le_bytes());
        for (m, v) in &vals {
            mask |= m;
            b.extend_from_slice(&v.to_le_bytes());
        }
        b[mpos..mpos + 4].copy_from_slice(&mask.to_le_bytes());
        self.req(1, depth, &b);
        let gc = if input_only {
            0
        } else {
            let g = self.alloc_id();
            let mut gb = Vec::new();
            gb.extend_from_slice(&g.to_le_bytes());
            gb.extend_from_slice(&id.to_le_bytes());
            gb.extend_from_slice(&0u32.to_le_bytes());
            self.req(55, 0, &gb);
            g
        };
        (id, gc, if input_only { 0 } else { depth.max(1) })
    }

    fn configure(&mut self, win: u32, r: (i32, i32, i32, i32)) {
        let mut b = Vec::new();
        b.extend_from_slice(&win.to_le_bytes());
        b.extend_from_slice(&0x000Fu16.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&r.0.to_le_bytes());
        b.extend_from_slice(&r.1.to_le_bytes());
        b.extend_from_slice(&(r.2.max(1) as u32).to_le_bytes());
        b.extend_from_slice(&(r.3.max(1) as u32).to_le_bytes());
        self.req(12, 0, &b);
    }
    fn raise(&mut self, win: u32) {
        let mut b = Vec::new();
        b.extend_from_slice(&win.to_le_bytes());
        b.extend_from_slice(&0x0040u16.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&0u32.to_le_bytes()); // Above
        self.req(12, 0, &b);
    }
    fn map(&mut self, win: u32) {
        self.req(8, 0, &win.to_le_bytes());
    }
    fn unmap(&mut self, win: u32) {
        self.req(10, 0, &win.to_le_bytes());
    }

    fn set_dock_props(&mut self, win: u32, sp: &SurfSpec, r: (i32, i32, i32, i32), name: &str) {
        let utf8 = self.intern("UTF8_STRING");
        let nm = self.intern("_NET_WM_NAME");
        self.change_prop(win, nm, utf8, 8, name.as_bytes());
        self.change_prop(win, ATOM_WM_NAME, ATOM_STRING, 8, name.as_bytes());
        let cls = format!("{}\0Lumen\0", name);
        self.change_prop(win, ATOM_WM_CLASS, ATOM_STRING, 8, cls.as_bytes());
        let dock = self.intern("_NET_WM_WINDOW_TYPE_DOCK");
        self.set_u32s(win, "_NET_WM_WINDOW_TYPE", ATOM_ATOM, &[dock]);
        let (above, sticky, skipt, skipp) = (self.intern("_NET_WM_STATE_ABOVE"), self.intern("_NET_WM_STATE_STICKY"), self.intern("_NET_WM_STATE_SKIP_TASKBAR"), self.intern("_NET_WM_STATE_SKIP_PAGER"));
        self.set_u32s(win, "_NET_WM_STATE", ATOM_ATOM, &[above, sticky, skipt, skipp]);
        self.set_u32s(win, "_NET_WM_DESKTOP", ATOM_CARDINAL, &[0xFFFFFFFF]);
        let hints: [u32; 18] = [1 | 2 | 4 | 8 | 16 | 32, r.0 as u32, r.1 as u32, r.2 as u32, r.3 as u32, r.2 as u32, r.3 as u32, r.2 as u32, r.3 as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let d: Vec<u8> = hints.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.change_prop(win, ATOM_WM_NORMAL_HINTS, ATOM_WM_SIZE_HINTS, 32, &d);
        self.set_struts(win, sp, r);
    }

    fn set_struts(&mut self, win: u32, sp: &SurfSpec, r: (i32, i32, i32, i32)) {
        let mut s = [0u32; 12];
        let ex = sp.exclusive.max(0) as u32;
        if ex > 0 {
            match sp.edge {
                Edge::Top => {
                    s[2] = r.1.max(0) as u32 + ex;
                    s[8] = r.0.max(0) as u32;
                    s[9] = (r.0 + r.2 - 1).max(0) as u32;
                }
                Edge::Bottom => {
                    s[3] = (self.root_h - r.1 - r.3).max(0) as u32 + ex;
                    s[10] = r.0.max(0) as u32;
                    s[11] = (r.0 + r.2 - 1).max(0) as u32;
                }
                Edge::Left => {
                    s[0] = r.0.max(0) as u32 + ex;
                    s[4] = r.1.max(0) as u32;
                    s[5] = (r.1 + r.3 - 1).max(0) as u32;
                }
                Edge::Right => {
                    s[1] = (self.root_w - r.0 - r.2).max(0) as u32 + ex;
                    s[6] = r.1.max(0) as u32;
                    s[7] = (r.1 + r.3 - 1).max(0) as u32;
                }
            }
        }
        self.set_u32s(win, "_NET_WM_STRUT_PARTIAL", ATOM_CARDINAL, &s);
        self.set_u32s(win, "_NET_WM_STRUT", ATOM_CARDINAL, &s[..4]);
    }

    fn put(&mut self, win: u32, gc: u32, depth: u8, px: &[u32], w: usize, h: usize, alpha: bool, bg: Color) {
        if w == 0 || h == 0 || px.len() < w * h {
            return;
        }
        let rows_per = ((self.max_req.saturating_sub(64)) / (w * 4)).max(1).min(h);
        let (br, bgc, bb) = ((bg >> 16) & 255, (bg >> 8) & 255, bg & 255);
        let mut y = 0;
        while y < h {
            let n = rows_per.min(h - y);
            let mut b = Vec::with_capacity(24 + n * w * 4);
            b.extend_from_slice(&win.to_le_bytes());
            b.extend_from_slice(&gc.to_le_bytes());
            b.extend_from_slice(&(w as u16).to_le_bytes());
            b.extend_from_slice(&(n as u16).to_le_bytes());
            b.extend_from_slice(&0i16.to_le_bytes());
            b.extend_from_slice(&(y as i16).to_le_bytes());
            b.push(0);
            b.push(depth);
            b.extend_from_slice(&[0, 0]);
            for p in &px[y * w..(y + n) * w] {
                let v = if alpha {
                    *p
                } else {
                    let ia = 255 - (p >> 24);
                    let r = (((p >> 16) & 255) + br * ia / 255).min(255);
                    let g = (((p >> 8) & 255) + bgc * ia / 255).min(255);
                    let bl = ((p & 255) + bb * ia / 255).min(255);
                    0xFF000000 | (r << 16) | (g << 8) | bl
                };
                b.extend_from_slice(&v.to_le_bytes());
            }
            self.req(72, 2, &b);
            y += n;
        }
    }

    fn apply_fullscreen_overlay(&mut self) {
        let want = self.fs_active;
        let ids: Vec<SurfId> = self.surfs.iter().filter(|(_, s)| s.spec.above_fullscreen && s.spec.kind == Kind::Panel).map(|(k, _)| *k).collect();
        for id in ids {
            let (create, twin, rect, spec, alpha, vis) = match self.surfs.get(&id) {
                Some(s) => (s.twin.is_none(), s.twin, s.rect, s.spec.clone(), s.alpha, s.want_visible),
                None => continue,
            };
            if want && vis {
                let tw = if create {
                    let (w, gc, _) = self.make_window(rect, alpha, true, false, EM_BUTTON_PRESS | EM_BUTTON_RELEASE | EM_ENTER | EM_LEAVE | EM_MOTION | EM_EXPOSURE);
                    self.by_win.insert(w, id);
                    if let Some(s) = self.surfs.get_mut(&id) {
                        s.twin = Some(w);
                        s.tgc = gc;
                    }
                    let _ = &spec;
                    w
                } else {
                    twin.unwrap_or(0)
                };
                self.configure(tw, rect);
                self.map(tw);
                self.raise(tw);
                if let Some(s) = self.surfs.get(&id) {
                    if let Some((px, w, h, bg)) = s.last.clone() {
                        let (gc, depth, al) = (s.tgc, s.depth, s.alpha);
                        self.put(tw, gc, depth, &px, w, h, al, bg);
                    }
                }
            } else if let Some(tw) = twin {
                self.unmap(tw);
            }
        }
    }

    fn keysym_for(&self, code: u8, state: u16) -> u32 {
        if self.per_kc == 0 || code < self.min_kc {
            return 0;
        }
        let i = (code - self.min_kc) as usize * self.per_kc;
        let c0 = self.keysyms.get(i).copied().unwrap_or(0);
        let c1 = if self.per_kc > 1 { self.keysyms.get(i + 1).copied().unwrap_or(0) } else { 0 };
        let shift = state & 1 != 0;
        let lock = state & 2 != 0;
        let letter = (0x61..=0x7a).contains(&c0);
        let up = shift ^ (lock && letter);
        if up {
            if c1 != 0 {
                c1
            } else if letter {
                c0 - 32
            } else {
                c0
            }
        } else {
            c0
        }
    }

    fn handle_event(&mut self, m: &[u8]) {
        let code = m[0] & 0x7f;
        let u16at = |o: usize| u16::from_le_bytes([m[o], m[o + 1]]);
        let i16at = |o: usize| i16::from_le_bytes([m[o], m[o + 1]]) as f32;
        let u32at = |o: usize| u32::from_le_bytes([m[o], m[o + 1], m[o + 2], m[o + 3]]);
        if self.randr_major != 0 && code == self.randr_event {
            self.dirty_out = true;
            return;
        }
        match code {
            2 | 3 | 4 | 5 | 6 | 7 | 8 => {
                let win = u32at(12);
                if code == 2 && win == self.root {
                    let st = u16at(28) & !(2 | 16);
                    if let Some(g) = self.grabs.iter().find(|g| g.0 == m[1] && g.1 as u16 == st & 0x4D) {
                        let id = g.2;
                        self.pending.push(Ev::Shortcut { id });
                    }
                    return;
                }
                let surf = match self.by_win.get(&win) {
                    Some(s) => *s,
                    None => return,
                };
                let (x, y) = (i16at(24), i16at(26));
                let state = u16at(28);
                let what = match code {
                    2 | 3 => {
                        if code == 2 {
                            let sym = self.keysym_for(m[1], state);
                            let text = sym_text(sym);
                            let mods = ((state & 1) as u8) | (((state >> 2) & 1) as u8) << 1 | (((state >> 3) & 1) as u8) << 2 | (((state >> 6) & 1) as u8) << 3;
                            self.pending.push(Ev::Key { surf, sym, text, ctrl: state & 4 != 0, mods });
                        }
                        return;
                    }
                    4 => match m[1] {
                        4 => Ptr::Scroll(-1),
                        5 => Ptr::Scroll(1),
                        6 | 7 => return,
                        b => Ptr::Down(b),
                    },
                    5 => match m[1] {
                        4..=7 => return,
                        b => Ptr::Up(b),
                    },
                    6 => Ptr::Move,
                    7 => Ptr::Enter,
                    _ => Ptr::Leave,
                };
                self.pending.push(Ev::Pointer { surf, x, y, what });
            }
            12 => {
                if u16at(16) == 0 {
                    if let Some(&surf) = self.by_win.get(&u32at(4)) {
                        // replay cached pixels directly (cheap) instead of asking UI
                        let rep = self.surfs.get(&surf).and_then(|s| s.last.as_ref().map(|l| (l.clone(), s.alpha, s.depth, s.gc, s.tgc, s.win, s.twin)));
                        match rep {
                            Some(((px, w, h, bg), al, depth, gc, tgc, win, twin)) => {
                                let ex = u32at(4);
                                if ex == win {
                                    self.put(win, gc, depth, &px, w, h, al, bg);
                                } else if Some(ex) == twin {
                                    self.put(ex, tgc, depth, &px, w, h, al, bg);
                                }
                            }
                            None => self.pending.push(Ev::Redraw { surf }),
                        }
                    }
                }
            }
            22 => {
                let win = u32at(8);
                if win == self.root {
                    let (w, h) = (u16at(20) as i32, u16at(22) as i32);
                    if w != self.root_w || h != self.root_h {
                        self.root_w = w;
                        self.root_h = h;
                        self.dirty_out = true;
                    }
                }
            }
            28 => {
                let win = u32at(4);
                let atom = u32at(8);
                let name = self.atom_name_cached(atom);
                if win == self.root {
                    match name.as_str() {
                        "_NET_CURRENT_DESKTOP" | "_NET_NUMBER_OF_DESKTOPS" | "_NET_DESKTOP_NAMES" => {
                            self.dirty_desk = true;
                            self.dirty_win = true;
                        }
                        "_NET_CLIENT_LIST" | "_NET_ACTIVE_WINDOW" | "_NET_CLIENT_LIST_STACKING" => self.dirty_win = true,
                        "_XSETTINGS_SETTINGS" | "RESOURCE_MANAGER" => self.pending.push(Ev::ThemeHint),
                        _ => {}
                    }
                } else if matches!(name.as_str(), "_NET_WM_NAME" | "WM_NAME" | "_NET_WM_STATE" | "_NET_WM_DESKTOP" | "WM_CLASS" | "_NET_WM_WINDOW_TYPE") {
                    self.dirty_win = true;
                }
            }
            _ => {}
        }
    }

    /// Cache-only lookup: never does a round trip (safe inside event handling).
    fn atom_name_cached(&self, a: u32) -> String {
        self.atoms.iter().find(|(_, v)| **v == a).map(|(k, _)| k.clone()).unwrap_or_default()
    }

    fn send_root_msg(&mut self, win: u32, msg: &str, data: [u32; 5]) {
        let ty = self.intern(msg);
        let mut ev = vec![33u8, 32];
        ev.extend_from_slice(&0u16.to_le_bytes());
        ev.extend_from_slice(&win.to_le_bytes());
        ev.extend_from_slice(&ty.to_le_bytes());
        for d in data {
            ev.extend_from_slice(&d.to_le_bytes());
        }
        let mut b = Vec::new();
        b.extend_from_slice(&self.root.to_le_bytes());
        b.extend_from_slice(&(EM_SUBSTRUCTURE_REDIRECT | EM_SUBSTRUCTURE_NOTIFY).to_le_bytes());
        b.extend_from_slice(&ev);
        let root = self.root;
        let mut full = vec![];
        full.extend_from_slice(&root.to_le_bytes());
        full.extend_from_slice(&(EM_SUBSTRUCTURE_REDIRECT | EM_SUBSTRUCTURE_NOTIFY).to_le_bytes());
        full.extend_from_slice(&ev);
        self.req(25, 0, &full);
        let _ = b;
    }
}

fn sym_text(sym: u32) -> String {
    let c = match sym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(sym),
        0x1000100..=0x110ffff => char::from_u32(sym - 0x1000000),
        0xffb0..=0xffb9 => char::from_u32(sym - 0xffb0 + 0x30),
        0xff0d | 0xff08 | 0xff1b | 0xff09 | 0xffff | 0xff51..=0xff58 => None,
        _ => None,
    };
    c.map(|c| c.to_string()).unwrap_or_default()
}

fn read_xauth(dnum: u32) -> (Vec<u8>, Vec<u8>) {
    let path = std::env::var("XAUTHORITY").ok().or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.Xauthority", h)));
    let data = match path.and_then(|p| std::fs::read(p).ok()) {
        Some(d) => d,
        None => return (vec![], vec![]),
    };
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_string();
    let mut o = 0;
    let be16 = |d: &[u8], o: usize| -> Option<usize> { Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]) as usize) };
    while o + 2 <= data.len() {
        let parse = (|| -> Option<(usize, usize, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
            let mut p = o;
            let fam = be16(&data, p)?;
            p += 2;
            let al = be16(&data, p)?;
            p += 2;
            let addr = data.get(p..p + al)?.to_vec();
            p += al;
            let nl = be16(&data, p)?;
            p += 2;
            let num = data.get(p..p + nl)?.to_vec();
            p += nl;
            let nml = be16(&data, p)?;
            p += 2;
            let name = data.get(p..p + nml)?.to_vec();
            p += nml;
            let dl = be16(&data, p)?;
            p += 2;
            let dat = data.get(p..p + dl)?.to_vec();
            p += dl;
            Some((fam, p, addr, num, name, dat))
        })();
        match parse {
            Some((fam, next, addr, num, name, dat)) => {
                let num_ok = String::from_utf8_lossy(&num).parse::<u32>().ok() == Some(dnum) || num.is_empty();
                let host_ok = fam == 0xFFFF || (fam == 256 && String::from_utf8_lossy(&addr) == host);
                if num_ok && host_ok && name == b"MIT-MAGIC-COOKIE-1" {
                    return (name, dat);
                }
                o = next;
            }
            None => break,
        }
    }
    (vec![], vec![])
}

struct RrRes {
    crtcs: Vec<u32>,
    outputs: Vec<u32>,
    modes: Vec<(u32, i32, i32, f32)>,
    config_ts: u32,
}

impl X11 {
    fn rr_resources(&mut self) -> Option<RrRes> {
        if self.randr_major == 0 {
            return None;
        }
        let root = self.root;
        let s = self.req(self.randr_major, 25, &root.to_le_bytes());
        let r = self.wait_reply(s)?;
        let u16at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]) as usize;
        let u32at = |o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
        let (nc, no, nm) = (u16at(16), u16at(18), u16at(20));
        let mut o = 32;
        let mut crtcs = vec![];
        for _ in 0..nc {
            if o + 4 > r.len() {
                return None;
            }
            crtcs.push(u32at(o));
            o += 4;
        }
        let mut outputs = vec![];
        for _ in 0..no {
            if o + 4 > r.len() {
                return None;
            }
            outputs.push(u32at(o));
            o += 4;
        }
        let mut modes = vec![];
        for _ in 0..nm {
            if o + 32 > r.len() {
                return None;
            }
            let id = u32at(o);
            let (w, h) = (u16at(o + 4) as i32, u16at(o + 6) as i32);
            let clock = u32at(o + 8) as f32;
            let (ht, vt) = (u16at(o + 16) as f32, u16at(o + 24) as f32);
            let flags = u32at(o + 28);
            let mut rf = if ht > 0.0 && vt > 0.0 { clock / (ht * vt) } else { 0.0 };
            if flags & 0x20 != 0 {
                rf /= 2.0;
            }
            if flags & 0x10 != 0 {
                rf *= 2.0;
            }
            modes.push((id, w, h, rf));
            o += 32;
        }
        Some(RrRes { crtcs, outputs, modes, config_ts: u32at(12) })
    }

    fn rr_crtc_info(&mut self, crtc: u32, ts: u32) -> Option<(i32, i32, i32, i32, u32, u16, Vec<u32>)> {
        let mut b = Vec::new();
        b.extend_from_slice(&crtc.to_le_bytes());
        b.extend_from_slice(&ts.to_le_bytes());
        let s = self.req(self.randr_major, 20, &b);
        let r = self.wait_reply(s)?;
        let u16at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
        let u32at = |o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
        let no = u16at(28) as usize;
        let outs = (0..no).filter_map(|i| r.get(32 + i * 4..36 + i * 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))).collect();
        Some((u16at(12) as i16 as i32, u16at(14) as i16 as i32, u16at(16) as i32, u16at(18) as i32, u32at(20), u16at(24), outs))
    }

    fn randr_outputs(&mut self) -> Option<Vec<DisplayOut>> {
        let res = self.rr_resources()?;
        let mut out = vec![];
        for oid in res.outputs.clone() {
            let mut b = Vec::new();
            b.extend_from_slice(&oid.to_le_bytes());
            b.extend_from_slice(&res.config_ts.to_le_bytes());
            let s = self.req(self.randr_major, 9, &b);
            let r = match self.wait_reply(s) {
                Some(r) => r,
                None => continue,
            };
            let u16at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]) as usize;
            let u32at = |o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
            let crtc = u32at(12);
            let connected = r[24] == 0;
            let (ncr, nmo, npref, ncl, nl) = (u16at(26), u16at(28), u16at(30), u16at(32), u16at(34));
            let mut o = 36 + ncr * 4;
            let mut mode_ids = vec![];
            for _ in 0..nmo {
                if o + 4 > r.len() {
                    break;
                }
                mode_ids.push(u32at(o));
                o += 4;
            }
            o += ncl * 4;
            let name = r.get(o..o + nl).map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
            let (mut x, mut y, mut cur_mode, mut rot) = (0, 0, 0u32, 1u16);
            if crtc != 0 {
                if let Some(ci) = self.rr_crtc_info(crtc, res.config_ts) {
                    x = ci.0;
                    y = ci.1;
                    cur_mode = ci.4;
                    rot = ci.5;
                }
            }
            let modes: Vec<DisplayMode> = mode_ids
                .iter()
                .enumerate()
                .filter_map(|(i, id)| res.modes.iter().find(|m| m.0 == *id).map(|m| DisplayMode { id: *id, w: m.1, h: m.2, refresh: m.3, current: *id == cur_mode, preferred: i < npref }))
                .collect();
            let primary = self.outputs.iter().any(|o2| o2.primary && o2.name == name);
            out.push(DisplayOut { name, x, y, connected, primary, rotation: rot, modes, scale: 1.0 });
        }
        Some(out)
    }

    fn randr_set_mode(&mut self, output: &str, mode: u32, rotation: u16) -> bool {
        let res = match self.rr_resources() {
            Some(r) => r,
            None => return false,
        };
        // find output id + its crtc
        let mut target: Option<(u32, u32)> = None;
        for oid in res.outputs.clone() {
            let mut b = Vec::new();
            b.extend_from_slice(&oid.to_le_bytes());
            b.extend_from_slice(&res.config_ts.to_le_bytes());
            let s = self.req(self.randr_major, 9, &b);
            if let Some(r) = self.wait_reply(s) {
                let nl = u16::from_le_bytes([r[34], r[35]]) as usize;
                let ncr = u16::from_le_bytes([r[26], r[27]]) as usize;
                let nmo = u16::from_le_bytes([r[28], r[29]]) as usize;
                let ncl = u16::from_le_bytes([r[32], r[33]]) as usize;
                let name_off = 36 + ncr * 4 + nmo * 4 + ncl * 4;
                let name = r.get(name_off..name_off + nl).map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
                if name == output {
                    let mut crtc = u32::from_le_bytes([r[12], r[13], r[14], r[15]]);
                    if crtc == 0 && ncr > 0 {
                        crtc = u32::from_le_bytes([r[36], r[37], r[38], r[39]]);
                    }
                    target = Some((oid, crtc));
                    break;
                }
            }
        }
        let (oid, crtc) = match target {
            Some(t) if t.1 != 0 => t,
            _ => return false,
        };
        let m = match res.modes.iter().find(|m| m.0 == mode) {
            Some(m) => *m,
            None => return false,
        };
        let rot = if rotation == 0 { 1 } else { rotation };
        let (mw, mh) = if rot & (2 | 8) != 0 { (m.2, m.1) } else { (m.1, m.2) };
        let info = self.rr_crtc_info(crtc, res.config_ts);
        let (cx, cy) = info.as_ref().map_or((0, 0), |i| (i.0, i.1));
        // grow the screen first if needed (never smaller than any other crtc's extent)
        let mut need_w = cx + mw;
        let mut need_h = cy + mh;
        for c in res.crtcs.clone() {
            if c == crtc {
                continue;
            }
            if let Some(ci) = self.rr_crtc_info(c, res.config_ts) {
                if ci.4 != 0 {
                    need_w = need_w.max(ci.0 + ci.2);
                    need_h = need_h.max(ci.1 + ci.3);
                }
            }
        }
        let (rw, rh) = (self.root_w.max(need_w), self.root_h.max(need_h));
        let set_size = |x: &mut X11, w: i32, h: i32| {
            let mut b = Vec::new();
            b.extend_from_slice(&x.root.to_le_bytes());
            b.extend_from_slice(&(w.clamp(1, 16000) as u16).to_le_bytes());
            b.extend_from_slice(&(h.clamp(1, 16000) as u16).to_le_bytes());
            b.extend_from_slice(&((w as f32 * 25.4 / 96.0) as u32).to_le_bytes());
            b.extend_from_slice(&((h as f32 * 25.4 / 96.0) as u32).to_le_bytes());
            x.req(x.randr_major, 7, &b);
        };
        if rw != self.root_w || rh != self.root_h {
            set_size(self, rw, rh);
        }
        let mut b = Vec::new();
        b.extend_from_slice(&crtc.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes()); // CurrentTime
        b.extend_from_slice(&res.config_ts.to_le_bytes());
        b.extend_from_slice(&(cx as i16).to_le_bytes());
        b.extend_from_slice(&(cy as i16).to_le_bytes());
        b.extend_from_slice(&mode.to_le_bytes());
        b.extend_from_slice(&rot.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&oid.to_le_bytes());
        let s = self.req(self.randr_major, 21, &b);
        let ok = self.wait_reply(s).map_or(false, |r| r[1] == 0);
        if ok {
            // shrink the screen to fit if the new mode is smaller
            if need_w != rw || need_h != rh {
                set_size(self, need_w, need_h);
            }
            self.dirty_out = true;
        }
        self.flush_inner();
        ok
    }
}

impl Platform for X11 {
    fn name(&self) -> &'static str {
        "x11"
    }
    fn fd(&self) -> i32 {
        self.sock.as_raw_fd()
    }
    fn dispatch(&mut self) -> Vec<Ev> {
        while let Some(m) = self.read_msg(false) {
            match m[0] {
                0 => {} // async error: ignore (e.g. BadWindow after destroy)
                1 => {}
                _ => self.handle_event(&m),
            }
        }
        for _ in 0..4 {
            if !(self.dirty_out || self.dirty_desk || self.dirty_win) {
                break;
            }
            if self.dirty_out {
                self.dirty_out = false;
                self.refresh_outputs();
                self.pending.push(Ev::OutputsChanged);
            }
            if self.dirty_desk {
                self.dirty_desk = false;
                self.refresh_desktops();
            }
            if self.dirty_win {
                self.dirty_win = false;
                self.refresh_windows();
            }
            while let Some(m) = self.read_msg(false) {
                if m[0] > 1 {
                    self.handle_event(&m);
                }
            }
        }
        // periodic compositing re-check
        if self.comp_checked.elapsed() > Duration::from_secs(2) {
            self.comp_checked = Instant::now();
            let c = self.compositing_now();
            if c != self.comp_cached {
                self.comp_cached = c;
                self.pending.push(Ev::OutputsChanged); // UI rebuilds surfaces with the right visual
            }
        }
        self.flush_inner();
        std::mem::take(&mut self.pending)
    }
    fn outputs(&self) -> Vec<Output> {
        self.outputs.clone()
    }
    fn create(&mut self, spec: &SurfSpec) -> SurfId {
        if self.comp_checked.elapsed() > Duration::from_secs(1) || self.surfs.is_empty() {
            self.comp_cached = self.compositing_now();
            self.comp_checked = Instant::now();
        }
        let rect = self.rect_for(spec);
        let alpha = self.surf_alpha_possible() && spec.kind != Kind::Catcher;
        let input_only = spec.kind == Kind::Catcher;
        let ev = match spec.kind {
            Kind::Catcher => EM_BUTTON_PRESS | EM_BUTTON_RELEASE,
            Kind::Overlay => EM_BUTTON_PRESS | EM_BUTTON_RELEASE | EM_MOTION | EM_KEY_PRESS | EM_KEY_RELEASE | EM_EXPOSURE,
            Kind::Popup if spec.keyboard => EM_BUTTON_PRESS | EM_BUTTON_RELEASE | EM_ENTER | EM_LEAVE | EM_MOTION | EM_KEY_PRESS | EM_KEY_RELEASE | EM_EXPOSURE,
            _ => EM_BUTTON_PRESS | EM_BUTTON_RELEASE | EM_ENTER | EM_LEAVE | EM_MOTION | EM_EXPOSURE,
        };
        let or = spec.kind != Kind::Panel;
        let (win, gc, depth) = self.make_window(rect, alpha, or, input_only, ev);
        let id = self.next_surf;
        self.next_surf += 1;
        if spec.kind == Kind::Panel {
            let name = if spec.edge == Edge::Top || spec.edge == Edge::Left && false { "lumen-bar" } else { "lumen-dock" };
            self.set_dock_props(win, spec, rect, if spec.exclusive >= 0 && spec.edge == Edge::Top { "lumen-bar" } else { name });
        }
        self.by_win.insert(win, id);
        self.surfs.insert(id, Surf { win, twin: None, gc, tgc: 0, depth, spec: spec.clone(), rect, mapped: false, want_visible: false, last: None, alpha });
        self.flush_inner();
        id
    }
    fn update(&mut self, id: SurfId, spec: &SurfSpec) {
        let rect = self.rect_for(spec);
        let (win, twin, changed_ex) = match self.surfs.get_mut(&id) {
            Some(s) => {
                let ce = s.spec.exclusive != spec.exclusive || s.rect != rect || s.spec.edge != spec.edge;
                s.spec = spec.clone();
                s.rect = rect;
                (s.win, s.twin, ce)
            }
            None => return,
        };
        self.configure(win, rect);
        if let Some(t) = twin {
            self.configure(t, rect);
        }
        if spec.kind == Kind::Panel && changed_ex {
            self.set_struts(win, spec, rect);
            let hints: [u32; 18] = [1 | 2 | 4 | 8 | 16 | 32, rect.0 as u32, rect.1 as u32, rect.2 as u32, rect.3 as u32, rect.2 as u32, rect.3 as u32, rect.2 as u32, rect.3 as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let d: Vec<u8> = hints.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.change_prop(win, ATOM_WM_NORMAL_HINTS, ATOM_WM_SIZE_HINTS, 32, &d);
        }
        if let Some(s) = self.surfs.get_mut(&id) {
            s.last = None;
        }
    }
    fn destroy(&mut self, id: SurfId) {
        if let Some(s) = self.surfs.remove(&id) {
            for w in [Some(s.win), s.twin].into_iter().flatten() {
                self.by_win.remove(&w);
                self.req(4, 0, &w.to_le_bytes());
            }
            for g in [s.gc, s.tgc] {
                if g != 0 {
                    self.req(60, 0, &g.to_le_bytes());
                }
            }
        }
    }
    fn set_visible(&mut self, id: SurfId, v: bool) {
        let (win, twin, kind, was, kb) = match self.surfs.get_mut(&id) {
            Some(s) => {
                let was = s.mapped;
                s.want_visible = v;
                s.mapped = v;
                (s.win, s.twin, s.spec.kind, was, s.spec.keyboard)
            }
            None => return,
        };
        if v && !was {
            self.map(win);
            if kind != Kind::Panel {
                self.raise(win);
            }
            if kind == Kind::Overlay || kb {
                let mut b = Vec::new();
                b.extend_from_slice(&win.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes());
                self.req(42, 1, &b); // SetInputFocus, revert PointerRoot
            }
        } else if !v && was {
            self.unmap(win);
            if let Some(t) = twin {
                self.unmap(t);
            }
        }
        self.apply_fullscreen_overlay();
    }
    fn present(&mut self, id: SurfId, cv: &Canvas, opaque_bg: Color) {
        let (win, gc, depth, alpha, twin, tgc, kind, vis) = match self.surfs.get(&id) {
            Some(s) => (s.win, s.gc, s.depth, s.alpha, s.twin, s.tgc, s.spec.kind, s.mapped),
            None => return,
        };
        if gc == 0 {
            return;
        }
        self.put(win, gc, depth, &cv.px, cv.w, cv.h, alpha, opaque_bg);
        if let Some(t) = twin {
            if self.fs_active && vis {
                self.put(t, tgc, depth, &cv.px, cv.w, cv.h, alpha, opaque_bg);
            }
        }
        if kind == Kind::Panel {
            if let Some(s) = self.surfs.get_mut(&id) {
                s.last = Some((cv.px.clone(), cv.w, cv.h, opaque_bg));
            }
        }
    }
    fn windows(&self) -> Vec<WinInfo> {
        self.windows.clone()
    }
    fn activate(&mut self, w: u64) {
        self.send_root_msg(w as u32, "_NET_ACTIVE_WINDOW", [2, 0, 0, 0, 0]);
    }
    fn minimize(&mut self, w: u64) {
        self.send_root_msg(w as u32, "WM_CHANGE_STATE", [3, 0, 0, 0, 0]);
    }
    fn close_window(&mut self, w: u64) {
        self.send_root_msg(w as u32, "_NET_CLOSE_WINDOW", [0, 2, 0, 0, 0]);
    }
    fn workspaces(&self) -> (Vec<String>, usize) {
        (self.desktops.clone(), self.cur_desktop)
    }
    fn switch_workspace(&mut self, i: usize) {
        let root = self.root;
        self.send_root_msg(root, "_NET_CURRENT_DESKTOP", [i as u32, 0, 0, 0, 0]);
    }
    fn display_outputs(&mut self) -> Option<Vec<DisplayOut>> {
        self.randr_outputs()
    }
    fn display_set_mode(&mut self, output: &str, mode: u32, rotation: u16) -> bool {
        self.randr_set_mode(output, mode, rotation)
    }
    fn capture(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<Canvas> {
        let (w, h) = (w.clamp(1, 16000), h.clamp(1, 16000));
        let mut b = Vec::new();
        b.extend_from_slice(&self.root.to_le_bytes());
        b.extend_from_slice(&(x as i16).to_le_bytes());
        b.extend_from_slice(&(y as i16).to_le_bytes());
        b.extend_from_slice(&(w as u16).to_le_bytes());
        b.extend_from_slice(&(h as u16).to_le_bytes());
        b.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        let s = self.req(73, 2, &b);
        let r = self.wait_reply(s)?;
        if r.len() < 32 + (w * h * 4) as usize {
            return None;
        }
        let mut cv = Canvas::new(w as usize, h as usize);
        for (i, c) in r[32..32 + (w * h * 4) as usize].chunks_exact(4).enumerate() {
            cv.px[i] = 0xFF000000 | ((c[2] as u32) << 16) | ((c[1] as u32) << 8) | c[0] as u32;
        }
        Some(cv)
    }
    fn grab_shortcut(&mut self, id: u32, sym: u32, mods: u8) -> bool {
        let kc = match (0..self.keysyms.len() / self.per_kc.max(1)).find(|k| (0..self.per_kc).any(|j| self.keysyms.get(k * self.per_kc + j) == Some(&sym))) {
            Some(k) => self.min_kc as usize + k,
            None => return false,
        };
        let xm: u8 = (mods & 1) | ((mods >> 1 & 1) << 2) | ((mods >> 2 & 1) << 3) | ((mods >> 3 & 1) << 6);
        for extra in [0u16, 2, 16, 18] {
            let mut b = Vec::new();
            b.extend_from_slice(&self.root.to_le_bytes());
            b.extend_from_slice(&(xm as u16 | extra).to_le_bytes());
            b.extend_from_slice(&[kc as u8, 1, 1, 0, 0, 0]);
            self.req(33, 1, &b);
        }
        self.grabs.push((kc as u8, xm, id));
        self.flush_inner();
        true
    }
    fn ungrab_shortcuts(&mut self) {
        let list = std::mem::take(&mut self.grabs);
        for (kc, xm, _) in list {
            for extra in [0u16, 2, 16, 18] {
                let mut u = Vec::new();
                u.extend_from_slice(&self.root.to_le_bytes());
                u.extend_from_slice(&(xm as u16 | extra).to_le_bytes());
                u.extend_from_slice(&[0, 0]);
                self.req(34, kc, &u);
            }
        }
        self.flush_inner();
    }
    fn set_workspace_count(&mut self, n: usize) {
        let root = self.root;
        self.send_root_msg(root, "_NET_NUMBER_OF_DESKTOPS", [n.clamp(1, 16) as u32, 0, 0, 0, 0]);
    }
    fn can_alpha(&self) -> bool {
        self.surf_alpha_possible()
    }
    fn fullscreen_active(&self) -> bool {
        self.fs_active
    }
    fn flush(&mut self) {
        self.flush_inner();
    }
    fn set_color_temp(&mut self, kelvin: i32) -> bool {
        if self.randr_major == 0 {
            return false;
        }
        let root = self.root;
        let s = self.req(self.randr_major, 25, &root.to_le_bytes());
        let r = match self.wait_reply(s) {
            Some(r) => r,
            None => return false,
        };
        let ncrtc = u16::from_le_bytes([r[16], r[17]]) as usize;
        let crtcs: Vec<u32> = (0..ncrtc).filter_map(|i| r.get(32 + i * 4..36 + i * 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))).collect();
        let (rm, gm, bm) = crate::wl::kelvin_rgb(kelvin);
        let mut any = false;
        for c in crtcs {
            let s = self.req(self.randr_major, 22, &c.to_le_bytes());
            let size = match self.wait_reply(s) {
                Some(r) => u16::from_le_bytes([r[8], r[9]]) as usize,
                None => continue,
            };
            if size < 2 {
                continue;
            }
            let mut b = Vec::new();
            b.extend_from_slice(&c.to_le_bytes());
            b.extend_from_slice(&(size as u16).to_le_bytes());
            b.extend_from_slice(&[0, 0]);
            for m in [rm, gm, bm] {
                for k in 0..size {
                    let v = (k as f32 / (size - 1) as f32 * m * 65535.0).round().clamp(0.0, 65535.0) as u16;
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            self.req(self.randr_major, 24, &b);
            any = true;
        }
        self.flush_inner();
        any
    }
}
