# lumen - Rust GNOME-like Shell for XFCE

One binary, one Settings — a desktop shell written in pure Rust, for XFCE (X11), labwc and Cinnamon.

## Changes

- Dock & Bar: "Extend to the Edge" stops at the top bar (bar never covered); new Indicator Position/Style options; the Settings window stays clear of the dock; GPU-switcher tile only on NVIDIA cards (PCI vendor 0x10de); bar status icons follow GNOME order.
- Night Light: Quick Settings shows a temperature slider while on; turning off resets and removes the `[nightlight]` config section.
- Themes: icon/cursor/GTK and window-border themes detected by content; XFCE keys mirrored from xfce4-settings (xsettings /Net/ThemeName, xfwm4 sync); "Window Borders Follow Theme" — the GNOME Style entry sets `window.frame=client`.
- Power: idle uses a /proc scan (cached 60 s), one wake per minute (30 s max); volume/brightness polls only while Quick Settings is open; ~0.06 wakeups/s, 0 CPU ticks, ~9.5 MB RSS at idle.
- Bluetooth: own stack without bluetoothd — one-time pkexec password prompt; BlueZ backend when bluetoothd runs (mock-bus tested: power, scan, pair, connect, trust, disconnect, forget); scans stop on page exit, connected devices disconnected on exit.
- Lumen Connect: add-on (Quick Settings tile > Mobile Devices); device Share/Plugins/Run Commands; five default commands — Lock Screen, Suspend, Log Out, Restart, Shut Down (XFCE session tools first, then systemd/logind).
- Connect/security: LAN transfer caps (8 handshakes out, 32 in, 4 transfers; slow identity senders cut off after 5 s); exclusive partial files (no overwriting, 32 GiB cap, mode 0600); IPC socket 0600 in private `/tmp/lumen-UID`; BlueZ parsers and ATT client fuzzed against hostile peers; builds warning-free; Xvfb-tested under XFCE + xfwm4.

## How to run

Download the `lumen` binary, make it executable, and move it to `~/.local/bin` (or any directory on your `PATH`). `cargo build --release` builds it from source (std-only, no extra crates).
