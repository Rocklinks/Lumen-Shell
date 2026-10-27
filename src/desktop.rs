//! freedesktop .desktop entry parsing, app index, Exec expansion and launching.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

#[derive(Clone, Debug, Default)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub exec: String,
}

#[derive(Clone, Debug, Default)]
pub struct App {
    pub id: String, // desktop file id without .desktop
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub icon: String,
    pub exec: String,
    pub terminal: bool,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub wm_class: String,
    pub actions: Vec<Action>,
    pub path: PathBuf,
}

fn locale_keys() -> Vec<String> {
    let l = std::env::var("LC_MESSAGES").ok().or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
    let l = l.split('.').next().unwrap_or("").to_string();
    let mut v = vec![];
    if l.len() >= 2 && l != "C" && l != "POSIX" {
        v.push(l.clone());
        if let Some((a, _)) = l.split_once('_') {
            v.push(a.to_string());
        }
    }
    v
}

fn unescape(s: &str) -> String {
    s.replace("\\s", " ").replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\")
}

pub fn parse_desktop(path: &Path, id: &str, desktops: &[String]) -> Option<App> {
    let txt = std::fs::read_to_string(path).ok()?;
    let locales = locale_keys();
    let mut app = App { id: id.to_string(), path: path.to_path_buf(), ..Default::default() };
    let mut section = String::new();
    let mut ok_type = false;
    let (mut hidden, mut nodisplay) = (false, false);
    let (mut only, mut not): (Vec<String>, Vec<String>) = (vec![], vec![]);
    let mut try_exec = String::new();
    let mut action_ids: Vec<String> = vec![];
    let mut acts: HashMap<String, Action> = HashMap::new();
    let mut loc_name: Option<(usize, String)> = None;
    for line in txt.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l.starts_with('[') {
            section = l.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let (k, v) = match l.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        let (k, v) = (k.trim(), v.trim());
        if section == "Desktop Entry" {
            match k {
                "Type" => ok_type = v == "Application",
                "Name" => {
                    if loc_name.is_none() {
                        app.name = unescape(v);
                    }
                }
                "GenericName" => app.generic = unescape(v),
                "Comment" => app.comment = unescape(v),
                "Icon" => app.icon = v.to_string(),
                "Exec" => app.exec = v.to_string(),
                "TryExec" => try_exec = v.to_string(),
                "Terminal" => app.terminal = v == "true",
                "NoDisplay" => nodisplay = v == "true",
                "Hidden" => hidden = v == "true",
                "OnlyShowIn" => only = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "NotShowIn" => not = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "Categories" => app.categories = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "Keywords" => app.keywords = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                "StartupWMClass" => app.wm_class = v.to_string(),
                "Actions" => action_ids = v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                _ => {
                    if let Some(rest) = k.strip_prefix("Name[") {
                        let loc = rest.trim_end_matches(']');
                        if let Some(pri) = locales.iter().position(|x| x == loc) {
                            if loc_name.as_ref().map_or(true, |(p, _)| pri < *p) {
                                loc_name = Some((pri, unescape(v)));
                            }
                        }
                    }
                }
            }
        } else if let Some(aid) = section.strip_prefix("Desktop Action ") {
            let a = acts.entry(aid.to_string()).or_insert_with(|| Action { id: aid.to_string(), ..Default::default() });
            match k {
                "Name" => a.name = unescape(v),
                "Exec" => a.exec = v.to_string(),
                _ => {}
            }
        }
    }
    if let Some((_, n)) = loc_name {
        app.name = n;
    }
    if !ok_type || hidden || nodisplay || app.exec.is_empty() || app.name.is_empty() {
        return None;
    }
    if !only.is_empty() && !desktops.iter().any(|d| only.contains(d)) {
        return None;
    }
    if desktops.iter().any(|d| not.contains(d)) {
        return None;
    }
    if !try_exec.is_empty() && !which(&try_exec) {
        return None;
    }
    app.actions = action_ids.iter().filter_map(|i| acts.remove(i)).filter(|a| !a.name.is_empty() && !a.exec.is_empty()).collect();
    Some(app)
}

pub fn which(cmd: &str) -> bool {
    if cmd.contains('/') {
        return Path::new(cmd).exists();
    }
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(cmd).exists())
}

pub fn data_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    let home = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(h) = home {
        v.push(h);
    }
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let dirs = if dirs.is_empty() { "/usr/local/share:/usr/share".to_string() } else { dirs };
    for d in dirs.split(':') {
        if !d.is_empty() {
            v.push(PathBuf::from(d));
        }
    }
    v
}

pub fn scan_apps() -> Vec<App> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "XFCE".into()).split(':').map(|s| s.to_string()).collect();
    let mut seen: HashMap<String, App> = HashMap::new();
    for base in data_dirs() {
        let dir = base.join("applications");
        let mut stack = vec![(dir.clone(), 0)];
        while let Some((d, depth)) = stack.pop() {
            let rd = match std::fs::read_dir(&d) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if depth < 3 {
                        stack.push((p, depth + 1));
                    }
                    continue;
                }
                if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                    continue;
                }
                let rel = p.strip_prefix(&dir).unwrap_or(&p).to_string_lossy().replace('/', "-");
                let id = rel.trim_end_matches(".desktop").to_string();
                if seen.contains_key(&id) {
                    continue; // earlier dirs win (user overrides)
                }
                if let Some(a) = parse_desktop(&p, &id, &desktops) {
                    seen.insert(id, a);
                }
            }
        }
    }
    let mut v: Vec<App> = seen.into_values().collect();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// Split an Exec line into argv, expanding field codes.
pub fn exec_argv(exec: &str, app: &App, files: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![];
    let mut cur = String::new();
    let mut in_q = false;
    let mut has = false;
    let mut chars = exec.chars().peekable();
    let push = |args: &mut Vec<String>, cur: &mut String, has: &mut bool| {
        if *has || !cur.is_empty() {
            args.push(std::mem::take(cur));
            *has = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_q = !in_q;
                has = true;
            }
            '\\' if in_q => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !in_q => push(&mut args, &mut cur, &mut has),
            '%' => match chars.next() {
                Some('%') => cur.push('%'),
                Some('f') | Some('u') => {
                    if let Some(f) = files.first() {
                        cur.push_str(f);
                        has = true;
                    }
                }
                Some('F') | Some('U') => {
                    for (i, f) in files.iter().enumerate() {
                        if i > 0 {
                            push(&mut args, &mut cur, &mut has);
                        }
                        cur.push_str(f);
                        has = true;
                    }
                }
                Some('i') => {
                    if !app.icon.is_empty() {
                        push(&mut args, &mut cur, &mut has);
                        args.push("--icon".into());
                        args.push(app.icon.clone());
                    }
                }
                Some('c') => {
                    cur.push_str(&app.name);
                    has = true;
                }
                Some('k') => {
                    cur.push_str(&app.path.to_string_lossy());
                    has = true;
                }
                _ => {}
            },
            _ => cur.push(c),
        }
    }
    push(&mut args, &mut cur, &mut has);
    args
}

pub fn launch(app: &App, action: Option<&Action>, children: &mut Vec<Child>) -> bool {
    let exec = action.map(|a| a.exec.as_str()).unwrap_or(&app.exec);
    let mut argv = exec_argv(exec, app, &[]);
    if argv.is_empty() {
        return false;
    }
    if app.terminal && action.is_none() {
        let term = ["x-terminal-emulator", "xfce4-terminal", "gnome-terminal", "konsole", "xterm"].iter().find(|t| which(t)).copied().unwrap_or("xterm");
        let mut t = vec![term.to_string(), "-e".to_string()];
        t.extend(argv);
        argv = t;
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    match cmd.spawn() {
        Ok(c) => {
            children.push(c);
            true
        }
        Err(_) => false,
    }
}

/// Match a toplevel (class / app_id) to a desktop entry.
pub fn match_app<'a>(apps: &'a [App], app_id: &str) -> Option<&'a App> {
    if app_id.is_empty() {
        return None;
    }
    let l = app_id.to_lowercase();
    let strip = |s: &str| s.to_lowercase().trim_end_matches(".desktop").to_string();
    apps.iter()
        .find(|a| !a.wm_class.is_empty() && a.wm_class.to_lowercase() == l)
        .or_else(|| apps.iter().find(|a| strip(&a.id) == l))
        .or_else(|| apps.iter().find(|a| strip(&a.id).rsplit('.').next() == Some(l.as_str())))
        .or_else(|| apps.iter().find(|a| exec_base(&a.exec) == l))
        .or_else(|| apps.iter().find(|a| a.name.to_lowercase() == l))
}

fn exec_base(exec: &str) -> String {
    let first = exec.split_whitespace().next().unwrap_or("");
    Path::new(first.trim_matches('"')).file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase()
}
