//! Minimal D-Bus client/server-side helper (session bus). Little-endian only, EXTERNAL auth, no fd passing.
//! Every read is bounds-checked; hostile/garbled messages are dropped, never panic.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    U8(u8),
    Bool(bool),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F64(f64),
    Str(String),
    Obj(String),
    Sig(String),
    Arr(String, Vec<Val>),
    Struct(Vec<Val>),
    Dict(Box<Val>, Box<Val>),
    Var(Box<Val>),
}

impl Val {
    pub fn s(x: &str) -> Val {
        Val::Str(x.to_string())
    }
    pub fn sig(&self) -> String {
        match self {
            Val::U8(_) => "y".into(),
            Val::Bool(_) => "b".into(),
            Val::I16(_) => "n".into(),
            Val::U16(_) => "q".into(),
            Val::I32(_) => "i".into(),
            Val::U32(_) => "u".into(),
            Val::I64(_) => "x".into(),
            Val::U64(_) => "t".into(),
            Val::F64(_) => "d".into(),
            Val::Str(_) => "s".into(),
            Val::Obj(_) => "o".into(),
            Val::Sig(_) => "g".into(),
            Val::Arr(e, _) => format!("a{}", e),
            Val::Struct(v) => format!("({})", v.iter().map(|x| x.sig()).collect::<String>()),
            Val::Dict(k, v) => format!("{{{}{}}}", k.sig(), v.sig()),
            Val::Var(_) => "v".into(),
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) | Val::Obj(s) | Val::Sig(s) => Some(s),
            Val::Var(v) => v.as_str(),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Val::U8(x) => Some(*x as i64),
            Val::I16(x) => Some(*x as i64),
            Val::U16(x) => Some(*x as i64),
            Val::I32(x) => Some(*x as i64),
            Val::U32(x) => Some(*x as i64),
            Val::I64(x) => Some(*x),
            Val::U64(x) => Some(*x as i64),
            Val::Var(v) => v.as_i64(),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Val::Bool(b) => Some(*b),
            Val::Var(v) => v.as_bool(),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::F64(x) => Some(*x),
            Val::Var(v) => v.as_f64(),
            _ => self.as_i64().map(|x| x as f64),
        }
    }
    pub fn as_arr(&self) -> Option<&Vec<Val>> {
        match self {
            Val::Arr(_, v) | Val::Struct(v) => Some(v),
            Val::Var(v) => v.as_arr(),
            _ => None,
        }
    }
    /// For a{sv}/a{ss}: look up a string key.
    pub fn dict_get(&self, key: &str) -> Option<&Val> {
        let items = match self {
            Val::Arr(_, v) => v,
            Val::Var(v) => return v.dict_get(key),
            _ => return None,
        };
        for it in items {
            if let Val::Dict(k, v) = it {
                if k.as_str() == Some(key) {
                    return Some(match &**v {
                        Val::Var(inner) => inner,
                        o => o,
                    });
                }
            }
        }
        None
    }
}

pub fn dict_sv(items: Vec<(&str, Val)>) -> Val {
    Val::Arr("{sv}".into(), items.into_iter().map(|(k, v)| Val::Dict(Box::new(Val::s(k)), Box::new(Val::Var(Box::new(v))))).collect())
}
pub fn arr_s(items: &[&str]) -> Val {
    Val::Arr("s".into(), items.iter().map(|s| Val::s(s)).collect())
}

fn align_of_sig(c: u8) -> usize {
    match c {
        b'y' | b'g' | b'v' => 1,
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b's' | b'o' | b'a' | b'h' => 4,
        _ => 8,
    }
}

struct W {
    b: Vec<u8>,
}
impl W {
    fn align(&mut self, n: usize) {
        while self.b.len() % n != 0 {
            self.b.push(0);
        }
    }
    fn put(&mut self, v: &Val) {
        match v {
            Val::U8(x) => self.b.push(*x),
            Val::Bool(x) => {
                self.align(4);
                self.b.extend_from_slice(&(*x as u32).to_le_bytes());
            }
            Val::I16(x) => {
                self.align(2);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::U16(x) => {
                self.align(2);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::I32(x) => {
                self.align(4);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::U32(x) => {
                self.align(4);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::I64(x) => {
                self.align(8);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::U64(x) => {
                self.align(8);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::F64(x) => {
                self.align(8);
                self.b.extend_from_slice(&x.to_le_bytes());
            }
            Val::Str(s) | Val::Obj(s) => {
                self.align(4);
                self.b.extend_from_slice(&(s.len() as u32).to_le_bytes());
                self.b.extend_from_slice(s.as_bytes());
                self.b.push(0);
            }
            Val::Sig(s) => {
                self.b.push(s.len() as u8);
                self.b.extend_from_slice(s.as_bytes());
                self.b.push(0);
            }
            Val::Arr(es, items) => {
                self.align(4);
                let lenpos = self.b.len();
                self.b.extend_from_slice(&0u32.to_le_bytes());
                self.align(align_of_sig(es.as_bytes().first().copied().unwrap_or(b'y')));
                let start = self.b.len();
                for it in items {
                    self.put(it);
                }
                let n = (self.b.len() - start) as u32;
                self.b[lenpos..lenpos + 4].copy_from_slice(&n.to_le_bytes());
            }
            Val::Struct(items) => {
                self.align(8);
                for it in items {
                    self.put(it);
                }
            }
            Val::Dict(k, v) => {
                self.align(8);
                self.put(k);
                self.put(v);
            }
            Val::Var(inner) => {
                self.put(&Val::Sig(inner.sig()));
                self.put(inner);
            }
        }
    }
}

struct R<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn align(&mut self, n: usize) -> Option<()> {
        while self.p % n != 0 {
            self.p += 1;
        }
        if self.p > self.d.len() {
            None
        } else {
            Some(())
        }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        self.align(4)?;
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn get(&mut self, sig: &[u8], i: &mut usize, depth: u32) -> Option<Val> {
        if depth > 24 {
            return None;
        }
        let c = *sig.get(*i)?;
        *i += 1;
        Some(match c {
            b'y' => Val::U8(self.take(1)?[0]),
            b'b' => Val::Bool(self.u32()? != 0),
            b'n' => {
                self.align(2)?;
                let b = self.take(2)?;
                Val::I16(i16::from_le_bytes([b[0], b[1]]))
            }
            b'q' => {
                self.align(2)?;
                let b = self.take(2)?;
                Val::U16(u16::from_le_bytes([b[0], b[1]]))
            }
            b'i' => Val::I32(self.u32()? as i32),
            b'u' | b'h' => Val::U32(self.u32()?),
            b'x' | b't' | b'd' => {
                self.align(8)?;
                let b = self.take(8)?;
                let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
                match c {
                    b'x' => Val::I64(i64::from_le_bytes(a)),
                    b't' => Val::U64(u64::from_le_bytes(a)),
                    _ => Val::F64(f64::from_le_bytes(a)),
                }
            }
            b's' | b'o' => {
                let n = self.u32()? as usize;
                if n > 1 << 24 {
                    return None;
                }
                let b = self.take(n)?;
                self.take(1)?;
                let s = String::from_utf8_lossy(b).to_string();
                if c == b's' { Val::Str(s) } else { Val::Obj(s) }
            }
            b'g' => {
                let n = self.take(1)?[0] as usize;
                let b = self.take(n)?;
                self.take(1)?;
                Val::Sig(String::from_utf8_lossy(b).to_string())
            }
            b'v' => {
                let n = self.take(1)?[0] as usize;
                let s = self.take(n)?.to_vec();
                self.take(1)?;
                let mut j = 0;
                let v = self.get(&s, &mut j, depth + 1)?;
                Val::Var(Box::new(v))
            }
            b'a' => {
                let n = self.u32()? as usize;
                if n > 1 << 26 {
                    return None;
                }
                let es_start = *i;
                skip_type(sig, i)?;
                let es = &sig[es_start..*i];
                self.align(align_of_sig(*es.first()?))?;
                let end = self.p.checked_add(n)?;
                if end > self.d.len() {
                    return None;
                }
                let mut items = vec![];
                while self.p < end {
                    let mut j = 0;
                    items.push(self.get(es, &mut j, depth + 1)?);
                    if items.len() > 200_000 {
                        return None;
                    }
                }
                if self.p != end {
                    return None;
                }
                Val::Arr(String::from_utf8_lossy(es).to_string(), items)
            }
            b'(' => {
                self.align(8)?;
                let mut items = vec![];
                while *sig.get(*i)? != b')' {
                    items.push(self.get(sig, i, depth + 1)?);
                }
                *i += 1;
                Val::Struct(items)
            }
            b'{' => {
                self.align(8)?;
                let k = self.get(sig, i, depth + 1)?;
                let v = self.get(sig, i, depth + 1)?;
                if *sig.get(*i)? != b'}' {
                    return None;
                }
                *i += 1;
                Val::Dict(Box::new(k), Box::new(v))
            }
            _ => return None,
        })
    }
}

fn skip_type(sig: &[u8], i: &mut usize) -> Option<()> {
    let c = *sig.get(*i)?;
    *i += 1;
    match c {
        b'a' => skip_type(sig, i),
        b'(' => {
            while *sig.get(*i)? != b')' {
                skip_type(sig, i)?;
            }
            *i += 1;
            Some(())
        }
        b'{' => {
            while *sig.get(*i)? != b'}' {
                skip_type(sig, i)?;
            }
            *i += 1;
            Some(())
        }
        _ => Some(()),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Msg {
    pub mtype: u8,
    pub flags: u8,
    pub serial: u32,
    pub reply_serial: u32,
    pub path: String,
    pub iface: String,
    pub member: String,
    pub error: String,
    pub dest: String,
    pub sender: String,
    pub sig: String,
    pub body: Vec<Val>,
}

pub fn encode(m: &Msg) -> Vec<u8> {
    let mut body = W { b: vec![] };
    for v in &m.body {
        body.put(v);
    }
    let mut w = W { b: vec![b'l', m.mtype, m.flags, 1] };
    w.b.extend_from_slice(&(body.b.len() as u32).to_le_bytes());
    w.b.extend_from_slice(&m.serial.to_le_bytes());
    let lenpos = w.b.len();
    w.b.extend_from_slice(&0u32.to_le_bytes());
    let start = w.b.len();
    let mut field = |code: u8, v: Val| {
        w.align(8);
        w.b.push(code);
        w.put(&Val::Sig(v.sig()));
        w.put(&v);
    };
    if !m.path.is_empty() {
        field(1, Val::Obj(m.path.clone()));
    }
    if !m.iface.is_empty() {
        field(2, Val::Str(m.iface.clone()));
    }
    if !m.member.is_empty() {
        field(3, Val::Str(m.member.clone()));
    }
    if !m.error.is_empty() {
        field(4, Val::Str(m.error.clone()));
    }
    if m.reply_serial != 0 {
        field(5, Val::U32(m.reply_serial));
    }
    if !m.dest.is_empty() {
        field(6, Val::Str(m.dest.clone()));
    }
    let sig: String = m.body.iter().map(|v| v.sig()).collect();
    if !sig.is_empty() {
        field(8, Val::Sig(sig));
    }
    let n = (w.b.len() - start) as u32;
    w.b[lenpos..lenpos + 4].copy_from_slice(&n.to_le_bytes());
    w.align(8);
    w.b.extend_from_slice(&body.b);
    w.b
}

/// Parse one message from `d` (must be a complete message). None if malformed.
pub fn decode(d: &[u8]) -> Option<Msg> {
    if d.len() < 16 || d[0] != b'l' || d[3] != 1 {
        return None;
    }
    let blen = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize;
    let mut m = Msg { mtype: d[1], flags: d[2], serial: u32::from_le_bytes([d[8], d[9], d[10], d[11]]), ..Default::default() };
    let flen = u32::from_le_bytes([d[12], d[13], d[14], d[15]]) as usize;
    let mut r = R { d, p: 16 };
    let fend = 16usize.checked_add(flen)?;
    if fend > d.len() {
        return None;
    }
    while r.p < fend {
        r.align(8)?;
        if r.p >= fend {
            break;
        }
        let code = r.take(1)?[0];
        let n = r.take(1)?[0] as usize;
        let s = r.take(n)?.to_vec();
        r.take(1)?;
        let mut j = 0;
        let v = r.get(&s, &mut j, 0)?;
        match code {
            1 => m.path = v.as_str()?.to_string(),
            2 => m.iface = v.as_str()?.to_string(),
            3 => m.member = v.as_str()?.to_string(),
            4 => m.error = v.as_str()?.to_string(),
            5 => m.reply_serial = v.as_i64()? as u32,
            6 => m.dest = v.as_str()?.to_string(),
            7 => m.sender = v.as_str()?.to_string(),
            8 => m.sig = v.as_str()?.to_string(),
            _ => {}
        }
    }
    let bstart = (fend + 7) & !7;
    if bstart.checked_add(blen)? > d.len() {
        return None;
    }
    let mut br = R { d: &d[..bstart + blen], p: bstart };
    let sb = m.sig.clone().into_bytes();
    let mut i = 0;
    while i < sb.len() {
        m.body.push(br.get(&sb, &mut i, 0)?);
    }
    Some(m)
}

pub enum Ev {
    Signal(Msg),
    Call(Msg),
    /// Reply (or error) to a tagged call we made.
    Reply { tag: String, msg: Msg },
}

pub struct Bus {
    sock: UnixStream,
    inbuf: Vec<u8>,
    outbuf: Vec<u8>,
    serial: u32,
    pending: HashMap<u32, String>,
    pub unique: String,
}

fn session_socket() -> Result<UnixStream, String> {
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().filter(|s| !s.is_empty());
    let mut cands: Vec<String> = vec![];
    if let Some(a) = addr {
        for part in a.split(';') {
            cands.push(part.to_string());
        }
    }
    cands.push(format!("unix:path=/run/user/{}/bus", unsafe { crate::sys::getuid() }));
    let mut last = "no session bus address".to_string();
    for c in cands {
        let Some(rest) = c.strip_prefix("unix:") else { continue };
        for kv in rest.split(',') {
            if let Some(p) = kv.strip_prefix("path=") {
                match UnixStream::connect(p) {
                    Ok(s) => return Ok(s),
                    Err(e) => last = format!("{}: {}", p, e),
                }
            } else if let Some(a) = kv.strip_prefix("abstract=") {
                use std::os::linux::net::SocketAddrExt;
                if let Ok(sa) = std::os::unix::net::SocketAddr::from_abstract_name(a.as_bytes()) {
                    match UnixStream::connect_addr(&sa) {
                        Ok(s) => return Ok(s),
                        Err(e) => last = format!("abstract {}: {}", a, e),
                    }
                }
            }
        }
    }
    Err(last)
}

impl Bus {
    pub fn connect() -> Result<Bus, String> {
        Bus::from_stream(session_socket()?)
    }

    pub fn from_stream(mut s: UnixStream) -> Result<Bus, String> {
        s.set_read_timeout(Some(Duration::from_secs(3))).ok();
        s.set_write_timeout(Some(Duration::from_secs(3))).ok();
        let uid = unsafe { crate::sys::getuid() }.to_string();
        let hex: String = uid.bytes().map(|b| format!("{:02x}", b)).collect();
        s.write_all(format!("\0AUTH EXTERNAL {}\r\n", hex).as_bytes()).map_err(|e| e.to_string())?;
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            s.read_exact(&mut b).map_err(|e| format!("auth: {}", e))?;
            line.push(b[0]);
            if line.len() > 512 {
                return Err("auth reply too long".into());
            }
        }
        if !line.starts_with(b"OK") {
            return Err(format!("auth rejected: {}", String::from_utf8_lossy(&line).trim()));
        }
        s.write_all(b"BEGIN\r\n").map_err(|e| e.to_string())?;
        let mut bus = Bus { sock: s, inbuf: vec![], outbuf: vec![], serial: 0, pending: HashMap::new(), unique: String::new() };
        let ser = bus.send_call("hello", "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", vec![]);
        bus.flush();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            match bus.read_some(true) {
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            while let Some(m) = bus.next_msg() {
                if m.mtype == 2 && m.reply_serial == ser {
                    bus.unique = m.body.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
                    bus.pending.remove(&ser);
                    bus.sock.set_nonblocking(true).ok();
                    return Ok(bus);
                }
            }
        }
        Err("Hello timed out".into())
    }

    pub fn fd(&self) -> i32 {
        self.sock.as_raw_fd()
    }

    fn read_some(&mut self, block: bool) -> Result<usize, String> {
        let mut tmp = [0u8; 65536];
        if block {
            self.sock.set_nonblocking(false).ok();
        }
        let r = self.sock.read(&mut tmp);
        if block {
            self.sock.set_nonblocking(true).ok();
        }
        match r {
            Ok(0) => Err("bus closed".into()),
            Ok(n) => {
                self.inbuf.extend_from_slice(&tmp[..n]);
                Ok(n)
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e.to_string()),
        }
    }

    fn next_msg(&mut self) -> Option<Msg> {
        loop {
            if self.inbuf.len() < 16 {
                return None;
            }
            let b = &self.inbuf;
            if b[0] != b'l' {
                // unsupported endianness / corrupt stream: drop everything
                self.inbuf.clear();
                return None;
            }
            let blen = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
            let flen = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
            if blen > 1 << 24 || flen > 1 << 20 {
                self.inbuf.clear();
                return None;
            }
            let total = ((16 + flen + 7) & !7) + blen;
            if self.inbuf.len() < total {
                return None;
            }
            let rest = self.inbuf.split_off(total);
            let raw = std::mem::replace(&mut self.inbuf, rest);
            if let Some(m) = decode(&raw) {
                return Some(m);
            }
            // malformed message: skip it, keep stream in sync
        }
    }

    pub fn flush(&mut self) {
        if self.outbuf.is_empty() {
            return;
        }
        self.sock.set_nonblocking(false).ok();
        let _ = self.sock.write_all(&self.outbuf);
        self.sock.set_nonblocking(true).ok();
        self.outbuf.clear();
    }

    fn push(&mut self, m: &Msg) {
        self.outbuf.extend_from_slice(&encode(m));
        if self.outbuf.len() > 1 << 20 {
            self.flush();
        }
    }

    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }

    pub fn send_call(&mut self, tag: &str, dest: &str, path: &str, iface: &str, member: &str, args: Vec<Val>) -> u32 {
        let serial = self.next_serial();
        let m = Msg { mtype: 1, serial, path: path.into(), iface: iface.into(), member: member.into(), dest: dest.into(), body: args, ..Default::default() };
        self.push(&m);
        if !tag.is_empty() {
            self.pending.insert(serial, tag.to_string());
        } else {
            // fire and forget: still get a reply; ignore it
            self.pending.insert(serial, String::new());
        }
        serial
    }

    pub fn reply(&mut self, call: &Msg, args: Vec<Val>) {
        let serial = self.next_serial();
        let m = Msg { mtype: 2, flags: 1, serial, reply_serial: call.serial, dest: call.sender.clone(), body: args, ..Default::default() };
        self.push(&m);
    }
    pub fn error(&mut self, call: &Msg, name: &str, text: &str) {
        let serial = self.next_serial();
        let m = Msg { mtype: 3, flags: 1, serial, reply_serial: call.serial, dest: call.sender.clone(), error: name.into(), body: vec![Val::s(text)], ..Default::default() };
        self.push(&m);
    }
    pub fn signal(&mut self, path: &str, iface: &str, member: &str, args: Vec<Val>) {
        let serial = self.next_serial();
        let m = Msg { mtype: 4, flags: 1, serial, path: path.into(), iface: iface.into(), member: member.into(), body: args, ..Default::default() };
        self.push(&m);
    }

    pub fn request_name(&mut self, name: &str) {
        // flags: DO_NOT_QUEUE (4)
        self.send_call(&format!("reqname:{}", name), "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", vec![Val::s(name), Val::U32(4)]);
    }
    pub fn add_match(&mut self, rule: &str) {
        self.send_call("", "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "AddMatch", vec![Val::s(rule)]);
    }

    /// Read & decode everything available.
    pub fn poll(&mut self) -> Result<Vec<Ev>, String> {
        loop {
            match self.read_some(false)? {
                0 => break,
                _ => continue,
            }
        }
        let mut out = vec![];
        while let Some(m) = self.next_msg() {
            match m.mtype {
                1 => out.push(Ev::Call(m)),
                2 | 3 => {
                    if let Some(tag) = self.pending.remove(&m.reply_serial) {
                        if !tag.is_empty() {
                            out.push(Ev::Reply { tag, msg: m });
                        }
                    }
                }
                4 => out.push(Ev::Signal(m)),
                _ => {}
            }
        }
        self.flush();
        Ok(out)
    }
}
