#![allow(dead_code)]
mod config;
mod dbus;
mod desktop;
mod gfx;
mod icons;
mod ipc;
mod jobs;
mod lifecycle;
mod platform;
mod pngenc;
mod png;
mod svg;
mod svc;
mod sys;
mod sysinfo;
mod theme;
mod ttf;
mod ui;
mod wl;
mod x11;
#[cfg(test)]
mod tests;

use platform::Platform;

fn make_platform() -> Result<Box<dyn Platform>, String> {
    let force = std::env::var("LUMEN_BACKEND").unwrap_or_default();
    if std::env::var("WAYLAND_DISPLAY").is_ok() && force != "x11" {
        match wl::Wayland::connect() {
            Ok(w) => return Ok(Box::new(w)),
            Err(e) => {
                eprintln!("lumen: wayland unavailable: {}", e);
                if std::env::var("DISPLAY").is_err() {
                    return Err(e);
                }
            }
        }
    }
    if std::env::var("DISPLAY").is_ok() {
        return x11::X11::connect().map(|x| Box::new(x) as Box<dyn Platform>);
    }
    Err("No display found (neither WAYLAND_DISPLAY nor DISPLAY set)".into())
}

fn run() -> i32 {
    let term_fd = sys::install_term_handlers();
    sys::die_with_parent();
    let sup_pid: i32 = std::env::var("LUMEN_SUPERVISOR_PID").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let p = match make_platform() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lumen: {}", e);
            return 2;
        }
    };
    let ipc = match ipc::Ipc::bind() {
        Ok(i) => i,
        Err(()) => {
            eprintln!("lumen: already running");
            return 3;
        }
    };
    let mut sh = ui::Shell::new(p);
    sh.qs.refresh_all(&mut sh.jobs);
    sh.apply_night();
    sh.init_services();
    sh.register_shortcuts();
    let mut last_vol = std::time::Instant::now();
    loop {
        let fds = [sh.p.fd(), sh.jobs.wake_r, ipc.fd(), sh.svc_fd(), term_fd];
        let mut to = sh.next_timeout_ms();
        if sh.p.repeat_pending() {
            to = to.min(15);
        }
        let ready = sys::poll_fds(&fds, to);
        if ready.get(4).copied().unwrap_or(false) {
            sys::drain(term_fd);
            sh.quit = true;
        }
        for c in ipc.poll() {
            sh.ipc_cmd(&c);
        }
        let evs = sh.p.dispatch();
        for e in evs {
            sh.handle(e);
        }
        sh.svc_poll();
        sh.tick();
        if last_vol.elapsed().as_secs() >= 5 {
            last_vol = std::time::Instant::now();
            sh.qs.refresh_vol(&mut sh.jobs);
        }
        sh.render();
        if sh.quit {
            sh.shutdown();
            // supervisor died (or no supervisor): hand the desktop panel back ourselves
            if sup_pid != 0 && unsafe { sys::getppid() } != sup_pid {
                lifecycle::restore_panel();
            }
            return 0;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("lumen-shell {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "/tmp/shot.png".into());
        let mut p = match make_platform() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("lumen: {}", e);
                std::process::exit(2);
            }
        };
        let o = p.outputs().first().cloned();
        let (w, h) = o.map_or((1280, 720), |o| (o.w, o.h));
        match p.capture(0, 0, w, h) {
            Some(cv) => {
                let png = pngenc::encode(&cv, 0, 0, cv.w, cv.h);
                let _ = std::fs::write(&out, &png);
                println!("captured {}x{} -> {} ({} bytes)", cv.w, cv.h, out, png.len());
            }
            None => {
                eprintln!("lumen: capture failed or unsupported by this session");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--icon-audit") {
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "/tmp/icon_audit.png".into());
        let raw = theme::read_raw();
        let th = theme::resolve(config::ThemeMode::Auto, &raw, 1.0, None);
        let mut icons = icons::Icons::new(&raw.icon_theme);
        let mut tx = gfx::TextCtx::new(vec![]);
        let mut txb = gfx::TextCtx::new(vec![]);
        let names = ["audio-volume-muted-symbolic", "audio-volume-low-symbolic", "audio-volume-medium-symbolic", "audio-volume-high-symbolic", "audio-volume-overamplified-symbolic", "network-wireless-signal-excellent-symbolic", "network-wired-symbolic", "bluetooth-active-symbolic", "battery-level-80-symbolic", "battery-level-50-charging-symbolic", "night-light-symbolic", "microphone-sensitivity-high-symbolic", "notifications-disabled-symbolic"];
        let mut cv = gfx::Canvas::new(names.len() * 40 + 10, 3 * 40 + 10);
        cv.fill_rect(0, 0, cv.w as i32, cv.h as i32, 0xFFF2F2F2);
        for (row, size) in [16.0f32, 20.0, 24.0].iter().enumerate() {
            let mut pt = ui::draw::Paint { tx: &mut tx, txb: &mut txb, icons: &mut icons, th: &th, s: 1.0 };
            for (i, n) in names.iter().enumerate() {
                let x = 10.0 + i as f32 * 40.0;
                let y = 8.0 + row as f32 * 40.0;
                cv.stroke_rrect(x - 2.0, y - 2.0, size + 4.0, size + 4.0, 2.0, 1.0, 0x30FF0000);
                pt.icon(&mut cv, n, x, y, *size, 0xFF222222);
            }
        }
        let _ = gfx::write_png(&out, &cv, 0xFFF2F2F2);
        println!("wrote {}", out);
        return;
    }
    if args.iter().any(|a| a == "--display-list" || a == "--display-set") {
        let mut p = match make_platform() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("lumen: {}", e);
                std::process::exit(2);
            }
        };
        match p.display_outputs() {
            Some(outs) => {
                if let Some(i) = args.iter().position(|a| a == "--display-set") {
                    let (name, mode) = (args.get(i + 1).cloned().unwrap_or_default(), args.get(i + 2).and_then(|m| m.parse::<u32>().ok()).unwrap_or(0));
                    let rot = args.get(i + 3).and_then(|m| m.parse::<u16>().ok()).unwrap_or(1);
                    println!("set {} mode {} rot {}: {}", name, mode, rot, p.display_set_mode(&name, mode, rot));
                } else {
                    for o in outs {
                        println!("{} connected={} rot={} at {},{}", o.name, o.connected, o.rotation, o.x, o.y);
                        for m in o.modes {
                            println!("  mode {} {}x{} @{:.2}{}{}", m.id, m.w, m.h, m.refresh, if m.current { " *" } else { "" }, if m.preferred { " +" } else { "" });
                        }
                    }
                }
            }
            None => println!("display configuration not supported by this backend"),
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--settings-page") {
        let n = args.get(i + 1).cloned().unwrap_or_else(|| "0".into());
        if ipc::send(&format!("settings:{}", n)) {
            return;
        }
        eprintln!("lumen: no running shell");
        std::process::exit(1);
    }
    for (flag, cmd) in [("--settings", "settings"), ("--toggle-grid", "grid"), ("--calendar", "calendar"), ("--quick", "quick"), ("--screenshot", "screenshot"), ("--shot-screen", "shot-screen"), ("--shot-area", "shot-area"), ("--record", "record"), ("--reload", "reload"), ("--quit", "quit")] {
        if args.iter().any(|a| a == flag) {
            if ipc::send(cmd) {
                return;
            }
            eprintln!("lumen: no running shell to receive '{}' (start `lumen` first)", cmd);
            std::process::exit(1);
        }
    }
    if !args.iter().any(|a| a == "--supervised") && ipc::send("ping") {
        eprintln!("lumen: already running");
        std::process::exit(3);
    }
    if args.iter().any(|a| a == "--supervised") || std::env::var("LUMEN_NO_SUPERVISOR").is_ok() {
        std::process::exit(run());
    }
    supervise(&args);
}

/// Supervisor: replaces xfce4-panel while running, respawns the shell only after real crashes,
/// and behaves like a normal app on kill/terminate (stops, and gives xfce4-panel back).
fn supervise(args: &[String]) {
    let cfg = config::Config::load();
    let manage_panel = cfg.manage_panel && std::env::var("LUMEN_KEEP_PANEL").is_err();
    let term_fd = sys::install_term_handlers();
    if manage_panel {
        lifecycle::stop_panel();
    }
    let finish = |manage: bool| {
        if manage {
            lifecycle::restore_panel();
        }
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| "lumen".into());
    let mut fails = 0;
    let mut window = std::time::Instant::now();
    loop {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--supervised").env("LUMEN_SUPERVISOR_PID", sys::getpid_str());
        for a in args.iter().skip(1) {
            cmd.arg(a);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("lumen: cannot start shell: {}", e);
                finish(manage_panel);
                return;
            }
        };
        // wait for the child, but react to our own SIGTERM/SIGINT/SIGHUP
        let status = loop {
            if sys::poll_fds(&[term_fd], 120).first().copied().unwrap_or(false) {
                sys::drain(term_fd);
                sys::sys_kill(child.id() as i32, 15);
                let t = std::time::Instant::now();
                loop {
                    if let Ok(Some(_)) = child.try_wait() {
                        break;
                    }
                    if t.elapsed() > std::time::Duration::from_millis(2500) {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
                finish(manage_panel);
                return;
            }
            match child.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) => {}
                Err(_) => {
                    finish(manage_panel);
                    return;
                }
            }
        };
        use std::os::unix::process::ExitStatusExt;
        match (status.code(), status.signal()) {
            (Some(0), _) | (Some(2), _) | (Some(3), _) => {
                // normal quit (lumen --quit, SIGTERM handled gracefully, or "already running")
                finish(manage_panel);
                return;
            }
            (None, Some(sig)) if sig == 9 || sig == 15 || sig == 2 || sig == 1 => {
                // the user killed it (Task manager "End task", kill -9, ...): stay dead like any app
                finish(manage_panel);
                return;
            }
            _ => {}
        }
        // genuine crash (panic / SIGSEGV / SIGABRT ...): restart, with crash-loop protection
        if window.elapsed().as_secs() > 60 {
            window = std::time::Instant::now();
            fails = 0;
        }
        fails += 1;
        eprintln!("lumen: shell crashed ({:?}), restarting ({})", status, fails);
        if fails > 6 {
            eprintln!("lumen: crash loop, giving up and restoring xfce4-panel");
            finish(manage_panel);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}
