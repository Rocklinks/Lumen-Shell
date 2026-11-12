# lumen v0.3.0

A lightweight shell for XFCE with a GNOME Shell look and feel, built from scratch.

`lumen` is a single-binary shell designed to run on top of XFCE, delivering a modern GNOME Shell–style interface without the weight of a full desktop environment. It is built from the ground up in Rust (std-only) and links `lumen-shell`, `lumen-bt`, and `lumen-connect` into one `lumen` binary.

## Build

```bash
cargo build --release   # -> target/release/lumen
# Plasma variant:
cargo build --release --no-default-features --features plasma
```

## Bluetooth (lumen-bt)

No `bluetoothd`, no `bluetoothctl`. Settings > Bluetooth and the Quick Settings tile talk to the controller directly:
- power, scan, pair (confirmation code shown as a notification), connect, disconnect, forget, battery, trusted
- BLE and classic keyboards, mice, gamepads
- Bonds saved in `~/.local/share/lumen/bluetooth`

Not supported: audio (A2DP/HFP), OBEX file transfer.

Permission (once):
```bash
sudo setcap cap_net_admin,cap_net_raw,cap_net_bind_service+eip $(readlink -f $(which lumen))
```
(Settings shows this line when permission is missing.) HID needs `/dev/uhid` access (udev `KERNEL=="uhid", TAG+="uaccess"`).

## LumenConnect (lumen-connect, Addon)

Enable in Tweaks > Extensions. Starts the KDE Connect v8 service inside lumen; a tile appears in Quick Settings.
- Pair from phone or device page; pairing question arrives as a notification (Accept / Reject)
- Phone notifications, calls, SMS alerts, files (both ways), text/links, clipboard sync, ping, find my phone, battery, browse files (needs sshfs)
- Stop `kdeconnectd`/GSConnect first. Open TCP+UDP 1714–1764.

Not done: SMS send, media controls, remote mouse.

## Tested

- `cargo test --release`: all tests pass (shell, Bluetooth mock controller, Connect end-to-end)
- Runs under Xvfb: shell starts, GSConnect binds 1716, standalone peer pairs, verification-code banner shows, Bluetooth page renders

Not tested: real Bluetooth hardware, real phone, real XFCE session.
