# Lumen

A GNOME-like Shell for Xfce and labwc: top bar, dock, app grid, Quick Settings,
Settings, workspace overview, lock screen, notifications, screenshots and screen recording. Rust, standard library
only, **one binary** (`lumen`: shell, Bluetooth, Lumen Connect). Light on memory and battery.

Idle on labwc: about 9 MB resident (6.5 MB of it really its own), one thread, about 2 wakeups a minute. The Settings window adds about 10 MB while it is open and gives it back when closed.

## Files
- `lumen` the binary (x86-64 Linux)
- `lumen-src.tar.gz` the source (Cargo workspace: lumen-shell, lumen-bt, lumen-connect, lumen-dbus)
- `README.md` this file

## Changes and Fixes

### Round 25
- Workspace overview follows the window manager's own workspace count and gained the workspace buttons.
- Lock screen redesigned (Cinnamon layout) with its settings page; the lock now follows the screen-off timer.
- Fixed: the lock screen settings (Background, Blur, Pixelate, Greyscale, Vignette, Style, Ring Size) shared their
  ids with the mouse page and changed pointer settings instead.
- Added: desktop icons, auto-tiling, frosted glass, Matugen-Labwc install, native Super+S / Super+R capture.
- Quick Settings icons fixed to Adwaita; toasts moved to banners; Caffeine page trimmed.
- Fixed: Chromium web-app notifications named after the browser instead of the app.
- Memory and battery: fonts memory-mapped (about 4 MB less), theme and font changes by inotify instead of polling
  every 2 seconds, no second copy of the wallpaper or the desktop-icon surface, region-only screen copies,
  spare window buffers and freed heap handed back to the system whenever the shell goes idle.
  Binary about 2.7 MB.

### Round 24
- [Additional fixes and changes from previous rounds]

## How to run it

1. Download the binary and copy it to `./local/bin`:
   ```bash
   ./lumen
   ```
2. Or run it directly from the current directory:
   ```bash
   ./lumen
   ```

The binary is self-contained — no installation beyond copying to `./local/bin` is needed.

### Start at login

- **XFCE / X11** — run Lumen once, then enable Settings → Tweaks → Start at login (it writes `~/.config/autostart/lumen-shell.desktop`), or add lumen in Settings → Session and Startup → Application Autostart.
- **labwc** — add `lumen &` to `~/.config/labwc/autostart`.

### Keyboard shortcuts

Bind them to keys: `lumen --overview`, `--toggle-grid`, `--settings`, `--lock`,
--calendar`, `--quick`, `--shot-area` (Super+S), `--shot-screen`, `--record-area` (Super+R), `--record`,
--tile` (Super+Y), `--volume-up|down|mute`, `--brightness-up|down`, `--reload`, `--quit`.
Settings > Keyboard writes the shortcuts into labwc's rc.xml for you.

## Features

**Everywhere (XFCE and labwc)**
- Top bar (activities, workspace dots, clock, tray, status icons), floating dock with favourites and running apps,
  full-screen app grid with folders, Quick Settings (Wi-Fi, Bluetooth, volume, brightness, Night Light, Dark Style,
  Caffeine, power profile, Lumen Connect), calendar with weather and events, notification banners and history.
- Quick Settings and the status icons always use GNOME's default icon theme (Adwaita) when installed, whatever
  icon theme the desktop uses. Messages ("Volume 40%", errors) show as a banner under the bar; the clock keeps its place.
- Caffeine: tile switches it on and off; the page offers 5 minutes to 2 hours.
- Workspace overview: a row of workspace buttons shows exactly the workspaces the window manager has (XFCE's
  workspace count; on labwc the desktops of rc.xml) and switches between them. Window cards with close buttons,
  neighbours as strips, type to search every window, scroll or arrow keys to switch.
- Notifications: Chromium and Brave web apps (WhatsApp Web) show their own name and icon.
- Screenshots, no extra programs: **Super+S**, drag over the area, let go: saved to `~/Screenshots`. Enter takes the
  whole screen, Esc or right click cancels.
- Screen recording, no extra programs: **Super+R**, drag over the area, let go: recording starts. Super+R pauses,
  Super+R resumes, and while paused **S saves** (Esc resumes). Saved to the Videos folder (default `~/Videos`) as an
  AVI (Motion JPEG) that every player opens. Own JPEG encoder and AVI writer, encoded on a worker thread, only the
  chosen area is copied, the pointer is recorded. No sound. The Quick Settings buttons do the same.
- Keep Awake on XFCE uses xfce4-power-manager's Presentation mode; labwc uses the screensaver service and a logind inhibitor.

**labwc only**
- Lock screen in the Cinnamon screen locker layout: floating clock with away message, media widget (any MPRIS
  player: art, title, previous/play/next), unlock dialog that appears on a key or mouse move. Settings > Power:
  lock when put to sleep, delay before locking after the screen turns off, away message, floating widgets, media
  controls, style (Cinnamon, Lumen, Ring), grace period, fade-in, screenshot background with blur, pixelate,
  greyscale, vignette. `loginctl lock-session` locks with Lumen.
- Desktop icons like xfdesktop: Home, File System, Trash and the Desktop folder in a grid, double click opens,
  drag moves (the place is remembered), right click for Open / Move to Trash / Empty Trash. Only the icons take
  input, so labwc's root menu still works. Switches in Settings > Appearance.
- Auto-tiling like COSMIC (Settings > Appearance > Window Tiling, or Super+Y): the windows of a workspace split the
  screen as a binary tree, up to 6 per workspace. labwc has no tiling, so Lumen writes one region per layout into
  rc.xml (marked blocks, your file is backed up as `rc.xml.lumen.bak`) and presses a hidden key (F13 to F33) per
  window on a virtual keyboard. The first time, labwc needs about a second and a half to read the new regions.
- Frosted glass (Settings > Appearance > Transparency): the top bar and dock show a blurred, tinted copy of Lumen's
  own wallpaper, like COSMIC's glass panels. COSMIC Dark and Light schemes are included.
- Window themes: Matugen-Labwc and the other bundled themes are written to `~/.local/share/themes` at start, so
  labwc and the Window Borders list see them as installed themes.

## What changed in this version
- Workspace overview follows the window manager's own workspace count and gained the workspace buttons.
- Lock screen redesigned (Cinnamon layout) with its settings page; the lock now follows the screen-off timer.
- Fixed: the lock screen settings (Background, Blur, Pixelate, Greyscale, Vignette, Style, Ring Size) shared their
  ids with the mouse page and changed pointer settings instead.
- Added: desktop icons, auto-tiling, frosted glass, Matugen-Labwc install, native Super+S / Super+R capture.
- Quick Settings icons fixed to Adwaita; toasts moved to banners; Caffeine page trimmed.
- Fixed: Chromium web-app notifications named after the browser instead of the app.
- Memory and battery: fonts memory-mapped (about 4 MB less), theme and font changes by inotify instead of polling
  every 2 seconds, no second copy of the wallpaper or the desktop-icon surface, region-only screen copies,
  spare window buffers and freed heap handed back to the system whenever the shell goes idle.
  Binary about 2.7 MB.

## Round 25
- Helium (and other Chromium forks): notifications are named after the web app, and its windows match the installed
  web app (`--app-id=` or `--app=URL`) whatever the browser brand, so clicking the entry starts or focuses the app
  window instead of the browser tab.
- Workspaces: the overview shows one box per workspace (app icons on it, current one outlined); a click opens it.
  Dock: a divider and the show-apps button at the far end, with a drawn fallback glyph (never an empty box).
- GPU tile: shown on every machine with a switching tool or two graphics chips (it was hidden unless an NVIDIA card
  was found). Without a tool it picks the GPU new apps start on (integrated or discrete, PRIME offload).
- Auto-tiling: rc.xml path escaping, panic guards; a failure switches tiling off with a message instead of closing Settings.
- Lockscreen: new page (labwc only) with the locker, background (Screenshot, Desktop Wallpaper, Picture with a
  picker, Dark), effects, layout and away message.
- Themes page: Special Themes (Default, Frosted Glass, Material You, COSMIC Dark) restyle bar, dock, corners, accent and
  window borders at once; Glass Transparency slider. Frosted glass also works without Lumen's own wallpaper.
- Matugen-Labwc: new button set; picked automatically for Frosted Glass and Material You (matugen detected, else bundled).
173 unit tests pass. Not run on a real labwc session here.

## Tested
170 unit tests pass. Run for real on a headless labwc 0.7.1: desktop icons, the lock-free parts of the shell, tiling
of three windows through regions and the virtual keyboard, frosted glass bar and dock, the Super+S overlay, and a
Super+R recording (pause, S to save) that ffprobe and ffmpeg read (MJPEG, right size, frame count and duration).

**Not tested on real hardware or full sessions:** an XFCE session, the lock screen on a real labwc session (unlock
with a real password), suspend and lock-before-sleep, MPRIS media widget with a real player, Keep Awake with a real
xfce4-power-manager, a second monitor. Recordings of very large areas (4K) encode on the CPU and may drop frames; the
playback speed stays right because the real frame rate is written into the file.