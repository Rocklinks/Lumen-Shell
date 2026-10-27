//! Background command runner (threads + wake pipe) so slow CLI backends never block the UI.
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

pub struct JobResult {
    pub tag: String,
    pub ok: bool,
    pub out: String,
    pub err: String,
}

pub struct Jobs {
    rx: Receiver<JobResult>,
    tx: Sender<JobResult>,
    pub wake_r: i32,
    wake_w: i32,
    inflight: Vec<String>,
}

pub fn run_cmd(argv: &[String], stdin: Option<&str>, timeout_ms: u64) -> (bool, String, String) {
    if argv.is_empty() {
        return (false, String::new(), String::new());
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.env("LC_ALL", "C");
    let mut ch = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, String::new(), e.to_string()),
    };
    if let (Some(s), Some(mut si)) = (stdin, ch.stdin.take()) {
        use std::io::Write;
        let _ = si.write_all(s.as_bytes());
    }
    let mut out = ch.stdout.take();
    let mut errp = ch.stderr.take();
    let err_reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(e) = errp.as_mut() {
            let _ = e.take(8192).read_to_end(&mut b);
        }
        String::from_utf8_lossy(&b).trim().to_string()
    });
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let mut b = Vec::new();
            let _ = o.take(1 << 20).read_to_end(&mut b);
            s = String::from_utf8_lossy(&b).to_string();
        }
        s
    });
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let ok = loop {
        match ch.try_wait() {
            Ok(Some(st)) => break st.success(),
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = ch.kill();
                    let _ = ch.wait();
                    break false;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => break false,
        }
    };
    (ok, reader.join().unwrap_or_default(), err_reader.join().unwrap_or_default())
}

impl Jobs {
    pub fn new() -> Jobs {
        let (tx, rx) = channel();
        let (r, w) = crate::sys::make_pipe().unwrap_or((-1, -1));
        Jobs { rx, tx, wake_r: r, wake_w: w, inflight: vec![] }
    }
    /// Run `argv` in the background; result delivered via `poll()` tagged `tag`. Same tag in flight is skipped.
    pub fn run(&mut self, tag: &str, argv: &[&str], stdin: Option<String>, timeout_ms: u64) {
        if self.inflight.iter().any(|t| t == tag) || self.inflight.len() > 64 {
            return;
        }
        self.inflight.push(tag.to_string());
        let tx = self.tx.clone();
        let w = self.wake_w;
        let tag = tag.to_string();
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        std::thread::spawn(move || {
            let (ok, out, err) = run_cmd(&argv, stdin.as_deref(), timeout_ms);
            let _ = tx.send(JobResult { tag, ok, out, err });
            crate::sys::wake(w);
        });
    }
    pub fn poll(&mut self) -> Vec<JobResult> {
        if self.wake_r >= 0 {
            crate::sys::drain(self.wake_r);
        }
        let mut v = vec![];
        while let Ok(r) = self.rx.try_recv() {
            self.inflight.retain(|t| *t != r.tag);
            v.push(r);
        }
        v
    }
}
