# lumen: one binary, one Settings

Workspace: `lumen-shell` (the shell) + `lumen-bt` + `lumen-connect`, all std-only, linked into ONE binary `lumen`.
Build: `cargo build --release` -> `target/release/lumen` (add `--no-default-features --features plasma` for the Plasma build).

## Bluetooth (lumen-bt, no bluetoothd, no bluetoothctl)
Settings > Bluetooth and the Quick Settings tile use it directly: power, scan, pair (confirm code shown as a notification), connect,
disconnect, forget, battery, trusted. BLE and classic keyboards/mice/gamepads. Bonds saved in ~/.local/share/lumen/bluetooth.
Not supported: audio (A2DP/HFP), OBEX file transfer.
Needs permission once: `sudo setcap cap_net_admin,cap_net_raw,cap_net_bind_service+eip $(readlink -f $(which lumen))`
(Settings shows this line when permission is missing). HID needs /dev/uhid access (udev `KERNEL=="uhid", TAG+="uaccess"`).

## GSConnect (lumen-connect, extension "GSConnect")
Turn on in Tweaks > Extensions: starts the KDE Connect v8 service inside lumen; tile appears in Quick Settings.
Pair from phone or from the device page; pairing question arrives as a notification (Accept / Reject).
Phone notifications, calls, SMS alerts, files (both ways), text/links, clipboard sync, ping, find my phone, battery, browse files (needs sshfs).
Stop kdeconnectd/GSConnect first. Open TCP+UDP 1714-1764. Not done: SMS send, media controls, remote mouse.

## Tested
cargo test --release: 48 shell + 7 Bluetooth (mock controller) + 5 + 2 end-to-end Connect tests, all pass.
Run under Xvfb: shell starts, GSConnect binds 1716, standalone peer pairs, banner "Pair with ... Verification code" shown, Bluetooth page renders.
NOT tested: real Bluetooth hardware, real phone, real XFCE session.
