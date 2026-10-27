//! Single-instance guard + tiny command channel (`lumen --settings`, `--toggle-grid`, ...) over a unix socket.
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

pub fn sock_path() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d).join("lumen.sock"),
        _ => PathBuf::from(format!("/tmp/lumen-{}.sock", unsafe { crate::sys::getuid() })),
    }
}

/// Send a command to the running shell. Returns false if none is running.
pub fn send(cmd: &str) -> bool {
    match UnixStream::connect(sock_path()) {
        Ok(mut s) => {
            let _ = s.set_write_timeout(Some(Duration::from_millis(500)));
            let r = s.write_all(format!("{}\n", cmd).as_bytes());
            if let Err(e) = &r {
                if std::env::var("LUMEN_DEBUG").is_ok() {
                    eprintln!("lumen: ipc write error: {}", e);
                }
            }
            r.is_ok()
        }
        Err(e) => {
            if std::env::var("LUMEN_DEBUG").is_ok() {
                eprintln!("lumen: ipc connect {:?}: {}", sock_path(), e);
            }
            false
        }
    }
}

pub struct Ipc {
    pub listener: UnixListener,
    path: PathBuf,
}

impl Ipc {
    /// Err(()) = another instance already owns the socket.
    pub fn bind() -> Result<Ipc, ()> {
        let path = sock_path();
        if UnixStream::connect(&path).is_ok() {
            return Err(());
        }
        let _ = std::fs::remove_file(&path); // stale
        let l = UnixListener::bind(&path).map_err(|_| ())?;
        l.set_nonblocking(true).map_err(|_| ())?;
        Ok(Ipc { listener: l, path })
    }
    pub fn fd(&self) -> i32 {
        use std::os::unix::io::AsRawFd;
        self.listener.as_raw_fd()
    }
    pub fn poll(&self) -> Vec<String> {
        let mut out = vec![];
        for _ in 0..8 {
            match self.listener.accept() {
                Ok((mut s, _)) => {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(80)));
                    let mut buf = [0u8; 128];
                    if let Ok(n) = s.read(&mut buf) {
                        let t = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                        if !t.is_empty() {
                            out.push(t);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        out
    }
}

impl Drop for Ipc {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
