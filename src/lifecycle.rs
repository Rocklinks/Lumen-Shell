//! Session lifecycle: replace xfce4-panel while running, always give it back when we stop,
//! honor kill/Terminate like a normal app, and manage the autostart entry.
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn have_panel() -> bool {
    crate::desktop::which("xfce4-panel")
}

/// Politely stop xfce4-panel (if it is running). Returns true if we stopped one.
pub fn stop_panel() -> bool {
    if !have_panel() || !crate::sys::proc_running("xfce4-panel") {
        return false;
    }
    let _ = Command::new("xfce4-panel").arg("--quit").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let t = Instant::now();
    while crate::sys::proc_running("xfce4-panel") && t.elapsed() < Duration::from_millis(2500) {
        std::thread::sleep(Duration::from_millis(60));
    }
    if crate::sys::proc_running("xfce4-panel") {
        let _ = Command::new("pkill").args(["-x", "xfce4-panel"]).status();
        std::thread::sleep(Duration::from_millis(200));
    }
    true
}

/// Start xfce4-panel again (detached) unless one is already running.
pub fn restore_panel() {
    if !have_panel() || crate::sys::proc_running("xfce4-panel") {
        return;
    }
    let _ = Command::new("xfce4-panel").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn();
}

pub fn autostart_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()).map(std::path::PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".config"))).unwrap_or_else(|| "/tmp".into());
    base.join("autostart/lumen-shell.desktop")
}

pub fn autostart_enabled() -> bool {
    autostart_path().exists()
}

pub fn set_autostart(on: bool) -> bool {
    let p = autostart_path();
    if !on {
        return std::fs::remove_file(&p).is_ok() || !p.exists();
    }
    let exe = std::env::current_exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|_| "lumen".into());
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new("/tmp")));
    let txt = format!("[Desktop Entry]\nType=Application\nName=lumen-shell\nComment=GNOME-style top bar, dock and app grid\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\nX-XFCE-Autostart-Override=true\nNoDisplay=true\n", exe);
    std::fs::write(&p, txt).is_ok()
}
