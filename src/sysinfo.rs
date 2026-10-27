//! Dependency-free system state: battery (sysfs), brightness (sysfs), network link state (sysfs).
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battery {
    pub present: bool,
    pub percent: i32,
    pub charging: bool,
    pub full: bool,
}

pub fn battery() -> Battery {
    if let Ok(v) = std::env::var("LUMEN_FAKE_BATTERY") {
        if let Ok(p) = v.parse::<i32>() {
            return Battery { present: true, percent: p.clamp(0, 100), charging: false, full: false };
        }
    }
    let rd = match fs::read_dir("/sys/class/power_supply") {
        Ok(r) => r,
        Err(_) => return Battery::default(),
    };
    for e in rd.flatten() {
        let p = e.path();
        let ty = fs::read_to_string(p.join("type")).unwrap_or_default();
        if ty.trim() != "Battery" {
            continue;
        }
        // skip peripheral batteries (scope=Device)
        if fs::read_to_string(p.join("scope")).map_or(false, |s| s.trim() == "Device") {
            continue;
        }
        let cap = fs::read_to_string(p.join("capacity")).ok().and_then(|s| s.trim().parse::<i32>().ok());
        let st = fs::read_to_string(p.join("status")).unwrap_or_default();
        if let Some(c) = cap {
            return Battery { present: true, percent: c.clamp(0, 100), charging: st.trim() == "Charging", full: st.trim() == "Full" };
        }
    }
    Battery::default()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Net {
    None,
    Wifi { up: bool },
    Wired { up: bool },
}

pub fn network() -> Net {
    let rd = match fs::read_dir("/sys/class/net") {
        Ok(r) => r,
        Err(_) => return Net::None,
    };
    let (mut wifi, mut wired): (Option<bool>, Option<bool>) = (None, None);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "lo" || name.starts_with("docker") || name.starts_with("veth") || name.starts_with("br-") || name.starts_with("virbr") {
            continue;
        }
        let p = e.path();
        let up = fs::read_to_string(p.join("operstate")).map_or(false, |s| s.trim() == "up");
        if p.join("wireless").exists() {
            wifi = Some(wifi.unwrap_or(false) || up);
        } else if p.join("device").exists() {
            wired = Some(wired.unwrap_or(false) || up);
        }
    }
    match (wifi, wired) {
        (_, Some(true)) => Net::Wired { up: true },
        (Some(u), _) => Net::Wifi { up: u },
        (None, Some(u)) => Net::Wired { up: u },
        _ => Net::None,
    }
}

pub struct Backlight {
    dir: PathBuf,
    pub max: i32,
}
impl Backlight {
    pub fn find() -> Option<Backlight> {
        let rd = fs::read_dir("/sys/class/backlight").ok()?;
        let mut best: Option<(i32, Backlight)> = None;
        for e in rd.flatten() {
            let p = e.path();
            let max = fs::read_to_string(p.join("max_brightness")).ok().and_then(|s| s.trim().parse::<i32>().ok())?;
            if max <= 0 {
                continue;
            }
            let kind = fs::read_to_string(p.join("type")).unwrap_or_default();
            let rank = match kind.trim() {
                "firmware" => 3,
                "platform" => 2,
                _ => 1,
            };
            if best.as_ref().map_or(true, |b| rank > b.0) {
                best = Some((rank, Backlight { dir: p, max }));
            }
        }
        best.map(|b| b.1)
    }
    pub fn get(&self) -> f32 {
        let v = fs::read_to_string(self.dir.join("brightness")).ok().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(0);
        v as f32 / self.max as f32
    }
    /// Direct sysfs write (works when udev rules/groups allow). Returns false if not permitted.
    pub fn set(&self, frac: f32) -> bool {
        let v = ((frac.clamp(0.02, 1.0)) * self.max as f32).round() as i32;
        fs::write(self.dir.join("brightness"), v.to_string()).is_ok()
    }
}

fn rd(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

fn gib(bytes: u64) -> String {
    let g = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if g >= 100.0 { format!("{:.0} GB", g) } else { format!("{:.1} GB", g) }
}

fn gpu_name(card: &std::path::Path) -> Option<String> {
    let dev = card.join("device");
    let vendor = rd(&dev.join("vendor").to_string_lossy());
    let driver = fs::read_to_string(dev.join("uevent")).ok()?.lines().find_map(|l| l.strip_prefix("DRIVER=").map(|s| s.to_string())).unwrap_or_default();
    let v = match vendor.as_str() {
        "0x10de" => "NVIDIA",
        "0x1002" => "AMD",
        "0x8086" => "Intel",
        "0x1af4" | "0x1234" => "Virtual GPU",
        "0x15ad" => "VMware",
        "0x1414" => "Microsoft",
        _ => "",
    };
    if v.is_empty() && driver.is_empty() {
        return None;
    }
    Some(format!("{}{}{}", v, if v.is_empty() || driver.is_empty() { "" } else { " (" }, if driver.is_empty() { String::new() } else if v.is_empty() { driver.clone() } else { format!("{})", driver) }))
}

/// System details for the About page, GNOME-style. Pure file reads; no tools needed.
pub fn about() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vec![];
    let host = rd("/etc/hostname");
    v.push(("Device Name".into(), if host.is_empty() { rd("/proc/sys/kernel/hostname") } else { host }));
    let (vendor, prod, ver) = (rd("/sys/class/dmi/id/sys_vendor"), rd("/sys/class/dmi/id/product_name"), rd("/sys/class/dmi/id/product_version"));
    let model = [vendor, prod, ver].iter().filter(|x| !x.is_empty() && !x.contains("To Be Filled") && x.as_str() != "None").cloned().collect::<Vec<_>>().join(" ");
    if !model.is_empty() {
        v.push(("Hardware Model".into(), model));
    }
    let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default().lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse::<u64>().ok())).unwrap_or(0);
    if mem > 0 {
        v.push(("Memory".into(), gib(mem * 1024)));
    }
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu = cpuinfo.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().to_string()).unwrap_or_default();
    let threads = cpuinfo.lines().filter(|l| l.starts_with("processor")).count();
    if !cpu.is_empty() {
        let clean = cpu.replace("(R)", "").replace("(TM)", "").replace("  ", " ");
        v.push(("Processor".into(), if threads > 0 { format!("{} × {}", clean, threads) } else { clean }));
    }
    let mut gpus: Vec<String> = vec![];
    if let Ok(rd_) = fs::read_dir("/sys/class/drm") {
        let mut cards: Vec<_> = rd_.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).map_or(false, |n| n.starts_with("card") && !n.contains('-'))).collect();
        cards.sort();
        for c in cards {
            if let Some(n) = gpu_name(&c) {
                if !gpus.contains(&n) {
                    gpus.push(n);
                }
            }
        }
    }
    if !gpus.is_empty() {
        v.push(("Graphics".into(), gpus.join(", ")));
    }
    if let Some((total, free)) = crate::sys::disk_space("/") {
        v.push(("Disk Capacity".into(), format!("{} ({} free)", gib(total), gib(free))));
    }
    let osr = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let get = |k: &str| osr.lines().find_map(|l| l.strip_prefix(&format!("{}=", k))).map(|s| s.trim_matches('"').to_string());
    v.push(("OS Name".into(), get("PRETTY_NAME").unwrap_or_else(|| "Linux".into())));
    v.push(("OS Type".into(), if cfg!(target_pointer_width = "64") { "64-bit".into() } else { "32-bit".into() }));
    v.push(("Kernel Version".into(), rd("/proc/sys/kernel/osrelease")));
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let ws = if std::env::var("WAYLAND_DISPLAY").is_ok() && session != "x11" { "Wayland" } else { "X11" };
    v.push(("Windowing System".into(), ws.into()));
    let de = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "XFCE".into());
    v.push(("Desktop".into(), format!("{} · lumen-shell {}", de, env!("CARGO_PKG_VERSION"))));
    v.push(("Session".into(), if session.is_empty() { ws.to_lowercase() } else { session }));
    let up: f64 = rd("/proc/uptime").split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    v.push(("Uptime".into(), if up >= 86400.0 { format!("{}d {}h", (up / 86400.0) as u32, ((up % 86400.0) / 3600.0) as u32) } else { format!("{}h {}m", (up / 3600.0) as u32, ((up % 3600.0) / 60.0) as u32) }));
    v
}
