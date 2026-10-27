//! Minimal libc FFI. The only `unsafe` OS surface in the project lives here.
use std::os::raw::{c_char, c_int, c_void};

#[repr(C)]
pub struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}
pub const POLLIN: i16 = 1;
pub const POLLERR: i16 = 8;
pub const POLLHUP: i16 = 16;

#[repr(C)]
pub struct IoVec {
    pub base: *mut c_void,
    pub len: usize,
}
#[repr(C)]
pub struct MsgHdr {
    pub name: *mut c_void,
    pub namelen: u32,
    pub iov: *mut IoVec,
    pub iovlen: usize,
    pub control: *mut c_void,
    pub controllen: usize,
    pub flags: c_int,
}
#[repr(C)]
pub struct CmsgHdr {
    pub len: usize,
    pub level: c_int,
    pub typ: c_int,
}

extern "C" {
    fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    fn ftruncate(fd: c_int, len: i64) -> c_int;
    fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn poll(fds: *mut PollFd, n: u64, timeout: c_int) -> c_int;
    fn sendmsg(fd: c_int, msg: *const MsgHdr, flags: c_int) -> isize;
    fn recvmsg(fd: c_int, msg: *mut MsgHdr, flags: c_int) -> isize;
    fn close(fd: c_int) -> c_int;
    pub fn getuid() -> u32;
    fn kill(pid: c_int, sig: c_int) -> c_int;
}

pub fn sys_kill(pid: i32, sig: i32) {
    unsafe {
        kill(pid, sig);
    }
}

pub fn sys_close(fd: i32) {
    unsafe {
        close(fd);
    }
}

/// Wait for any of `fds` readable, up to `timeout_ms` (-1 = forever). Returns readiness per fd.
pub fn poll_fds(fds: &[i32], timeout_ms: i32) -> Vec<bool> {
    let mut v: Vec<PollFd> = fds.iter().map(|&fd| PollFd { fd, events: POLLIN, revents: 0 }).collect();
    let r = unsafe { poll(v.as_mut_ptr(), v.len() as u64, timeout_ms) };
    if r <= 0 {
        return vec![false; fds.len()];
    }
    v.iter().map(|p| p.revents & (POLLIN | POLLERR | POLLHUP) != 0).collect()
}

/// Shared-memory region backed by memfd, mapped read/write.
pub struct Shm {
    pub fd: i32,
    ptr: *mut u8,
    pub len: usize,
}

impl Shm {
    pub fn new(len: usize) -> Option<Shm> {
        unsafe {
            let name = b"lumen\0";
            let fd = memfd_create(name.as_ptr() as *const c_char, 1); // MFD_CLOEXEC
            if fd < 0 {
                return None;
            }
            if ftruncate(fd, len as i64) != 0 {
                close(fd);
                return None;
            }
            let p = mmap(std::ptr::null_mut(), len, 3, 1, fd, 0); // PROT_READ|WRITE, MAP_SHARED
            if p as isize == -1 {
                close(fd);
                return None;
            }
            Some(Shm { fd, ptr: p as *mut u8, len })
        }
    }
    pub fn as_u32_mut(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut u32, self.len / 4) }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr as *mut c_void, self.len);
            close(self.fd);
        }
    }
}

/// Send bytes plus optional fds over a unix socket.
pub fn send_with_fds(sock: i32, data: &[u8], fds: &[i32]) -> isize {
    unsafe {
        let mut iov = IoVec { base: data.as_ptr() as *mut c_void, len: data.len() };
        let mut cbuf = [0u64; 8];
        let mut hdr = MsgHdr {
            name: std::ptr::null_mut(),
            namelen: 0,
            iov: &mut iov,
            iovlen: 1,
            control: std::ptr::null_mut(),
            controllen: 0,
            flags: 0,
        };
        if !fds.is_empty() && fds.len() <= 4 {
            let datalen = fds.len() * 4;
            let cmsg = cbuf.as_mut_ptr() as *mut CmsgHdr;
            (*cmsg).len = std::mem::size_of::<CmsgHdr>() + datalen;
            (*cmsg).level = 1; // SOL_SOCKET
            (*cmsg).typ = 1; // SCM_RIGHTS
            let dp = (cmsg as *mut u8).add(std::mem::size_of::<CmsgHdr>()) as *mut i32;
            for (i, f) in fds.iter().enumerate() {
                *dp.add(i) = *f;
            }
            hdr.control = cbuf.as_mut_ptr() as *mut c_void;
            hdr.controllen = (std::mem::size_of::<CmsgHdr>() + datalen + 7) & !7;
        }
        sendmsg(sock, &hdr, 0x4000) // MSG_NOSIGNAL
    }
}

/// Receive bytes and any passed fds (non-blocking). Returns (n, fds); n<0 on error, 0 on EOF.
pub fn recv_with_fds(sock: i32, buf: &mut [u8]) -> (isize, Vec<i32>) {
    unsafe {
        let mut iov = IoVec { base: buf.as_mut_ptr() as *mut c_void, len: buf.len() };
        let mut cbuf = [0u64; 16];
        let mut hdr = MsgHdr {
            name: std::ptr::null_mut(),
            namelen: 0,
            iov: &mut iov,
            iovlen: 1,
            control: cbuf.as_mut_ptr() as *mut c_void,
            controllen: std::mem::size_of_val(&cbuf),
            flags: 0,
        };
        let n = recvmsg(sock, &mut hdr, 0x40 | 0x40000000); // MSG_DONTWAIT | MSG_CMSG_CLOEXEC
        let mut fds = Vec::new();
        if n >= 0 && hdr.controllen >= std::mem::size_of::<CmsgHdr>() {
            let cmsg = cbuf.as_ptr() as *const CmsgHdr;
            if (*cmsg).level == 1 && (*cmsg).typ == 1 {
                let dlen = (*cmsg).len.saturating_sub(std::mem::size_of::<CmsgHdr>());
                let dp = (cmsg as *const u8).add(std::mem::size_of::<CmsgHdr>()) as *const i32;
                for i in 0..dlen / 4 {
                    fds.push(*dp.add(i));
                }
            }
        }
        (n, fds)
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Tm {
    pub sec: i32,
    pub min: i32,
    pub hour: i32,
    pub mday: i32,
    pub mon: i32,
    pub year: i32,
    pub wday: i32,
    pub yday: i32,
    pub isdst: i32,
    pub gmtoff: i64,
    pub zone: usize,
}
extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}
pub fn local_time(epoch: i64) -> Tm {
    let mut tm = Tm::default();
    unsafe {
        localtime_r(&epoch, &mut tm);
    }
    tm
}
pub fn now_epoch() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

extern "C" {
    fn pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
}
pub fn make_pipe() -> Option<(i32, i32)> {
    let mut f = [0 as c_int; 2];
    if unsafe { pipe2(f.as_mut_ptr(), 2048 | 524288) } == 0 {
        Some((f[0], f[1]))
    } else {
        None
    }
}
pub fn wake(fd: i32) {
    let b = [1u8];
    unsafe {
        write(fd, b.as_ptr() as *const c_void, 1);
    }
}
pub fn drain(fd: i32) {
    let mut b = [0u8; 64];
    unsafe {
        while read(fd, b.as_mut_ptr() as *mut c_void, 64) > 0 {}
    }
}

#[repr(C)]
struct StatVfs {
    bsize: u64,
    frsize: u64,
    blocks: u64,
    bfree: u64,
    bavail: u64,
    files: u64,
    ffree: u64,
    favail: u64,
    fsid: u64,
    flag: u64,
    namemax: u64,
    spare: [i32; 6],
}
extern "C" {
    fn statvfs(path: *const c_char, buf: *mut StatVfs) -> c_int;
}
/// (total bytes, free bytes) of the filesystem at `path`.
pub fn disk_space(path: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut b = std::mem::MaybeUninit::<StatVfs>::zeroed();
    unsafe {
        if statvfs(c.as_ptr(), b.as_mut_ptr()) != 0 {
            return None;
        }
        let b = b.assume_init();
        Some((b.blocks * b.frsize, b.bavail * b.frsize))
    }
}

use std::sync::atomic::{AtomicI32, Ordering};
static SIG_PIPE_W: AtomicI32 = AtomicI32::new(-1);
extern "C" {
    fn signal(signum: c_int, handler: usize) -> usize;
    fn prctl(option: c_int, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> c_int;
    pub fn getppid() -> i32;
    pub fn getpid() -> i32;
}
extern "C" fn on_signal(sig: c_int) {
    let fd = SIG_PIPE_W.load(Ordering::Relaxed);
    if fd >= 0 {
        let b = [sig as u8];
        unsafe {
            write(fd, b.as_ptr() as *const c_void, 1);
        }
    }
}
/// Route SIGTERM/SIGINT/SIGHUP/SIGQUIT into a pipe the main loop polls. Returns the read fd.
pub fn install_term_handlers() -> i32 {
    match make_pipe() {
        Some((r, w)) => {
            SIG_PIPE_W.store(w, Ordering::Relaxed);
            for s in [1, 2, 3, 15] {
                unsafe {
                    signal(s, on_signal as usize);
                }
            }
            r
        }
        None => -1,
    }
}
/// Ask the kernel to SIGTERM us if our parent (the supervisor) dies.
pub fn die_with_parent() {
    unsafe {
        prctl(1, 15, 0, 0, 0); // PR_SET_PDEATHSIG, SIGTERM
    }
}
/// Names of running processes (from /proc/*/comm).
pub fn proc_running(name: &str) -> bool {
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(c) = std::fs::read_to_string(e.path().join("comm")) {
                    if c.trim() == name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn getpid_str() -> String {
    unsafe { getpid() }.to_string()
}
