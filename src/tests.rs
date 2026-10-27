//! Robustness + logic tests. Parsers must never panic on hostile input.
use crate::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(src: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut v = src.to_vec();
    match rng.below(5) {
        0 => v.truncate(rng.below(v.len().max(1))),
        1 => {
            for _ in 0..1 + rng.below(16) {
                if !v.is_empty() {
                    let i = rng.below(v.len());
                    v[i] ^= 1 << rng.below(8);
                }
            }
        }
        2 => {
            for _ in 0..1 + rng.below(8) {
                if !v.is_empty() {
                    let i = rng.below(v.len());
                    v[i] = rng.next() as u8;
                }
            }
        }
        3 => {
            let i = rng.below(v.len().max(1));
            let j = (i + rng.below(64)).min(v.len());
            v.drain(i..j);
        }
        _ => {
            let n = rng.below(256);
            for _ in 0..n {
                v.push(rng.next() as u8);
            }
        }
    }
    v
}

fn find_first(dir: &str, ext: &str) -> Option<std::path::PathBuf> {
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|x| x.to_str()) == Some(ext) && std::fs::metadata(&p).map_or(false, |m| m.len() > 200 && m.len() < 600_000) {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[test]
fn fuzz_png() {
    let p = find_first("/usr/share/icons", "png").expect("a png");
    let src = std::fs::read(p).unwrap();
    assert!(png::decode_png(&src).is_some(), "valid png must decode");
    let mut r = Rng(0x1234_5678_9abc_def1);
    for _ in 0..4000 {
        let m = mutate(&src, &mut r);
        let _ = png::decode_png(&m);
    }
    for _ in 0..2000 {
        let n = r.below(600);
        let rnd: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        let _ = png::decode_png(&rnd);
        let _ = png::inflate(&rnd, 1 << 20);
    }
}

#[test]
fn fuzz_svg() {
    let p = find_first("/usr/share/icons", "svg").expect("an svg");
    let src = std::fs::read(p).unwrap();
    assert!(svg::render(&String::from_utf8_lossy(&src), 48, None).is_some(), "valid svg must render");
    let mut r = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..1500 {
        let m = mutate(&src, &mut r);
        let _ = svg::render(&String::from_utf8_lossy(&m), 32, if r.below(2) == 0 { Some(0xFFFFFFFF) } else { None });
    }
    // pathological inputs
    for s in ["", "<", "<svg", "<svg viewBox='0 0 0 0'/>", "<svg viewBox='0 0 1e30 1e30'><path d='M0 0 L1e30 1e30 z'/></svg>", "<svg viewBox='0 0 10 10'><path d='M0 0 A 0 0 0 1 1 5 5 A -1e9 1e9 7 0 0 3 3 Z'/></svg>", "<svg viewBox='0 0 10 10'><use href='#a'/><g id='a'><use href='#a'/></g></svg>", "<svg viewBox='0 0 10 10' transform='scale(nan)'><circle r='1e9'/></svg>"] {
        let _ = svg::render(s, 24, None);
    }
}

#[test]
fn fuzz_ttf() {
    let p = std::path::Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
    let src = std::fs::read(p).unwrap();
    let f = ttf::Font::parse(src.clone()).expect("valid font");
    assert!(f.has_glyph('A'));
    let mut r = Rng(0x0bad_c0de_1357_9bdf);
    for _ in 0..300 {
        let m = mutate(&src, &mut r);
        if let Some(f) = ttf::Font::parse(m) {
            for c in "AaÀ€x ~".chars() {
                let g = f.glyph_index(c);
                let _ = f.outline(g);
                let _ = f.advance(g);
            }
        }
    }
    let mut tc = gfx::TextCtx::new(vec![ttf::Font::parse(src).unwrap()]);
    let mut cv = gfx::Canvas::new(64, 32);
    for ch in ['A', '日', '\u{1F600}', '\0', '\u{FFFF}'] {
        let _ = tc.glyph(ch, 14.0);
        tc.draw(&mut cv, &ch.to_string(), 14.0, -20.0, 40.0, 0xFFFFFFFF);
    }
    tc.draw(&mut cv, "huge", 400.0, 0.0, 0.0, 0xFFFFFFFF);
    tc.draw(&mut cv, "tiny", 0.5, 0.0, 0.0, 0xFFFFFFFF);
}

#[test]
fn config_tolerant_and_roundtrip() {
    let dir = std::env::temp_dir().join(format!("lumen-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("XDG_CONFIG_HOME", &dir);
    let mut r = Rng(42);
    let junk = ["[bar]\nheight = abc\nopacity = 99\n", "[[[\n===\n=\n", "[bar]\nleft = , , ,\nposition = sideways\n", "\u{0}\u{1}\u{2}", "[dock]\nicon_size = -5\nfavorites =\n", "[general]\nui_scale = nan\n"];
    for j in junk {
        std::fs::create_dir_all(dir.join("lumen")).unwrap();
        std::fs::write(dir.join("lumen/config.ini"), j).unwrap();
        let c = config::Config::load();
        assert!(c.bar_height >= 20 && c.bar_height <= 96);
        assert!(c.bar_opacity >= 0.0 && c.bar_opacity <= 1.0);
        assert!(c.dock_icon >= 24 && c.dock_icon <= 96);
        assert!(c.ui_scale.is_finite() && c.ui_scale >= 0.5 && c.ui_scale <= 3.0);
    }
    for _ in 0..500 {
        let n = r.below(300);
        let b: Vec<u8> = (0..n).map(|_| (r.next() as u8) % 100 + 10).collect();
        std::fs::write(dir.join("lumen/config.ini"), &b).unwrap();
        let _ = config::Config::load();
    }
    // roundtrip
    let _ = std::fs::remove_file(dir.join("lumen/config.ini"));
    let mut c = config::Config::load();
    c.bar_height = 40;
    c.favorites = vec!["a".into(), "b".into()];
    c.right = vec!["status".into()];
    c.save();
    let c2 = config::Config::load();
    assert_eq!(c2.bar_height, 40);
    assert_eq!(c2.favorites, vec!["a", "b"]);
    assert_eq!(c2.right, vec!["status"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_exec_expansion() {
    let app = desktop::App { name: "Test App".into(), icon: "ico".into(), path: "/x/t.desktop".into(), ..Default::default() };
    assert_eq!(desktop::exec_argv("foo --bar %f", &app, &[]), vec!["foo", "--bar"]);
    assert_eq!(desktop::exec_argv("foo %F", &app, &["a b".into(), "c".into()]), vec!["foo", "a b", "c"]);
    assert_eq!(desktop::exec_argv("\"/opt/my app/run\" --x=\"1 2\" 100%%", &app, &[]), vec!["/opt/my app/run", "--x=1 2", "100%"]);
    assert_eq!(desktop::exec_argv("env A=1 prog %i %c", &app, &[]), vec!["env", "A=1", "prog", "--icon", "ico", "Test App"]);
    assert!(desktop::exec_argv("", &app, &[]).is_empty());
    assert!(desktop::exec_argv("%U %u", &app, &[]).is_empty());
}

#[test]
fn desktop_parse_robust() {
    let dir = std::env::temp_dir().join(format!("lumen-dt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = "[Desktop Entry]\nType=Application\nName=Hello\nName[xx]=Salut\nExec=hello %U\nIcon=hello\nCategories=Utility;Test;\n[Desktop Action new]\nName=New\nExec=hello --new\n";
    let mut r = Rng(7);
    let p = dir.join("a.desktop");
    std::fs::write(&p, good).unwrap();
    let a = desktop::parse_desktop(&p, "a", &["XFCE".into()]).expect("parses");
    assert_eq!(a.name, "Hello");
    assert_eq!(a.categories, vec!["Utility", "Test"]);
    for _ in 0..1500 {
        let m = mutate(good.as_bytes(), &mut r);
        std::fs::write(&p, &m).unwrap();
        let _ = desktop::parse_desktop(&p, "a", &["XFCE".into()]);
    }
    let hidden = "[Desktop Entry]\nType=Application\nName=H\nExec=h\nNoDisplay=true\n";
    std::fs::write(&p, hidden).unwrap();
    assert!(desktop::parse_desktop(&p, "a", &["XFCE".into()]).is_none());
    let only = "[Desktop Entry]\nType=Application\nName=H\nExec=h\nOnlyShowIn=GNOME;\n";
    std::fs::write(&p, only).unwrap();
    assert!(desktop::parse_desktop(&p, "a", &["XFCE".into()]).is_none());
    assert!(desktop::parse_desktop(&p, "a", &["GNOME".into()]).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn calendar_math() {
    use ui::draw::*;
    assert_eq!(weekday(2026, 10, 17), 6); // Saturday
    assert_eq!(weekday(2000, 1, 1), 6);
    assert_eq!(weekday(2024, 2, 29), 4);
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2100, 2), 28);
    assert_eq!(days_in_month(2000, 2), 29);
    assert_eq!(days_in_month(2026, 4), 30);
}

#[test]
fn theme_parsing() {
    let r = theme::Raw { gtk_theme: "Adwaita-dark".into(), icon_theme: "Adwaita".into(), font: "Noto Sans Bold 12".into(), prefer_dark: None };
    let t = theme::resolve(config::ThemeMode::Auto, &r, 1.0, None);
    assert!(t.dark);
    assert_eq!(t.font_family, "Noto Sans");
    assert_eq!(t.font_size, 12.0);
    let t2 = theme::resolve(config::ThemeMode::Light, &r, 1.0, None);
    assert!(!t2.dark);
    let t3 = theme::resolve(config::ThemeMode::Auto, &theme::Raw::default(), 1.0, Some(true));
    assert!(t3.dark, "portal preference wins in Auto");
    // empty/garbage fonts never panic
    for f in ["", " ", "12", "Bold", "A B C D 1e99", "\u{0}"] {
        let _ = theme::resolve(config::ThemeMode::Auto, &theme::Raw { font: f.into(), ..Default::default() }, 1.0, None);
    }
}

#[test]
fn gfx_shapes_never_panic() {
    let mut cv = gfx::Canvas::new(40, 30);
    let mut r = Rng(99);
    for _ in 0..3000 {
        let f = |r: &mut Rng| (r.next() % 200) as f32 - 80.0;
        match r.below(6) {
            0 => cv.fill_rrect(f(&mut r), f(&mut r), f(&mut r), f(&mut r), f(&mut r), 0x80FF0000),
            1 => cv.fill_circle(f(&mut r), f(&mut r), f(&mut r).abs(), 0xFF00FF00),
            2 => cv.fill_rect(f(&mut r) as i32, f(&mut r) as i32, f(&mut r) as i32, f(&mut r) as i32, 0x40000000),
            3 => cv.stroke_rrect(f(&mut r), f(&mut r), f(&mut r), f(&mut r), f(&mut r), f(&mut r), 0xFF0000FF),
            4 => {
                let mut s = gfx::Shape::for_rect(f(&mut r), f(&mut r), f(&mut r).abs(), f(&mut r).abs(), 1);
                s.polygon(&[(f(&mut r), f(&mut r)), (f(&mut r), f(&mut r)), (f(&mut r), f(&mut r))]);
                s.arc(f(&mut r), f(&mut r), f(&mut r).abs(), 2.0, f(&mut r), f(&mut r));
                cv.paint(&s, 0xFFFFFFFF);
            }
            _ => {
                let img = gfx::Image { w: 3, h: 3, px: vec![0xFF112233; 9] };
                cv.draw_image(&img, f(&mut r) as i32, f(&mut r) as i32, (f(&mut r) as i32).abs(), (f(&mut r) as i32).abs());
            }
        }
    }
}

#[test]
fn xdg_theme_color_parse() {
    // negative/odd inputs through svg tint & mix
    assert_eq!(gfx::mix(0xFF000000, 0xFFFFFFFF, 2.0), 0xFFFFFFFF);
    assert_eq!(gfx::mix(0xFF000000, 0xFFFFFFFF, -1.0), 0xFF000000);
    assert_eq!(gfx::with_alpha(0xFFFFFFFF, 0.0) >> 24, 0);
}

#[test]
fn kelvin_monotonic() {
    let (r6, g6, b6) = wl::kelvin_rgb(6500);
    assert!(r6 > 0.95 && g6 > 0.9 && b6 > 0.9);
    let (r3, _g3, b3) = wl::kelvin_rgb(3000);
    assert!(r3 > b3, "warm light has more red than blue");
    let (_, _, bmin) = wl::kelvin_rgb(1000);
    assert!(bmin <= b3);
    for k in [-5, 0, 1, 99999, i32::MAX, i32::MIN] {
        let (a, b, c) = wl::kelvin_rgb(k);
        assert!((0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) && (0.0..=1.0).contains(&c));
    }
}

#[test]
fn dbus_codec_roundtrip_and_fuzz() {
    use dbus::*;
    let body = vec![
        Val::s("hello"),
        Val::U32(7),
        Val::Arr("s".into(), vec![Val::s("a"), Val::s("bc")]),
        dict_sv(vec![("k", Val::I32(-5)), ("d", Val::F64(1.5)), ("nested", Val::Struct(vec![Val::U8(1), Val::Str("x".into())])), ("b", Val::Bool(true))]),
        Val::Arr("(iiay)".into(), vec![Val::Struct(vec![Val::I32(2), Val::I32(2), Val::Arr("y".into(), vec![Val::U8(1), Val::U8(2), Val::U8(3), Val::U8(4)])])]),
        Val::Arr("i".into(), vec![]),
        Val::I64(-9), Val::Obj("/a/b".into()),
    ];
    let m = Msg { mtype: 1, serial: 5, path: "/x".into(), iface: "a.b".into(), member: "M".into(), dest: "d.e".into(), body: body.clone(), ..Default::default() };
    let enc = encode(&m);
    let d = decode(&enc).expect("decodes");
    assert_eq!(d.body, body);
    assert_eq!(d.member, "M");
    assert_eq!(d.path, "/x");
    assert_eq!(d.serial, 5);
    let mut r = Rng(0xfeed_beef);
    for _ in 0..20000 {
        let mm = mutate(&enc, &mut r);
        let _ = decode(&mm);
    }
    for _ in 0..5000 {
        let n = r.below(200);
        let rnd: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        let _ = decode(&rnd);
    }
}

#[test]
fn dbus_live_session_roundtrip() {
    use dbus::*;
    use std::io::Read;
    let child = std::process::Command::new("dbus-daemon").args(["--session", "--print-address", "--nofork"]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => return, // no dbus-daemon available: skip
    };
    let mut addr = String::new();
    {
        let out = child.stdout.as_mut().unwrap();
        let mut b = [0u8; 1];
        while out.read(&mut b).map_or(false, |n| n == 1) && b[0] != b'\n' {
            addr.push(b[0] as char);
        }
    }
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", addr.trim());
    let mut a = Bus::connect().expect("a connects");
    let mut b = Bus::connect().expect("b connects");
    assert!(a.unique.starts_with(':') && b.unique.starts_with(':') && a.unique != b.unique);
    a.request_name("org.lumen.Test");
    a.flush();
    let mut owned = false;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        for e in a.poll().unwrap() {
            if let Ev::Reply { tag, msg } = e {
                if tag.starts_with("reqname:") {
                    owned = msg.body.first().and_then(|v| v.as_i64()) == Some(1);
                }
            }
        }
        if owned {
            break;
        }
    }
    assert!(owned, "name ownership granted");
    // b calls a with a complex body; a replies
    b.send_call("t1", "org.lumen.Test", "/obj", "org.lumen.I", "Echo", vec![Val::s("ping"), dict_sv(vec![("x", Val::U32(3))])]);
    b.flush();
    let mut got_call = None;
    let mut got_reply = None;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        for e in a.poll().unwrap() {
            if let Ev::Call(m) = e {
                if m.member == "Echo" {
                    got_call = Some(m);
                }
            }
        }
        if let Some(m) = got_call.take() {
            assert_eq!(m.body[0].as_str(), Some("ping"));
            assert_eq!(m.body[1].dict_get("x").and_then(|v| v.as_i64()), Some(3));
            a.reply(&m, vec![Val::s("pong")]);
            a.flush();
        }
        for e in b.poll().unwrap() {
            if let Ev::Reply { tag, msg } = e {
                if tag == "t1" {
                    got_reply = Some(msg);
                }
            }
        }
        if got_reply.is_some() {
            break;
        }
    }
    let r = got_reply.expect("reply arrived");
    assert_eq!(r.mtype, 2, "error reply: {} {:?}", r.error, r.body);
    assert_eq!(r.body[0].as_str(), Some("pong"));
    // unknown name -> error reply
    b.send_call("t2", "org.nobody.Home", "/", "x.y", "Z", vec![]);
    b.flush();
    let mut err = None;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        for e in b.poll().unwrap() {
            if let Ev::Reply { tag, msg } = e {
                if tag == "t2" {
                    err = Some(msg);
                }
            }
        }
        if err.is_some() {
            break;
        }
    }
    assert_eq!(err.expect("error reply").mtype, 3);
    let _ = child.kill();
}
