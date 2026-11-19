# lumen: one binary, one Settings

Workspace: lumen-shell + lumen-bt + lumen-connect, std-only, linked into ONE binary `lumen`.
Build: `cargo build --release`. XFCE (X11), labwc, Cinnamon. Plasma code removed.

## Settings (the only settings, no Tweaks app)
Wi-Fi, Network, Bluetooth | Displays, Sound, Power, Keyboard (shortcuts incl. screenshot/recording, keyboard, mouse, touchpad) |
Appearance, Themes, Fonts, Window Titlebars, Dock & Bar (Just Perfection, top bar) | Notifications, Date & Time | Add-ons | About.
Removed: Tweaks app, Windows, Startup, Session, Capture page, Extensions page.
Add-ons: only Caffeine, GPU Switcher, Lumen Connect. Each shows its options inline once enabled.
Themes: icon, cursor, GTK and window-border themes are detected by content (icon themes need icon dirs, cursor themes need a cursors folder).

## Bluetooth (own stack, no bluetoothd)
Linux only lets a program manage the adapter with CAP_NET_ADMIN, so a one-time password prompt (pkexec + setcap, package libcap) is unavoidable. Lumen now asks by itself the first time it needs it (once per installed binary), then restarts. Declined? Settings > Bluetooth > "Allow Bluetooth".
Keyboards/mice/gamepads work. No audio (A2DP) and no file transfer. If bluetoothd runs too, Settings warns: disable it.

## Lumen Connect (add-on)
Quick Settings tile > Mobile Devices > device actions; same list in Settings > Add-ons. Pairing asks via notification (Accept/Reject).
Open TCP+UDP 1714-1764; stop kdeconnectd/GSConnect; install sshfs for Browse Files.

## Tested
cargo test --release all pass (50 shell, 7 BT, 5+2 Connect, theme detection test). Xvfb: Add-ons, Bluetooth, pairing banner checked.
NOT tested: real XFCE session (Themes page shows "xfconf-query not found" without XFCE), real Bluetooth hardware, real phone, Quick Settings phone page visuals, touchpad/mouse apply.

## Round 10 changes
- Dock "Extend to the Edge" stops at the top bar (X11 struts and Wayland layer margins); bar never covered.
- Night Light: Quick Settings shows a temperature slider while on; turning off resets and removes the `[nightlight]` config section.
- Themes (XFCE): same keys as xfce4-settings (xsettings /Net/ThemeName, /Net/IconThemeName, /Gtk/CursorThemeName, /Xfce/SyncThemes -> xfwm4 /general/theme); shell recolors itself 0.6 s after a change; new "Window Borders Follow Theme" switch.
- Power: idle spawned `pgrep` x4 + xfconf-query every 3 s; now /proc scan, cached 60 s. Idle wake = once per minute (30 s max); volume/brightness polls only while Quick Settings is open. Measured under Xvfb: ~0.06 wakeups/s, 0 CPU ticks, 0 child processes, ~9.5 MB RSS.
- Removed: Plasma build (kde, kwin, KDE tweaks/power), lumen-wm detection.
- Fixed: screen-record FPS setting was unreachable (id clash with workspace count); notification system toggles 2508/2509 were shadowed by GSConnect handler.
## Tested
cargo test --release --workspace all pass (51 shell). Xvfb: left extended dock under bar, QS. NOT tested: real XFCE session, real Bluetooth, compositing transparency (needs xfwm4 compositing or picom).

## Round 11: transparency, xfwm4/xfwl4, security
Reference: xfwm4 `src/compositor.c` and xfwl4 (github.com/xfce-mirror).
- xfwm4 draws a rectangular shadow under every dock window, even ARGB ones (`show_dock_shadow`). Under a see-through or rounded bar it shows as a dark box. Dock & Bar > Transparency now has "Compositing (xfwm4)" and "Dock Shadow (xfwm4)" switches, and lowering an opacity slider turns the dock shadow off once.
- xfwm4 only composites ARGB windows with compositing on; without any compositor the page says so (run picom or use xfwm4 compositing).
- xfwl4 (Xfce's Wayland compositor, still in development) implements layer-shell, wlr-foreign-toplevel, ext-workspace and wlr-gamma-control, so lumen treats it like labwc: ARGB layer surfaces are alpha-blended by the compositor, Night Light works through gamma control, and window options use the xfwm4 xfconf channel. Detected as backend `xfwl4`. Not tested against a real xfwl4.
- xfwm4 option reads are one cached `xfconf-query -lv` instead of one process per row per redraw.

Security fixes (found by audit and fuzzing):
- Connect: unauthenticated LAN packets could start unlimited threads and each made lumen connect out; now capped (8 handshakes out, 32 in, 4 transfers). Slow identity senders are cut off after 5 s total. Received files: exclusive partial file with its own name (two transfers could clobber one `.lumen-part`), no overwriting, 32 GiB cap, mode 0600. runcommand children are reaped. Config folder is 0700.
- Shell injection: audio device names went into `sh -c` inside plain quotes; now properly quoted. Pointer script no longer uses a here-document that a device line could end.
- IPC socket: 0600, and without XDG_RUNTIME_DIR it lives in a private `/tmp/lumen-UID` directory (was a guessable shared /tmp path).
- Bluetooth permission: refuses to give a root-granted capability to a binary or folder that others can write to.
- New fuzz tests: Bluetooth parsers and the ATT client against a hostile peripheral, no panics. Whole workspace passes in debug (overflow checks) and release.

Quality: `cargo build --release` prints no warnings. Idle (Xvfb, 2 min): 0.1 wakeups/s, 0 CPU ticks, ~9.4 MB RSS, no child processes.

## Round 12: clicks dead across the whole desktop
Cause: on X11 every `Catcher` surface was sized to the whole output. The dock auto-hide / intelligent-hide trigger is a 2 px strip at the screen edge, built as a Catcher, so it became a permanent full-screen, input-eating window above xfwm4, the panel and every app. It also never saw the pointer arrive (no enter/motion events), so it could not reveal the dock.
Fix: a Catcher with a size is now exactly that size and gets enter/leave/motion events; only the popup click-away catcher (no size) covers the screen. Defence in depth: `open_popup` always closes the previous popup and its catcher first, and the main loop destroys any catcher left over without a popup.
Checked under Xvfb: strip is 1366x2 at the bottom edge, the pointer over the desktop hits the root window, and opening/closing Quick Settings repeatedly leaves no full-screen window.

## Round 13: settings, launcher, panel
- lumen never touches xfce4-panel any more: no stop, no restore, no guard process (`--guard` removed).
- Date & Time: the 24-hour / AM-PM dropdown was inverted; fixed.
- One place per setting: battery percentage and Hide Battery Icon live in Power; banner position in Notifications; hot corner in Dock & Bar. "Just Perfection" is now "Top Bar Elements", "Blur My Shell" is "Blur". The unreachable Top Bar page body was removed.
- Interface Scale is a dropdown. Window Borders is one dropdown: "Follow System Theme" (xfce4-settings sync) then installed themes.
- Display change: a separate popup in the middle of the screen with a 10 s countdown (xfce4-settings uses 10 s), Keep / Revert, Enter / Esc. Unanswered = reverted.
- No flicker: bar and dock are reconfigured in place (same windows) when settings change, and Settings stays open. Full rebuild only on output changes or when compositing changes.
- App grid: 8 columns x 4 rows (old saved 7 migrates to 8). Folders: Add to New Folder / Add to <folder> in the app menu, click a folder to open it, click its title to rename, Remove from Folder, Delete Folder (right click), Esc or empty space goes back. Empty folders disappear. Saved in config key `folders`.
- Lumen Connect: the Quick Settings tile and page are named "Lumen Connect". Device page now has Share (send text, send link), Plugins (show phone notifications, receive clipboard, per device) and Run Commands (list, add, remove; saved 0600 in commands.json). Existing: ping, find phone, send file, send clipboard, browse files, pair/unpair.
- Bluetooth: the "bluetoothd is also running" notice is replaced by one button, "Use Lumen Bluetooth", which stops the system service after a password prompt.

## Round 14
- Bluetooth now has a bluetoothd (BlueZ) backend (`lumen-bt/src/bluez.rs`, over the new shared `lumen-dbus` crate). When bluetoothd runs, lumen uses it: no permission prompt, no conflict notice. Otherwise the kernel path is used as before. Pairing agent included (confirm code prompt). Tested against a mock bluetoothd on a private bus (`lumen-bt/tests/bluez_mock.rs`, needs dbus-daemon, python3, dbus-next; skipped otherwise): power, scan, pair, connect, trust, disconnect, forget.
- Verified in Xvfb: AM/PM dropdown (bar shows `12:01 PM`, `24h = false` saved); Window Borders dropdown lists Follow System Theme then themes, and picking one sets SyncThemes false plus the xfwm4 theme.
- Drag an app onto another app (or a folder) to make or fill a folder; the sides still reorder. Compiles; NOT tested by drag in Xvfb.
- Still not done: Lumen Connect SMS, media and remote-input plugins; per-device outgoing clipboard.
- `lite-shell.tar.gz` is the rofi + polybar + waybar alternative, see its README. It measured heavier than lumen at idle.

## Also not Tested — Round 14
- Drag-to-folder: dragging an app onto another app to make a folder, onto a folder to fill it, or onto the sides to reorder. "Compiles; NOT tested by drag in Xvfb" — only the click-to-create folder flow (Round 13) was Xvfb-tested.
- Real NetworkManager scanning while the Wi-Fi page is open ("NOT tested: real NetworkManager scanning", Round 14).
- Lumen Connect SMS, media and remote-input plugins; per-device outgoing clipboard ("Still not done", Round 14).
- `lite-shell.tar.gz` (the rofi + polybar + waybar alternative) — only measured, not compared A/B.

## Test (round 14)
- Bluetooth BlueZ backend over the mock bluetoothd on a private bus (`lumen-bt/tests/bluez_mock.rs`; needs dbus-daemon, python3, dbus-next; skipped otherwise): power, scan, pair, connect, trust, disconnect, forget.
- Xvfb: AM/PM dropdown (bar shows `12:01 PM`, `24h = false` saved); Window Borders dropdown lists "Follow System Theme" then themes; picking a theme sets SyncThemes false plus the xfwm4 theme.

This README was written with AI assistance.

## Round 17 fixes
- Icons: multi-tone dark icons (mouse, trackpad, terminal) keep their inner detail on the dark dock instead of turning into a white blob; apps with no icon use the theme's own unknown-app icon (application-x-executable), letter tile only as last resort.
- Settings window leaves room for the dock, so the running Settings icon is never covered.
- Wi-Fi, Bluetooth and Lumen Connect scan only when the scan row is clicked; scans stop when the page is left.
- Window Frame option removed; "GNOME Style" is now an entry in Themes > Window Borders (sets window.frame=client).
- Lumen Connect: Name/Connect rows removed (name = hostname); Add Command commits the field being typed; lumen-connect.desktop + `--connect` so the window can be pinned.
- On exit the BlueZ backend stops scanning and disconnects connected devices; stale "connected" state is no longer restored from the cache.
- Bar status icons follow GNOME order (Caffeine, Night Light, network, Bluetooth, volume, battery), one list for horizontal and vertical bars.

## Tested
cargo test --release --workspace: all pass (84 shell). Xvfb + xfwm4: dock icons, Settings/Connect windows over the dock, Window Borders dropdown (GNOME Style saved). NOT tested: real Bluetooth, NetworkManager scanning, real phone, kill -9 cleanup.

## Round 18 fixes
- Keep Awake: no "no screensaver service answered" toast any more; without a screensaver service it quietly falls back to `xset` (X11).
- Launcher folders: right-click an app for "New Folder…" and "Add to <folder>" (inside an open folder: "Remove from Folder"); right-click a folder for Open / Rename… / Delete Folder. Dragging one app onto the centre of another still makes a folder.
- Settings page "Keyboard" is now "Keyboard & Mouse".
- Bluetooth settings: the "Bluetooth Stack" group (and its dead switching code) is gone; the backend is chosen automatically.
- Lumen Connect: until you edit a device's command list, phones are offered Lock Screen, Suspend, Log Out, Restart and Shut Down (XFCE session tools first, systemd/logind as fallback). Adding or removing a command saves the list as it stands.

## Tested (round 18)
cargo test --release --workspace: all pass (the Connect e2e test now expects the five default commands). Xvfb + xfwm4: folder create via menu ("Unnamed Folder" saved), Add to Folder entry, folder Open/Rename/Delete menu, sidebar label. NOT tested: a real phone running the commands, real Bluetooth, a real screensaver service.
