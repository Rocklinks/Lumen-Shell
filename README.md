# Lumen Shell

A tiny, GNOME-style **top bar + bottom dock + app grid + settings panel** for
**XFCE on X11** and **Wayland (labwc)**.

It is written in pure Rust with **zero crates and zero GUI libraries** — no GTK, no Qt,
no Xlib, no libwayland bindings. The whole shell is one small binary (~1 MB) that talks
to X11 or Wayland directly.

![status](https://img.shields.io/badge/build-cargo%20build%20--release-informational)
![deps](https://img.shields.io/badge/dependencies-none-brightgreen)

---

## Quick start

You need a Linux desktop with **X11** or **Wayland (labwc)** and, ideally, **XFCE** —
Lumen replaces the XFCE panel while it runs.

### Option A — run the prebuilt binary (fastest)

A ready-to-run build for **Linux x86_64** is included in this repository:

```bash
git clone https://github.com/Rocklinks/Lumen-Shell.git
cd Lumen-Shell
chmod +x lumen-x86_64
./lumen-x86_64            # starts the shell
```

No Rust toolchain needed for this. Skip to [First run](#first-run).

> If `./lumen-x86_64` says *"cannot execute binary file"* you are on a different CPU
> architecture (e.g. ARM). Build from source with Option B instead.

### Option B — build from source

You only need Rust. There are no external libraries and **no network access** is
required for the build (no crates are downloaded).

```bash
# 1. install Rust (once). Any rustc >= 1.75 works.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 2. build and run
git clone https://github.com/Rocklinks/Lumen-Shell.git
cd Lumen-Shell
cargo build --release
./target/release/lumen
```

On Debian/Ubuntu/Mint you can also use the distro package: `sudo apt install cargo`.

There is a convenience script that does the right thing either way
(builds if `cargo` exists, otherwise falls back to the prebuilt binary):

```bash
./run.sh              # start the shell
./run.sh --settings   # ...with any lumen flag
```

---

## First run

1. **Start it** with `./run.sh`, `./lumen-x86_64` or `./target/release/lumen`.
2. The bar appears at the top of the screen and the dock at the bottom.
   While Lumen runs it **stops `xfce4-panel`** so the two do not overlap, and it
   **restarts `xfce4-panel`** automatically when Lumen exits (any way you exit it).
   You can turn this behaviour off in `Settings → Tweaks` or with `manage_panel = false`
   in the config file.
3. **Quit** with `lumen --quit` or `Ctrl+C` in the terminal.
4. Nothing is permanent until you ask for it: config lives in
   `~/.config/lumen/config.ini` (created on first run, hot-reloaded on change).

> Lumen auto-detects the backend: if `WAYLAND_DISPLAY` is set it uses Wayland,
> otherwise X11. Force one with `LUMEN_BACKEND=x11` or `LUMEN_BACKEND=wayland`.

### Start at login

* **XFCE / X11** — run Lumen once, then enable *Settings → Tweaks → Start at login*
  (it writes `~/.config/autostart/lumen-shell.desktop`), or add `lumen` in
  *Settings → Session and Startup → Application Autostart*.
* **labwc** — add `lumen &` to `~/.config/labwc/autostart`.

### Keyboard shortcuts

Bind these in *XFCE → Keyboard → Application Shortcuts* (or labwc `rc.xml`):

| Command | Action |
| --- | --- |
| `lumen --toggle-grid` | app grid (Activities) |
| `lumen --settings` | settings panel |
| `lumen --calendar` | date / notification popup |
| `lumen --quick` | quick settings |
| `lumen --screenshot` | screenshot tool |
| `lumen --record` | start/stop screen recording |
| `lumen --reload` | re-read the config file |
| `lumen --quit` | quit the shell |

On Wayland, *Settings → Keyboard → "Install shortcuts into labwc"* writes them for you.
`lumen --version`, `--shot FILE`, `--display-list`, `--settings-page NAME` and
`--icon-audit` also exist for scripting/debugging.

---

## What it looks like / what it does

| Area | Behaviour |
| --- | --- |
| **Top bar** | workspace pills, app name, clock, status pill (Wi-Fi/Bluetooth/volume/battery/night light), system tray. Every widget can be Off / Left / Center / Right; top or bottom edge; height, transparency, floating pill, always / auto-hide / hide-in-fullscreen; stays above fullscreen windows. |
| **Dock** | pinned + running apps, run indicators; click = launch/focus/minimize/cycle, middle-click = new window, right-click = window list, New Window, Pin/Unpin, Quit. Size, opacity, position, auto-hide. |
| **App grid** | paged, fuzzy type-to-search, keyboard navigation, right-click to pin, real theme icons (PNG + a built-in SVG renderer). |
| **Quick settings** | volume, brightness, Wi-Fi (scan/connect/password), Bluetooth, power mode, night light, dark style, lock/power menu. |
| **Settings** | GNOME-style searchable sidebar: Wi-Fi, Network, Bluetooth, Displays, Sound, Power, Keyboard, GPU, Multitasking, Appearance, Notifications, Screenshots, Date & Time, Top Bar, Dock, Extensions, Tweaks, About. |
| **Notifications** | built-in freedesktop notification server, banners, Do Not Disturb, date menu with notifications next to the calendar. |
| **Tray** | StatusNotifierItem / AppIndicator host with full dbusmenu menus (own D-Bus client, no libdbus). |
| **Screenshots** | GNOME-style tool (Selection / Screen), animated shutter, native capture on X11 (GetImage) and Wayland (wlr-screencopy), own PNG encoder, saved to `~/Screenshots`. |
| **Recording** | MP4 (H.264) to `~/Videos` using `ffmpeg` (X11) or `wf-recorder` (Wayland); red indicator in the bar, click to stop. |
| **Extras** | night light (gamma), caffeine / keep-awake, GPU mode tile, battery percentage, crash supervisor, single-instance guard. |

### Optional command-line tools

Lumen shells out to these in background threads. **None are required** — if a tool is
missing, the matching feature simply hides itself.

`nmcli` (Wi-Fi) · `bluetoothctl` · `pactl` or `wpctl` (volume) · `powerprofilesctl` ·
`supergfxctl` / `envycontrol` (GPU) · `systemctl`, `loginctl`, `xflock4` (power/lock) ·
`ffmpeg` / `wf-recorder` (recording) · `xfce4-panel` (to replace it while running).

Brightness uses sysfs directly, so it needs write permission (e.g. the `video` group or
a udev rule).

---

## Project layout

```
Cargo.toml          # no dependencies at all
src/main.rs         # entry point, CLI flags, supervisor
src/ui/             # bar, dock, app grid, popups, settings, screenshots
src/config.rs       # INI config loader/writer (tolerant, never panics)
src/x11.rs          # native X11 protocol client
src/wl.rs           # native Wayland client (layer-shell, screencopy, gamma...)
src/gfx.rs          # software renderer + window/blit plumbing
src/ttf.rs, src/png.rs, src/pngenc.rs, src/svg.rs, src/icons.rs   # text, image, icon stack
src/desktop.rs      # .desktop files, launching apps
src/dbus.rs         # D-Bus client/server (notifications, tray, portal bits)
src/svc.rs, src/jobs.rs, src/sys.rs, src/theme.rs, src/sysinfo.rs # services and helpers
tools/              # dev/test helpers: fake nmcli, bluetoothctl, pactl, xfce4-panel,
                    # a Python D-Bus app simulator, Xvfb / labwc launchers
```

Read `src/main.rs` first, then `src/ui/mod.rs` — the shell is a single event loop over a
`poll()` of the display fd, the job queue and the IPC socket. `src/tests.rs` (13 tests,
`cargo test --release`) is a good tour of the parsers, including fuzzing and a live
D-Bus round trip.

---

## Configuration

`~/.config/lumen/config.ini` — created on first run, edited by the Settings panel and
**hot-reloaded** (or `lumen --reload`). Every value falls back to a safe default, so a
broken file never stops the shell from starting.

```ini
[bar]
position = top          ; top | bottom
height = 32
opacity = 1
floating = false
visibility = always     ; always | auto-hide | hide-in-fullscreen
left = workspaces
center = clock
right = tray, status

[dock]
icon_size = 48
opacity = 0.92
visibility = always
favorites = firefox, org.gnome.Nautilus, xfce4-terminal
```

---

## Changelog

### Round 3
* **Lifecycle:** while running, lumen stops `xfce4-panel`; when lumen quits for any reason (`lumen --quit`, SIGTERM, "End task", SIGKILL, supervisor death) it starts `xfce4-panel` again.
  A kill is a kill: only genuine crashes (panic/SIGSEGV/abort) respawn the shell. Settings -> Tweaks has "Start at login" and "Replace xfce4-panel".
* **Instant state:** Wi-Fi, Bluetooth, volume, power mode and GPU state are cached and shown on the first frame, then refreshed every 20 s in the background.
  (Root cause of "Wi-Fi not working": the job queue silently dropped scans when >12 jobs were queued - fixed.)
* **Settings rebuilt in GNOME's structure:** searchable sidebar, centered cards of grouped rows (switch, segmented, slider, combo, entry, navigation),
  pages: Wi-Fi, Network, Bluetooth, Displays, Sound, Power, Keyboard, GPU, Multitasking, Appearance, Notifications, Screenshots, Date & Time, Top Bar, Dock, Extensions, Tweaks, About.
* **Screenshots:** Print opens a GNOME-style tool (Selection / Screen, shutter, record). Native capture on X11 (GetImage) and Wayland (wlr-screencopy), own compressed PNG encoder,
  saved to ~/Screenshots (configurable) with a notification. Global shortcuts via X11 key grabs; on labwc use Settings -> Keyboard -> "Install shortcuts into labwc".
* **Screen recording:** MP4 (H.264) saved to ~/Videos, using ffmpeg (X11) or wf-recorder (Wayland). Red indicator in the bar; click it to stop.
* **Display:** X11 resolution/refresh/orientation via native RandR with 15 s auto-revert.
* **Icons:** fixed a rasterizer bug that garbled theme icons touching their right edge (volume, wired network, charging battery, many app icons).
* **Wayland:** fixed clicks right after pointer-enter being reported at (0,0) and a protocol error when temporarily hiding panels.

### Round 2
* Workspace indicator (GNOME 45+ pills) replaces Activities; click = workspace switcher (add/remove, window icons, Workspace settings).
  The old Activities button and a running-app-name widget are optional (Settings -> Top Bar -> Widget placement).
* Bar follows your theme (light/dark); color schemes: Catppuccin Mocha/Macchiato/Frappe/Latte, COSMIC Dark/Light, Nord, Dracula, Gruvbox, Tokyo Night, Rose Pine + accent colors + bar/dock transparency.
* Notifications: built-in freedesktop notification server, GNOME-style banners, date menu with notifications beside the calendar, Do Not Disturb, Clear, unseen dot.
* Tray: StatusNotifierItem / AppIndicator / KStatusNotifierItem host with full dbusmenu menus (own D-Bus client, no libdbus).
* Caffeine: Keep Awake tile (inhibits screensaver/idle via org.freedesktop.ScreenSaver) with timers and optional auto-on in fullscreen.
* GPU tile (supergfxctl / envycontrol), Bluetooth pairing, battery percentage, Wi-Fi rescan flow with real error messages.

---

## Dev tooling

`tools/` has the helper scripts used for testing: fake `nmcli`/`bluetoothctl`/`pactl`/`powerprofilesctl`/`supergfxctl`, a fake `xfce4-panel`, a Python D-Bus app simulator, and X11/labwc launchers.

```bash
cargo test --release     # 13 tests: parser fuzzing + a live dbus-daemon round trip
tools/startx.sh          # Xvfb :7 + xfwm4 (headless X11 test session)
tools/startwl.sh         # headless labwc (Wayland test session)
```

---

## Known limitations (honest list)

* Not yet built: GNOME-51 app-grid folders/drag and shrink animation, world clocks + weather, GSConnect/KDE Connect, idle auto-lock, media/clipboard/system-monitor widgets,
  Wayland display configuration (wlr-output-management), live window thumbnails, Blur-My-Shell style wallpaper blur, user theme files.
* Screenshot overlay: on the headless labwc used for testing, key events (Esc/Enter) did not reach the overlay; use the on-screen buttons. Unverified on a real labwc seat. "Window" screenshot mode is not implemented.
* xfwl4 is not tested (it is not usable yet); lumen relies only on wlr-layer-shell and detects every other protocol at runtime.
* Not tested on real hardware (GPU, radios, battery, real games); tested in Xvfb+xfwm4 and headless labwc only.
* Exclusive-fullscreen / direct-scanout games cannot be overlaid by any shell; borderless fullscreen works.
* Bar and dock are horizontal only (top/bottom). Legacy XEmbed tray icons are not supported (SNI/AppIndicator only).
* HDR: not possible on X11 or labwc (shown as unavailable).
* Wayland: HiDPI buffer scaling, per-monitor bars and ext-workspace (needs labwc >= 0.8) are untested/partial. WPA-Enterprise Wi-Fi unsupported.
* Fonts must be TrueType (glyf); CFF/OTF-only fonts fall back to DejaVu/Liberation/Noto if present.
* English-only labels.

---

## Contributing

Only basic Rust knowledge is needed — there is no framework and no dependency tree to
learn, and the build is instant.

1. `cargo build --release && ./target/release/lumen`
2. `cargo test --release` before sending a patch.
3. Areas that would help most are listed under *Known limitations*.

Bug reports are welcome: include your session type (X11/Wayland), distro, and the output
of `lumen --version`.
