//! Pure-Rust Wayland client backend (no libwayland). Uses wlr-layer-shell (panels/popups on TOP/OVERLAY),
//! wlr-foreign-toplevel (dock/appmenu), ext-workspace-v1, wlr-gamma-control (night light), cursor-shape.
//! xkbcommon is loaded at runtime if present; otherwise a US-layout fallback handles typing.
use crate::gfx::{Canvas, Color};
use crate::platform::*;
use crate::sys::{self, Shm};
use std::collections::HashMap;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

extern "C" {
    fn dlopen(name: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(h: *mut c_void, name: *const c_char) -> *mut c_void;
}

struct Xkb {
    ctx: *mut c_void,
    keymap: *mut c_void,
    state: *mut c_void,
    new_keymap: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, c_int) -> *mut c_void,
    new_state: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    update: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32, u32, u32) -> c_int,
    sym: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    utf8: unsafe extern "C" fn(*mut c_void, u32, *mut c_char, usize) -> c_int,
    unref_state: unsafe extern "C" fn(*mut c_void),
    unref_keymap: unsafe extern "C" fn(*mut c_void),
}

impl Xkb {
    fn load() -> Option<Xkb> {
        unsafe {
            let mut h = std::ptr::null_mut();
            for n in [&b"libxkbcommon.so.0\0"[..], &b"libxkbcommon.so\0"[..]] {
                h = dlopen(n.as_ptr() as *const c_char, 2);
                if !h.is_null() {
                    break;
                }
            }
            if h.is_null() {
                return None;
            }
            macro_rules! f {
                ($n:expr) => {{
                    let p = dlsym(h, concat!($n, "\0").as_ptr() as *const c_char);
                    if p.is_null() {
                        return None;
                    }
                    std::mem::transmute(p)
                }};
            }
            let ctx_new: unsafe extern "C" fn(c_int) -> *mut c_void = f!("xkb_context_new");
            let ctx = ctx_new(0);
            if ctx.is_null() {
                return None;
            }
            Some(Xkb {
                ctx,
                keymap: std::ptr::null_mut(),
                state: std::ptr::null_mut(),
                new_keymap: f!("xkb_keymap_new_from_string"),
                new_state: f!("xkb_state_new"),
                update: f!("xkb_state_update_mask"),
                sym: f!("xkb_state_key_get_one_sym"),
                utf8: f!("xkb_state_key_get_utf8"),
                unref_state: f!("xkb_state_unref"),
                unref_keymap: f!("xkb_keymap_unref"),
            })
        }
    }
    fn set_keymap(&mut self, text: &[u8]) {
        let mut v = text.to_vec();
        if let Some(p) = v.iter().position(|&b| b == 0) {
            v.truncate(p);
        }
        v.push(0);
        unsafe {
            let km = (self.new_keymap)(self.ctx, v.as_ptr() as *const c_char, 1, 0);
            if km.is_null() {
                return;
            }
            let st = (self.new_state)(km);
            if st.is_null() {
                (self.unref_keymap)(km);
                return;
            }
            if !self.state.is_null() {
                (self.unref_state)(self.state);
            }
            if !self.keymap.is_null() {
                (self.unref_keymap)(self.keymap);
            }
            self.keymap = km;
            self.state = st;
        }
    }
}

enum Arg<'a> {
    U(u32),
    I(i32),
    S(&'a str),
}

struct Buf {
    id: u32,
    pool: u32,
    shm: Shm,
    w: i32,
    h: i32,
    busy: bool,
}

struct Surf {
    wl: u32,
    ls: u32,
    spec: SurfSpec,
    configured: bool,
    w: i32,
    h: i32,
    visible: bool,
    mapped: bool,
    bufs: Vec<Buf>,
    last: Option<(Vec<u32>, usize, usize)>,
    pending_resize: bool,
    zero: bool, // catcher: transparent buffer
}

#[derive(Default)]
struct Cap {
    w: u32,
    h: u32,
    stride: u32,
    fmt: u32,
    have_buf: bool,
    buf_done: bool,
    ready: bool,
    failed: bool,
    flags: u32,
}

#[derive(Default, Clone)]
struct Out {
    wl: u32,
    name: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: i32,
    gamma: u32,
    gamma_size: u32,
}

#[derive(Default, Clone)]
struct Tl {
    title: String,
    app_id: String,
    maximized: bool,
    minimized: bool,
    fullscreen: bool,
    activated: bool,
    done: bool,
}

#[derive(Default, Clone)]
struct Ws {
    name: String,
    state: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum ObjKind {
    Display,
    Registry,
    Callback,
    Seat,
    Pointer,
    Keyboard,
    Output(usize),
    Layer(SurfId),
    Buffer,
    Toplevel,
    TlMgr,
    WsMgr,
    WsHandle,
    WsGroup,
    Gamma(usize),
    Frame,
    Other,
}

pub struct Wayland {
    sock: UnixStream,
    inbuf: Vec<u8>,
    fdq: Vec<i32>,
    out: Vec<u8>,
    next_id: u32,
    objs: HashMap<u32, ObjKind>,
    registry: u32,
    globals: HashMap<String, (u32, u32)>,
    compositor: u32,
    shm: u32,
    layer_shell: u32,
    layer_ver: u32,
    seat: u32,
    pointer: u32,
    keyboard: u32,
    tl_mgr: u32,
    ws_mgr: u32,
    gamma_mgr: u32,
    cursor_mgr: u32,
    cursor_dev: u32,
    outs: Vec<Out>,
    surfs: HashMap<SurfId, Surf>,
    by_wl: HashMap<u32, SurfId>,
    next_surf: SurfId,
    evs: Vec<Ev>,
    tls: HashMap<u32, Tl>,
    ws: Vec<(u32, Ws)>,
    ws_pending_done: bool,
    ptr_surf: Option<SurfId>,
    ptr_serial: u32,
    kb_surf: Option<SurfId>,
    xkb: Option<Xkb>,
    mods: u32,
    repeat: Option<(u32, Instant, bool)>, // keycode, next fire, started
    repeat_rate: i32,
    repeat_delay: i32,
    cb_done: Vec<u32>,
    fatal: bool,
    kelvin: i32,
    cursor_surf: u32,
    sc_mgr: u32,
    cap: Option<Cap>,
    out_globals: Vec<(u32, u32)>,
    bound: bool,
    last_xy: (f32, f32),
    repeat_sym: (u32, String),
}

type Res<T> = Result<T, String>;

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn u32(&mut self) -> u32 {
        let v = self.d.get(self.p..self.p + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        self.p += 4;
        v
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn str(&mut self) -> String {
        let n = self.u32() as usize;
        let s = self.d.get(self.p..self.p + n.saturating_sub(1)).map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
        self.p += (n + 3) & !3;
        s
    }
    fn arr(&mut self) -> &'a [u8] {
        let n = self.u32() as usize;
        let s = self.d.get(self.p..self.p + n).unwrap_or(&[]);
        self.p += (n + 3) & !3;
        s
    }
}

impl Wayland {
    pub fn connect() -> Res<Wayland> {
        let name = std::env::var("WAYLAND_DISPLAY").map_err(|_| "no WAYLAND_DISPLAY")?;
        let path = if name.starts_with('/') {
            name
        } else {
            let rt = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "no XDG_RUNTIME_DIR")?;
            format!("{}/{}", rt, name)
        };
        let sock = UnixStream::connect(&path).map_err(|e| format!("connect {}: {}", path, e))?;
        sock.set_nonblocking(true).ok();
        let mut w = Wayland {
            sock,
            inbuf: vec![],
            fdq: vec![],
            out: vec![],
            next_id: 2,
            objs: HashMap::new(),
            registry: 0,
            globals: HashMap::new(),
            compositor: 0,
            shm: 0,
            layer_shell: 0,
            layer_ver: 1,
            seat: 0,
            pointer: 0,
            keyboard: 0,
            tl_mgr: 0,
            ws_mgr: 0,
            gamma_mgr: 0,
            cursor_mgr: 0,
            cursor_dev: 0,
            outs: vec![],
            surfs: HashMap::new(),
            by_wl: HashMap::new(),
            next_surf: 1,
            evs: vec![],
            tls: HashMap::new(),
            ws: vec![],
            ws_pending_done: false,
            ptr_surf: None,
            ptr_serial: 0,
            kb_surf: None,
            xkb: Xkb::load(),
            mods: 0,
            repeat: None,
            repeat_rate: 25,
            repeat_delay: 400,
            cb_done: vec![],
            fatal: false,
            kelvin: 6500,
            cursor_surf: 0,
            sc_mgr: 0,
            cap: None,
            out_globals: vec![],
            bound: false,
            last_xy: (0.0, 0.0),
            repeat_sym: (0, String::new()),
        };
        w.objs.insert(1, ObjKind::Display);
        w.registry = w.new_obj(ObjKind::Registry);
        w.send(1, 1, &[Arg::U(w.registry)]);
        w.roundtrip()?;
        w.bind_globals()?;
        w.roundtrip()?;
        w.roundtrip()?;
        if w.layer_shell == 0 {
            return Err("compositor lacks wlr-layer-shell (needed for panels)".into());
        }
        if w.compositor == 0 || w.shm == 0 {
            return Err("compositor lacks wl_compositor/wl_shm".into());
        }
        Ok(w)
    }

    fn new_obj(&mut self, k: ObjKind) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.objs.insert(id, k);
        id
    }

    fn send(&mut self, obj: u32, op: u16, args: &[Arg]) {
        let mut body: Vec<u8> = vec![];
        for a in args {
            match a {
                Arg::U(v) => body.extend_from_slice(&v.to_le_bytes()),
                Arg::I(v) => body.extend_from_slice(&v.to_le_bytes()),
                Arg::S(s) => {
                    body.extend_from_slice(&(s.len() as u32 + 1).to_le_bytes());
                    body.extend_from_slice(s.as_bytes());
                    body.push(0);
                    while body.len() % 4 != 0 {
                        body.push(0);
                    }
                }
            }
        }
        let size = 8 + body.len();
        self.out.extend_from_slice(&obj.to_le_bytes());
        self.out.extend_from_slice(&(((size as u32) << 16) | op as u32).to_le_bytes());
        self.out.extend_from_slice(&body);
    }

    fn send_fd(&mut self, obj: u32, op: u16, args: &[Arg], fd: i32) {
        self.flush_out();
        let before = self.out.len();
        self.send(obj, op, args);
        let data = self.out.split_off(before);
        let n = sys::send_with_fds(self.sock.as_raw_fd(), &data, &[fd]);
        if n < 0 {
            self.fatal = true;
        }
    }

    fn flush_out(&mut self) {
        let mut off = 0;
        while off < self.out.len() {
            let n = sys::send_with_fds(self.sock.as_raw_fd(), &self.out[off..], &[]);
            if n > 0 {
                off += n as usize;
            } else {
                // would block: wait briefly for writability
                std::thread::sleep(Duration::from_millis(2));
                if n == 0 || off == 0 && self.fatal {
                    break;
                }
                let e = std::io::Error::last_os_error();
                if e.kind() != std::io::ErrorKind::WouldBlock {
                    self.fatal = true;
                    break;
                }
            }
        }
        self.out.drain(..off);
    }

    fn roundtrip(&mut self) -> Res<()> {
        let cb = self.new_obj(ObjKind::Callback);
        self.send(1, 0, &[Arg::U(cb)]);
        self.flush_out();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.cb_done.contains(&cb) {
            if Instant::now() > deadline {
                return Err("compositor roundtrip timeout".into());
            }
            sys::poll_fds(&[self.sock.as_raw_fd()], 100);
            self.pump();
            if self.fatal {
                return Err("connection lost".into());
            }
        }
        self.cb_done.retain(|c| *c != cb);
        Ok(())
    }

    fn bind(&mut self, iface: &str, max_ver: u32, kind: ObjKind) -> u32 {
        let (name, ver) = match self.globals.get(iface) {
            Some(g) => *g,
            None => return 0,
        };
        let v = ver.min(max_ver);
        let id = self.new_obj(kind);
        let r = self.registry;
        self.send(r, 0, &[Arg::U(name), Arg::S(iface), Arg::U(v), Arg::U(id)]);
        id
    }

    fn bind_globals(&mut self) -> Res<()> {
        self.compositor = self.bind("wl_compositor", 4, ObjKind::Other);
        self.shm = self.bind("wl_shm", 1, ObjKind::Other);
        self.layer_ver = self.globals.get("zwlr_layer_shell_v1").map_or(1, |g| g.1.min(4));
        self.layer_shell = self.bind("zwlr_layer_shell_v1", 4, ObjKind::Other);
        self.seat = self.bind("wl_seat", 5, ObjKind::Seat);
        self.tl_mgr = self.bind("zwlr_foreign_toplevel_manager_v1", 3, ObjKind::TlMgr);
        self.ws_mgr = self.bind("ext_workspace_manager_v1", 1, ObjKind::WsMgr);
        self.gamma_mgr = self.bind("zwlr_gamma_control_manager_v1", 1, ObjKind::Other);
        self.cursor_mgr = self.bind("wp_cursor_shape_manager_v1", 1, ObjKind::Other);
        self.sc_mgr = self.bind("zwlr_screencopy_manager_v1", 1, ObjKind::Other);
        let og = self.out_globals.clone();
        for (name, ver) in og {
            self.bind_output(name, ver);
        }
        self.bound = true;
        Ok(())
    }

    fn bind_output(&mut self, name: u32, ver: u32) {
        let idx = self.outs.len();
        let id = self.new_obj(ObjKind::Output(idx));
        let r = self.registry;
        self.send(r, 0, &[Arg::U(name), Arg::S("wl_output"), Arg::U(ver.min(4)), Arg::U(id)]);
        self.outs.push(Out { wl: id, scale: 1, ..Default::default() });
    }

    // ---------- input pump ----------
    fn pump(&mut self) {
        loop {
            let mut buf = vec![0u8; 65536];
            let (n, fds) = sys::recv_with_fds(self.sock.as_raw_fd(), &mut buf);
            self.fdq.extend(fds);
            if n > 0 {
                self.inbuf.extend_from_slice(&buf[..n as usize]);
            } else {
                if n == 0 {
                    self.fatal = true;
                }
                break;
            }
        }
        let mut off = 0;
        loop {
            if self.inbuf.len() < off + 8 {
                break;
            }
            let b = &self.inbuf[off..];
            let obj = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let w2 = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            let (size, op) = ((w2 >> 16) as usize, (w2 & 0xffff) as u16);
            if size < 8 {
                self.fatal = true;
                break;
            }
            if self.inbuf.len() < off + size {
                break;
            }
            let body: Vec<u8> = self.inbuf[off + 8..off + size].to_vec();
            off += size;
            self.handle_msg(obj, op, &body);
        }
        self.inbuf.drain(..off);
    }

    fn handle_msg(&mut self, obj: u32, op: u16, body: &[u8]) {
        let kind = match self.objs.get(&obj) {
            Some(k) => *k,
            None => return,
        };
        let mut rd = Rd { d: body, p: 0 };
        match kind {
            ObjKind::Display => match op {
                0 => {
                    let _o = rd.u32();
                    let code = rd.u32();
                    let msg = rd.str();
                    eprintln!("lumen: wayland protocol error {}: {}", code, msg);
                    self.fatal = true;
                }
                1 => {
                    let id = rd.u32();
                    self.objs.remove(&id);
                }
                _ => {}
            },
            ObjKind::Registry => match op {
                0 => {
                    let name = rd.u32();
                    let iface = rd.str();
                    let ver = rd.u32();
                    if iface == "wl_output" {
                        self.out_globals.push((name, ver));
                        if self.bound {
                            self.bind_output(name, ver);
                        }
                    } else {
                        self.globals.insert(iface, (name, ver));
                    }
                }
                _ => {}
            },
            ObjKind::Callback => {
                if op == 0 {
                    self.cb_done.push(obj);
                }
            }
            ObjKind::Seat => {
                if op == 0 {
                    let caps = rd.u32();
                    if caps & 1 != 0 && self.pointer == 0 {
                        let id = self.new_obj(ObjKind::Pointer);
                        self.pointer = id;
                        let seat = self.seat;
                        self.send(seat, 0, &[Arg::U(id)]);
                        if self.cursor_mgr != 0 {
                            let dev = self.new_obj(ObjKind::Other);
                            self.cursor_dev = dev;
                            let m = self.cursor_mgr;
                            self.send(m, 1, &[Arg::U(dev), Arg::U(id)]);
                        }
                    }
                    if caps & 2 != 0 && self.keyboard == 0 {
                        let id = self.new_obj(ObjKind::Keyboard);
                        self.keyboard = id;
                        let seat = self.seat;
                        self.send(seat, 1, &[Arg::U(id)]);
                    }
                }
            }
            ObjKind::Pointer => self.on_pointer(op, &mut rd),
            ObjKind::Keyboard => self.on_keyboard(op, &mut rd),
            ObjKind::Output(i) => {
                if let Some(o) = self.outs.get_mut(i) {
                    match op {
                        0 => {
                            o.x = rd.i32();
                            o.y = rd.i32();
                        }
                        1 => {
                            let flags = rd.u32();
                            let (w, h) = (rd.i32(), rd.i32());
                            if flags & 1 != 0 {
                                o.w = w;
                                o.h = h;
                            }
                        }
                        2 => {
                            self.evs.push(Ev::OutputsChanged);
                            self.setup_gamma(i);
                        }
                        3 => o.scale = rd.i32().max(1),
                        4 => o.name = rd.str(),
                        _ => {}
                    }
                }
            }
            ObjKind::Layer(sid) => match op {
                0 => {
                    let serial = rd.u32();
                    let (w, h) = (rd.u32() as i32, rd.u32() as i32);
                    self.send(obj, 6, &[Arg::U(serial)]);
                    let mut vis = false;
                    let mut changed = false;
                    let mut resized = false;
                    if let Some(s) = self.surfs.get_mut(&sid) {
                        let (ow, oh, was) = (s.w, s.h, s.configured);
                        let pend = s.pending_resize;
                        s.pending_resize = false;
                        if w > 0 {
                            s.w = w;
                        }
                        if h > 0 {
                            s.h = h;
                        }
                        s.configured = true;
                        vis = s.visible;
                        changed = !was || ow != s.w || oh != s.h;
                        resized = pend;
                    }
                    let (cw, ch) = self.surfs.get(&sid).map_or((w, h), |s| (s.w, s.h));
                    if changed {
                        self.evs.push(Ev::Configure { surf: sid, w: cw, h: ch });
                    }
                    if vis && (changed || resized) {
                        self.reattach(sid);
                    }
                }
                1 => {
                    self.evs.push(Ev::OutputsChanged);
                }
                _ => {}
            },
            ObjKind::Buffer => {
                if op == 0 {
                    for s in self.surfs.values_mut() {
                        for b in s.bufs.iter_mut() {
                            if b.id == obj {
                                b.busy = false;
                            }
                        }
                    }
                }
            }
            ObjKind::Frame => {
                if let Some(c) = &mut self.cap {
                    match op {
                        0 => {
                            c.fmt = rd.u32();
                            c.w = rd.u32();
                            c.h = rd.u32();
                            c.stride = rd.u32();
                            c.have_buf = true;
                        }
                        1 => c.flags = rd.u32(),
                        2 => c.ready = true,
                        3 => c.failed = true,
                        6 => c.buf_done = true,
                        _ => {}
                    }
                }
            }
            ObjKind::TlMgr => {
                if op == 0 {
                    let id = rd.u32();
                    self.objs.insert(id, ObjKind::Toplevel);
                    self.tls.insert(id, Tl::default());
                }
            }
            ObjKind::Toplevel => {
                let mut changed = false;
                match op {
                    0 => {
                        let t = rd.str();
                        if let Some(x) = self.tls.get_mut(&obj) {
                            x.title = t;
                        }
                    }
                    1 => {
                        let t = rd.str();
                        if let Some(x) = self.tls.get_mut(&obj) {
                            x.app_id = t;
                        }
                    }
                    4 => {
                        let a = rd.arr();
                        if let Some(x) = self.tls.get_mut(&obj) {
                            x.maximized = false;
                            x.minimized = false;
                            x.activated = false;
                            x.fullscreen = false;
                            for c in a.chunks_exact(4) {
                                match u32::from_le_bytes([c[0], c[1], c[2], c[3]]) {
                                    0 => x.maximized = true,
                                    1 => x.minimized = true,
                                    2 => x.activated = true,
                                    3 => x.fullscreen = true,
                                    _ => {}
                                }
                            }
                        }
                    }
                    5 => {
                        if let Some(x) = self.tls.get_mut(&obj) {
                            x.done = true;
                        }
                        changed = true;
                    }
                    6 => {
                        self.tls.remove(&obj);
                        self.send(obj, 7, &[]);
                        self.objs.remove(&obj);
                        changed = true;
                    }
                    _ => {}
                }
                if changed {
                    self.evs.push(Ev::WindowsChanged);
                }
            }
            ObjKind::WsMgr => match op {
                0 => {
                    let id = rd.u32();
                    self.objs.insert(id, ObjKind::WsGroup);
                }
                1 => {
                    let id = rd.u32();
                    self.objs.insert(id, ObjKind::WsHandle);
                    self.ws.push((id, Ws::default()));
                }
                2 => self.evs.push(Ev::WindowsChanged),
                _ => {}
            },
            ObjKind::WsHandle => match op {
                1 => {
                    let n = rd.str();
                    if let Some(w) = self.ws.iter_mut().find(|w| w.0 == obj) {
                        w.1.name = n;
                    }
                }
                3 => {
                    let st = rd.u32();
                    if let Some(w) = self.ws.iter_mut().find(|w| w.0 == obj) {
                        w.1.state = st;
                    }
                }
                5 => {
                    self.ws.retain(|w| w.0 != obj);
                    self.send(obj, 0, &[]);
                    self.objs.remove(&obj);
                }
                _ => {}
            },
            ObjKind::Gamma(i) => match op {
                0 => {
                    let size = rd.u32();
                    if let Some(o) = self.outs.get_mut(i) {
                        o.gamma_size = size;
                    }
                    let k = self.kelvin;
                    self.apply_gamma(i, k);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn setup_gamma(&mut self, i: usize) {
        if self.gamma_mgr == 0 || self.outs[i].gamma != 0 {
            return;
        }
        let id = self.new_obj(ObjKind::Gamma(i));
        let (m, o) = (self.gamma_mgr, self.outs[i].wl);
        self.send(m, 0, &[Arg::U(id), Arg::U(o)]);
        self.outs[i].gamma = id;
    }

    fn apply_gamma(&mut self, i: usize, kelvin: i32) {
        let (size, id) = match self.outs.get(i) {
            Some(o) if o.gamma != 0 && o.gamma_size > 1 => (o.gamma_size as usize, o.gamma),
            _ => return,
        };
        let (r, g, b) = kelvin_rgb(kelvin);
        let mut data: Vec<u8> = Vec::with_capacity(size * 6);
        for mult in [r, g, b] {
            for k in 0..size {
                let v = (k as f32 / (size - 1) as f32 * mult * 65535.0).round().clamp(0.0, 65535.0) as u16;
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        if let Some(mut shm) = Shm::new(data.len()) {
            let bytes: &mut [u32] = shm.as_u32_mut();
            for (k, ch) in data.chunks_exact(4).enumerate() {
                if k < bytes.len() {
                    bytes[k] = u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]);
                }
            }
            self.send_fd(id, 0, &[], shm.fd);
        }
    }

    fn set_cursor(&mut self) {
        if self.cursor_dev != 0 {
            let (d, s) = (self.cursor_dev, self.ptr_serial);
            self.send(d, 1, &[Arg::U(s), Arg::U(1)]);
            return;
        }
        if self.cursor_surf == 0 {
            // fallback: draw an arrow into a tiny shm surface
            let mut cv = Canvas::new(24, 24);
            let pts_o: Vec<(f32, f32)> = vec![(0.5, 0.5), (0.5, 18.5), (4.5, 14.5), (7.5, 21.0), (10.5, 19.6), (7.6, 13.5), (13.0, 13.5)];
            let mut so = crate::gfx::Shape::for_rect(0.0, 0.0, 24.0, 24.0, 0);
            so.polygon(&pts_o);
            cv.paint(&so, 0xFF000000);
            let pts_i: Vec<(f32, f32)> = vec![(2.0, 3.6), (2.0, 15.0), (5.0, 12.2), (8.2, 19.2), (9.2, 18.8), (6.2, 11.8), (10.6, 11.8)];
            let mut si = crate::gfx::Shape::for_rect(0.0, 0.0, 24.0, 24.0, 0);
            si.polygon(&pts_i);
            cv.paint(&si, 0xFFFFFFFF);
            if let Some(mut shm) = Shm::new(24 * 24 * 4) {
                shm.as_u32_mut().copy_from_slice(&cv.px);
                let wl = self.new_obj(ObjKind::Other);
                let c = self.compositor;
                self.send(c, 0, &[Arg::U(wl)]);
                let pool = self.new_obj(ObjKind::Other);
                let sh = self.shm;
                self.send_fd(sh, 0, &[Arg::U(pool), Arg::I(24 * 24 * 4)], shm.fd);
                let buf = self.new_obj(ObjKind::Other);
                self.send(pool, 0, &[Arg::U(buf), Arg::I(0), Arg::I(24), Arg::I(24), Arg::I(96), Arg::U(0)]);
                self.send(pool, 1, &[]);
                self.send(wl, 1, &[Arg::U(buf), Arg::I(0), Arg::I(0)]);
                self.send(wl, 6, &[]);
                self.cursor_surf = wl;
                self.flush_out();
            }
        }
        if self.cursor_surf != 0 {
            let (p, s, cs) = (self.pointer, self.ptr_serial, self.cursor_surf);
            self.send(p, 0, &[Arg::U(s), Arg::U(cs), Arg::I(1), Arg::I(1)]);
        }
    }

    fn on_pointer(&mut self, op: u16, rd: &mut Rd) {
        match op {
            0 => {
                let serial = rd.u32();
                let surf = rd.u32();
                let (x, y) = (rd.i32() as f32 / 256.0, rd.i32() as f32 / 256.0);
                self.ptr_serial = serial;
                self.last_xy = (x, y);
                self.ptr_surf = self.by_wl.get(&surf).copied();
                self.set_cursor();
                if let Some(s) = self.ptr_surf {
                    self.evs.push(Ev::Pointer { surf: s, x, y, what: Ptr::Enter });
                }
            }
            1 => {
                let _serial = rd.u32();
                if let Some(s) = self.ptr_surf.take() {
                    self.evs.push(Ev::Pointer { surf: s, x: -1.0, y: -1.0, what: Ptr::Leave });
                }
            }
            2 => {
                let _t = rd.u32();
                let (x, y) = (rd.i32() as f32 / 256.0, rd.i32() as f32 / 256.0);
                self.last_xy = (x, y);
                if let Some(s) = self.ptr_surf {
                    self.evs.push(Ev::Pointer { surf: s, x, y, what: Ptr::Move });
                }
            }
            3 => {
                let _serial = rd.u32();
                let _t = rd.u32();
                let btn = rd.u32();
                let st = rd.u32();
                let b = match btn {
                    0x110 => 1,
                    0x112 => 2,
                    0x111 => 3,
                    _ => return,
                };
                if let Some(s) = self.ptr_surf {
                    let (x, y) = self.last_xy;
                    self.evs.push(Ev::Pointer { surf: s, x, y, what: if st == 1 { Ptr::Down(b) } else { Ptr::Up(b) } });
                }
            }
            4 => {
                let _t = rd.u32();
                let axis = rd.u32();
                let v = rd.i32();
                if axis == 0 && v != 0 {
                    if let Some(s) = self.ptr_surf {
                        let (x, y) = self.last_xy;
                        self.evs.push(Ev::Pointer { surf: s, x, y, what: Ptr::Scroll(if v > 0 { 1 } else { -1 }) });
                    }
                }
            }
            _ => {}
        }
    }

    fn key_to_ev(&mut self, code: u32, pressed: bool) -> Option<(u32, String)> {
        let _ = pressed;
        if let Some(x) = &self.xkb {
            if !x.state.is_null() {
                unsafe {
                    let sym = (x.sym)(x.state, code + 8);
                    let mut buf = [0 as c_char; 16];
                    let n = (x.utf8)(x.state, code + 8, buf.as_mut_ptr(), 16);
                    let mut text = String::new();
                    if n > 0 {
                        let bytes: Vec<u8> = buf[..n as usize].iter().map(|c| *c as u8).collect();
                        text = String::from_utf8_lossy(&bytes).to_string();
                        if text.chars().next().map_or(true, |c| (c as u32) < 0x20 || c as u32 == 0x7f) {
                            text.clear();
                        }
                    }
                    return Some((sym, text));
                }
            }
        }
        // US fallback
        let shift = self.mods & 1 != 0 || self.mods & 2 != 0;
        let special = match code {
            1 => Some(0xff1b),
            14 => Some(0xff08),
            15 => Some(0xff09),
            28 | 96 => Some(0xff0d),
            102 => Some(0xff50),
            103 => Some(0xff52),
            104 => Some(0xff55),
            105 => Some(0xff51),
            106 => Some(0xff53),
            107 => Some(0xff57),
            108 => Some(0xff54),
            109 => Some(0xff56),
            111 => Some(0xffff),
            _ => None,
        };
        if let Some(s) = special {
            return Some((s, String::new()));
        }
        let (lo, hi): (&str, &str) = match code {
            2..=11 => ("1234567890", "!@#$%^&*()"),
            16..=25 => ("qwertyuiop", "QWERTYUIOP"),
            30..=38 => ("asdfghjkl", "ASDFGHJKL"),
            44..=50 => ("zxcvbnm", "ZXCVBNM"),
            12 => ("-", "_"),
            13 => ("=", "+"),
            26 => ("[", "{"),
            27 => ("]", "}"),
            39 => (";", ":"),
            40 => ("'", "\""),
            41 => ("`", "~"),
            43 => ("\\", "|"),
            51 => (",", "<"),
            52 => (".", ">"),
            53 => ("/", "?"),
            57 => (" ", " "),
            _ => return None,
        };
        let idx = match code {
            2..=11 => code - 2,
            16..=25 => code - 16,
            30..=38 => code - 30,
            44..=50 => code - 44,
            _ => 0,
        } as usize;
        let c = if shift { hi } else { lo }.chars().nth(idx)?;
        Some((c as u32, c.to_string()))
    }

    fn on_keyboard(&mut self, op: u16, rd: &mut Rd) {
        match op {
            0 => {
                let _fmt = rd.u32();
                let size = rd.u32() as usize;
                if self.fdq.is_empty() {
                    return;
                }
                let fd = self.fdq.remove(0);
                use std::os::unix::fs::FileExt;
                use std::os::unix::io::FromRawFd;
                let f = unsafe { std::fs::File::from_raw_fd(fd) };
                let mut buf = vec![0u8; size.min(4 << 20)];
                if f.read_exact_at(&mut buf, 0).is_ok() {
                    if let Some(x) = &mut self.xkb {
                        x.set_keymap(&buf);
                    }
                }
            }
            1 => {
                let _serial = rd.u32();
                let surf = rd.u32();
                self.kb_surf = self.by_wl.get(&surf).copied();
            }
            2 => {
                self.kb_surf = None;
                self.repeat = None;
            }
            3 => {
                let _serial = rd.u32();
                let _t = rd.u32();
                let key = rd.u32();
                let st = rd.u32();
                let surf = match self.kb_surf {
                    Some(s) => s,
                    None => return,
                };
                if st == 1 {
                    if let Some((sym, text)) = self.key_to_ev(key, true) {
                        let ctrl = self.mods & 4 != 0;
                        let m = self.mods;
                        let mods = ((m & 1) as u8) | (((m >> 2) & 1) as u8) << 1 | (((m >> 3) & 1) as u8) << 2 | (((m >> 6) & 1) as u8) << 3;
                        self.evs.push(Ev::Key { surf, sym, text: text.clone(), ctrl, mods });
                        let is_mod = (0xffe1..=0xffee).contains(&sym);
                        if !is_mod && self.repeat_rate > 0 {
                            self.repeat = Some((key, Instant::now() + Duration::from_millis(self.repeat_delay as u64), false));
                            self.repeat_sym = (sym, text);
                        }
                    }
                } else if self.repeat.map_or(false, |r| r.0 == key) {
                    self.repeat = None;
                }
            }
            4 => {
                let _serial = rd.u32();
                let (d, l, k, g) = (rd.u32(), rd.u32(), rd.u32(), rd.u32());
                if let Some(x) = &self.xkb {
                    if !x.state.is_null() {
                        unsafe {
                            (x.update)(x.state, d, l, k, g, 0, 0);
                        }
                    }
                }
                self.mods = d | l | k;
                self.mods = (d | l) | (k & 2);
            }
            5 => {
                self.repeat_rate = rd.i32();
                self.repeat_delay = rd.i32();
            }
            _ => {}
        }
    }

    // ---------- surfaces ----------
    fn apply_props(&mut self, sid: SurfId) {
        let (ls, sp, wl) = match self.surfs.get(&sid) {
            Some(s) => (s.ls, s.spec.clone(), s.wl),
            None => return,
        };
        let (top, bottom, left, right) = (1u32, 2u32, 4u32, 8u32);
        let (anchor, w, h, excl, mt, ml): (u32, u32, u32, i32, i32, i32);
        match sp.kind {
            Kind::Panel => {
                excl = sp.exclusive;
                mt = 0;
                ml = 0;
                match sp.edge {
                    Edge::Top | Edge::Bottom => {
                        let e = if sp.edge == Edge::Top { top } else { bottom };
                        if sp.len > 0 {
                            anchor = e;
                            w = sp.len as u32;
                        } else {
                            anchor = e | left | right;
                            w = 0;
                        }
                        h = sp.h as u32;
                    }
                    Edge::Left | Edge::Right => {
                        let e = if sp.edge == Edge::Left { left } else { right };
                        if sp.len > 0 {
                            anchor = e;
                            h = sp.len as u32;
                        } else {
                            anchor = e | top | bottom;
                            h = 0;
                        }
                        w = sp.w as u32;
                    }
                }
            }
            Kind::Popup | Kind::Overlay => {
                anchor = top | left;
                w = sp.w.max(1) as u32;
                h = sp.h.max(1) as u32;
                excl = -1;
                mt = sp.y;
                ml = sp.x;
            }
            Kind::Catcher => {
                excl = -1;
                if sp.w > 0 {
                    anchor = top | left;
                    w = sp.w as u32;
                    h = sp.h.max(1) as u32;
                    mt = sp.y;
                    ml = sp.x;
                } else {
                    anchor = top | bottom | left | right;
                    w = 0;
                    h = 0;
                    mt = 0;
                    ml = 0;
                }
            }
        }
        let kb = match sp.kind {
            Kind::Overlay => 1,
            _ if sp.keyboard => {
                if self.layer_ver >= 4 { 2 } else { 1 }
            }
            _ => 0,
        };
        self.send(ls, 0, &[Arg::U(w), Arg::U(h)]);
        self.send(ls, 1, &[Arg::U(anchor)]);
        self.send(ls, 2, &[Arg::I(excl)]);
        self.send(ls, 3, &[Arg::I(mt), Arg::I(0), Arg::I(0), Arg::I(ml)]);
        self.send(ls, 4, &[Arg::U(kb)]);
        self.send(wl, 6, &[]);
    }

    fn reattach(&mut self, sid: SurfId) {
        let (zero, last, w, h, kind) = match self.surfs.get(&sid) {
            Some(s) => (s.zero, s.last.clone(), s.w, s.h, s.spec.kind),
            None => return,
        };
        if zero || kind == Kind::Catcher {
            let cv = Canvas::new(w.max(1) as usize, h.max(1) as usize);
            self.attach(sid, &cv.px, cv.w, cv.h);
        } else if let Some((px, cw, ch)) = last {
            self.attach(sid, &px, cw, ch);
        }
    }

    fn attach(&mut self, sid: SurfId, px: &[u32], w: usize, h: usize) {
        if w == 0 || h == 0 || px.len() < w * h {
            return;
        }
        let (wl, bi) = {
            let s = match self.surfs.get_mut(&sid) {
                Some(s) => s,
                None => return,
            };
            let pos = s.bufs.iter().position(|b| !b.busy && b.w == w as i32 && b.h == h as i32);
            // drop idle buffers of the wrong size
            let mut dead = vec![];
            s.bufs.retain(|b| {
                let keep = b.busy || (b.w == w as i32 && b.h == h as i32);
                if !keep {
                    dead.push(b.id);
                }
                keep
            });
            let _ = pos;
            let idx = s.bufs.iter().position(|b| !b.busy && b.w == w as i32 && b.h == h as i32);
            for d in dead {
                self.out.extend_from_slice(&d.to_le_bytes());
                self.out.extend_from_slice(&((8u32 << 16) | 0).to_le_bytes());
            }
            (s.wl, idx)
        };
        let bi = match bi {
            Some(i) => i,
            None => {
                let nb = self.surfs.get(&sid).map_or(0, |s| s.bufs.len());
                if nb >= 4 {
                    return; // compositor is slow; drop frame (a later present will catch up)
                }
                let size = w * h * 4;
                let shm = match Shm::new(size) {
                    Some(s) => s,
                    None => return,
                };
                let pool = self.new_obj(ObjKind::Other);
                let sh = self.shm;
                self.send_fd(sh, 0, &[Arg::U(pool), Arg::I(size as i32)], shm.fd);
                let buf = self.new_obj(ObjKind::Buffer);
                self.send(pool, 0, &[Arg::U(buf), Arg::I(0), Arg::I(w as i32), Arg::I(h as i32), Arg::I((w * 4) as i32), Arg::U(0)]);
                self.send(pool, 1, &[]);
                if let Some(s) = self.surfs.get_mut(&sid) {
                    s.bufs.push(Buf { id: buf, pool, shm, w: w as i32, h: h as i32, busy: false });
                    s.bufs.len() - 1
                } else {
                    return;
                }
            }
        };
        let bid = {
            let s = self.surfs.get_mut(&sid).unwrap();
            let b = &mut s.bufs[bi];
            b.shm.as_u32_mut()[..w * h].copy_from_slice(&px[..w * h]);
            b.busy = true;
            s.mapped = true;
            b.id
        };
        self.send(wl, 1, &[Arg::U(bid), Arg::I(0), Arg::I(0)]);
        self.send(wl, 9, &[Arg::I(0), Arg::I(0), Arg::I(w as i32), Arg::I(h as i32)]);
        self.send(wl, 6, &[]);
    }
}

pub fn kelvin_rgb(k: i32) -> (f32, f32, f32) {
    let t = (k.clamp(1000, 10000) as f32) / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g = if t <= 66.0 { 99.470_8 * t.ln() - 161.119_57 } else { 288.122_17 * (t - 60.0).powf(-0.075_514_85) };
    let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { 138.517_73 * (t - 10.0).ln() - 305.044_8 };
    ((r / 255.0).clamp(0.0, 1.0), (g / 255.0).clamp(0.0, 1.0), (b / 255.0).clamp(0.0, 1.0))
}

impl Platform for Wayland {
    fn name(&self) -> &'static str {
        "wayland"
    }
    fn fd(&self) -> i32 {
        self.sock.as_raw_fd()
    }
    fn dispatch(&mut self) -> Vec<Ev> {
        self.pump();
        if let Some((code, next, started)) = self.repeat {
            if Instant::now() >= next && self.kb_surf.is_some() {
                let (sym, text) = self.repeat_sym.clone();
                let surf = self.kb_surf.unwrap();
                self.evs.push(Ev::Key { surf, sym, text, ctrl: self.mods & 4 != 0, mods: 0 });
                let iv = Duration::from_millis((1000 / self.repeat_rate.max(1)) as u64);
                self.repeat = Some((code, Instant::now() + iv, true));
                let _ = started;
            }
        }
        self.flush_out();
        if self.fatal {
            self.evs.push(Ev::Closed { surf: 0 });
        }
        std::mem::take(&mut self.evs)
    }
    fn repeat_pending(&self) -> bool {
        self.repeat.is_some()
    }
    fn outputs(&self) -> Vec<Output> {
        let mut v: Vec<Output> = self.outs.iter().filter(|o| o.w > 0 && o.h > 0).map(|o| Output { x: o.x, y: o.y, w: o.w / o.scale.max(1), h: o.h / o.scale.max(1), name: o.name.clone(), primary: false }).collect();
        if let Some(f) = v.first_mut() {
            f.primary = true;
        }
        v
    }
    fn create(&mut self, spec: &SurfSpec) -> SurfId {
        let sid = self.next_surf;
        self.next_surf += 1;
        let wl = self.new_obj(ObjKind::Other);
        let c = self.compositor;
        self.send(c, 0, &[Arg::U(wl)]);
        let ls = self.new_obj(ObjKind::Layer(sid));
        let output = self.outs.get(spec.out).map(|o| o.wl).unwrap_or(0);
        let layer = match spec.kind {
            Kind::Panel => if spec.above_fullscreen { 3 } else { 2 },
            _ => 3,
        };
        let shell = self.layer_shell;
        self.send(shell, 0, &[Arg::U(ls), Arg::U(wl), Arg::U(output), Arg::U(layer), Arg::S("lumen")]);
        let o = self.outputs().get(spec.out).cloned().or_else(|| self.outputs().first().cloned());
        let (iw, ih) = o.map_or((spec.w, spec.h), |o| (o.w, o.h));
        let _ = (iw, ih);
        let init_w = if spec.kind == Kind::Panel && spec.len == 0 { iw } else { spec.w };
        self.surfs.insert(sid, Surf { wl, ls, spec: spec.clone(), configured: false, w: init_w.max(1), h: spec.h.max(1), visible: false, mapped: false, bufs: vec![], last: None, pending_resize: false, zero: spec.kind == Kind::Catcher });
        self.by_wl.insert(wl, sid);
        self.apply_props(sid);
        self.flush_out();
        sid
    }
    fn update(&mut self, id: SurfId, spec: &SurfSpec) {
        if let Some(s) = self.surfs.get_mut(&id) {
            let (rw, rh) = match spec.kind {
                Kind::Panel => match spec.edge {
                    Edge::Top | Edge::Bottom => (if spec.len > 0 { spec.len } else { s.w }, spec.h),
                    _ => (spec.w, if spec.len > 0 { spec.len } else { s.h }),
                },
                _ => (spec.w.max(1), spec.h.max(1)),
            };
            s.pending_resize = s.configured && (rw != s.w || rh != s.h);
            s.spec = spec.clone();
            s.last = None;
        }
        self.apply_props(id);
        self.flush_out();
    }
    fn destroy(&mut self, id: SurfId) {
        if let Some(s) = self.surfs.remove(&id) {
            self.by_wl.remove(&s.wl);
            self.send(s.ls, 7, &[]);
            for b in &s.bufs {
                self.send(b.id, 0, &[]);
            }
            self.send(s.wl, 0, &[]);
            if self.ptr_surf == Some(id) {
                self.ptr_surf = None;
            }
            if self.kb_surf == Some(id) {
                self.kb_surf = None;
            }
            self.objs.remove(&s.ls);
            self.flush_out();
        }
    }
    fn set_visible(&mut self, id: SurfId, v: bool) {
        let (was, configured, wl, mapped, w, h, kind) = match self.surfs.get_mut(&id) {
            Some(s) => {
                let was = s.visible;
                s.visible = v;
                (was, s.configured, s.wl, s.mapped, s.w, s.h, s.spec.kind)
            }
            None => return,
        };
        if v && !was {
            if configured {
                // restore input, then show the last frame (or let the UI present a fresh one)
                if mapped {
                    self.send(wl, 5, &[Arg::U(0)]);
                }
                self.reattach(id);
            }
        } else if !v && was && mapped && kind != Kind::Catcher {
            // "Hidden" = fully transparent + click-through. Never unmap: a layer surface that unmaps must
            // wait for a new configure before it may map again, which broke temporary hiding.
            let region = self.new_obj(ObjKind::Other);
            let c = self.compositor;
            self.send(c, 1, &[Arg::U(region)]);
            self.send(wl, 5, &[Arg::U(region)]);
            self.send(region, 0, &[]);
            let blank = Canvas::new(w.max(1) as usize, h.max(1) as usize);
            self.attach(id, &blank.px, blank.w, blank.h);
        } else if !v && was && kind == Kind::Catcher {
            let region = self.new_obj(ObjKind::Other);
            let c = self.compositor;
            self.send(c, 1, &[Arg::U(region)]);
            self.send(wl, 5, &[Arg::U(region)]);
            self.send(region, 0, &[]);
            self.send(wl, 6, &[]);
        }
    }
    fn present(&mut self, id: SurfId, cv: &Canvas, _bg: Color) {
        let (configured, visible, kind, pending) = match self.surfs.get(&id) {
            Some(s) => (s.configured, s.visible, s.spec.kind, s.pending_resize),
            None => return,
        };
        if kind == Kind::Catcher {
            return;
        }
        if let Some(s) = self.surfs.get_mut(&id) {
            s.last = Some((cv.px.clone(), cv.w, cv.h));
        }
        if configured && visible && !pending {
            self.attach(id, &cv.px, cv.w, cv.h);
        }
    }
    fn windows(&self) -> Vec<WinInfo> {
        let mut v: Vec<WinInfo> = self.tls.iter().filter(|(_, t)| t.done).map(|(id, t)| WinInfo { id: *id as u64, title: t.title.clone(), app_id: t.app_id.clone(), active: t.activated, minimized: t.minimized, fullscreen: t.fullscreen, maximized: t.maximized, workspace: -1 }).collect();
        v.sort_by_key(|w| w.id);
        v
    }
    fn activate(&mut self, w: u64) {
        let id = w as u32;
        if self.tls.get(&id).map_or(false, |t| t.minimized) {
            self.send(id, 3, &[]);
        }
        let seat = self.seat;
        self.send(id, 4, &[Arg::U(seat)]);
        self.flush_out();
    }
    fn minimize(&mut self, w: u64) {
        self.send(w as u32, 2, &[]);
        self.flush_out();
    }
    fn close_window(&mut self, w: u64) {
        self.send(w as u32, 5, &[]);
        self.flush_out();
    }
    fn workspaces(&self) -> (Vec<String>, usize) {
        let vis: Vec<&(u32, Ws)> = self.ws.iter().filter(|w| w.1.state & 4 == 0).collect();
        let names = vis.iter().enumerate().map(|(i, w)| if w.1.name.is_empty() { format!("{}", i + 1) } else { w.1.name.clone() }).collect();
        let cur = vis.iter().position(|w| w.1.state & 1 != 0).unwrap_or(0);
        (names, cur)
    }
    fn switch_workspace(&mut self, i: usize) {
        let ids: Vec<u32> = self.ws.iter().filter(|w| w.1.state & 4 == 0).map(|w| w.0).collect();
        if let Some(h) = ids.get(i) {
            self.send(*h, 1, &[]);
            let m = self.ws_mgr;
            if m != 0 {
                self.send(m, 0, &[]);
            }
            self.flush_out();
        }
    }
    fn capture(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<Canvas> {
        if self.sc_mgr == 0 {
            eprintln!("lumen: compositor does not offer wlr-screencopy");
            return None;
        }
        let out = match self.outs.first() {
            Some(o) => o.wl,
            None => {
                eprintln!("lumen: screencopy: no outputs known");
                return None;
            }
        };
        let frame = self.new_obj(ObjKind::Frame);
        self.cap = Some(Cap::default());
        let m = self.sc_mgr;
        self.send(m, 0, &[Arg::U(frame), Arg::I(0), Arg::U(out)]);
        self.flush_out();
        let deadline = Instant::now() + Duration::from_secs(4);
        while !self.cap.as_ref().map_or(false, |c| c.have_buf || c.failed) {
            if Instant::now() > deadline {
                eprintln!("lumen: screencopy: no buffer description from compositor");
                self.cap = None;
                return None;
            }
            sys::poll_fds(&[self.sock.as_raw_fd()], 100);
            self.pump();
        }
        let (cw, ch, stride) = match &self.cap {
            Some(c) if !c.failed => (c.w as usize, c.h as usize, c.stride as usize),
            _ => {
                eprintln!("lumen: screencopy: compositor refused the capture");
                self.cap = None;
                return None;
            }
        };
        if cw == 0 || ch == 0 || cw > 16384 || ch > 16384 || stride < cw * 4 {
            self.cap = None;
            return None;
        }
        let size = stride * ch;
        let mut shm = match Shm::new(size) {
            Some(s) => s,
            None => {
                eprintln!("lumen: screencopy: shm allocation failed");
                return None;
            }
        };
        let pool = self.new_obj(ObjKind::Other);
        let sh = self.shm;
        self.send_fd(sh, 0, &[Arg::U(pool), Arg::I(size as i32)], shm.fd);
        let fmt = self.cap.as_ref().map_or(1, |c| c.fmt);
        let buf = self.new_obj(ObjKind::Other);
        self.send(pool, 0, &[Arg::U(buf), Arg::I(0), Arg::I(cw as i32), Arg::I(ch as i32), Arg::I(stride as i32), Arg::U(fmt)]);
        self.send(pool, 1, &[]);
        self.send(frame, 0, &[Arg::U(buf)]);
        self.flush_out();
        while !self.cap.as_ref().map_or(false, |c| c.ready || c.failed) {
            if Instant::now() > deadline {
                eprintln!("lumen: screencopy: timed out waiting for the frame");
                self.cap = None;
                return None;
            }
            sys::poll_fds(&[self.sock.as_raw_fd()], 100);
            self.pump();
        }
        let flags = self.cap.as_ref().map_or(0, |c| c.flags);
        let ok = self.cap.as_ref().map_or(false, |c| c.ready);
        self.cap = None;
        self.send(frame, 1, &[]);
        self.send(buf, 0, &[]);
        self.flush_out();
        if !ok {
            eprintln!("lumen: screencopy: copy failed");
            return None;
        }
        let words: &[u32] = shm.as_u32_mut();
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        let (rw, rh) = ((w.max(1) as usize).min(cw.saturating_sub(x0)), (h.max(1) as usize).min(ch.saturating_sub(y0)));
        if rw == 0 || rh == 0 {
            eprintln!("lumen: screencopy: empty region");
            return None;
        }
        let mut cv = Canvas::new(rw, rh);
        let swap_rb = fmt == 0x34324241 || fmt == 0x34324258;
        for yy in 0..rh {
            let sy = if flags & 1 != 0 { ch - 1 - (y0 + yy) } else { y0 + yy };
            for xx in 0..rw {
                let p = words.get(sy * (stride / 4) + x0 + xx).copied().unwrap_or(0);
                let p = if swap_rb { (p & 0xFF00FF00) | ((p & 0xFF) << 16) | ((p >> 16) & 0xFF) } else { p };
                cv.px[yy * rw + xx] = 0xFF000000 | (p & 0xFFFFFF);
            }
        }
        Some(cv)
    }
    fn can_alpha(&self) -> bool {
        true
    }
    fn fullscreen_active(&self) -> bool {
        self.tls.values().any(|t| t.done && t.fullscreen && !t.minimized)
    }
    fn flush(&mut self) {
        self.flush_out();
    }
    fn set_color_temp(&mut self, kelvin: i32) -> bool {
        self.kelvin = kelvin;
        if self.gamma_mgr == 0 {
            return false;
        }
        for i in 0..self.outs.len() {
            self.apply_gamma(i, kelvin);
        }
        self.flush_out();
        true
    }
}
