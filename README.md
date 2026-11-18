# Lumen Shell v0.6.0 

Binary: `lumen` (release, stripped). Source: `lumen-src.tar.gz` (`cargo build --release`, no extra crates).

<div align="center">

![Lumen Shell demo](lumen-thumb-play.jpg)

</div>

## What changed
- **Lumen Connect > Run Command**: the phone's "add command" button now opens that phone's page (it used to only open the
  device list). Name and Command fields keep what you typed when you move between them, and there is an **Add Command**
  button. The command list is pushed to the phone on every add/remove, as before.
- **Lumen Connect pairing view**: a paired device never shows a leftover "Waiting for the other device… / Cancel" row.
- **Window drawing (all shell windows)**: frames are built in an off-screen pixmap and copied to the window in one request,
  so a compositor (xfwm4) can no longer show a half-updated window while the content is being sent.
- **Tray**: Lumen queues for `org.kde.StatusNotifierWatcher`. When another tray host that owned it exits, Lumen takes it
  over at once and announces itself, so apps started afterwards show their icons here.
- **Launcher**: the search box can be clicked (it used to close the launcher). Type `12*(3+4)`, `10/4`, `2^8`, `7%3`,
  `6×7`: the answer shows under the box, Enter or a click copies it. Ctrl+V pastes into the search.
- **Dock**: the trash stays next to the last app when "extend to edge" is on; only the launcher goes to the far end.
  New Dock & Bar options: **Indicator Position** (Automatic / Bottom / Top / Left / Right) and **Indicator Style**
  (Dot / Square / Dash). Dock colours follow the Color Scheme (light and dark).
- **Settings on the dock**: an open Settings window lights up its pinned icon; Lumen Connect gets a temporary icon.
  Icons come from the icon theme, with the generic fallback.
- **GPU switcher**: the Quick Settings tile appears only when an NVIDIA card is present (PCI vendor 0x10de). No NVIDIA:
  no tile, no GPU jobs.
- **Settings sidebar** uses the GTK theme's `sidebar_bg_color` when the theme defines one, else a tint of the window colour.
- **Removed**: Activities hot corner (code, setting and capability), the whole "Top Bar Elements" group, "Top bar follows
  the theme" (the bar and dock always follow the Color Scheme), and the Notifications "Service" group. Old config keys
  are ignored. "Hide Battery Icon" stays on the Power page.


##how to Run this
Just Download the lumen binary file, make it executable and move it the .local/bin

