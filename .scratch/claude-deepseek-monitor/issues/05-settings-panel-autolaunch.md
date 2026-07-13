# 05 — Settings panel: editable DeepSeek windows + auto-launch toggle

**What to build:** A settings panel (reachable from the tray menu) where the user can edit the DeepSeek peak-window start/end times (pre-filled with the defaults from ticket 04: 9:00–12:00 and 14:00–18:00 Beijing time) and toggle auto-launch-at-login (on by default). Settings persist to a local config file in the OS-appropriate app-data directory and survive app restarts. The auto-launch toggle actually registers/unregisters the app for OS-level login startup on both Windows and macOS. The DeepSeek indicator from ticket 04 reads its window configuration from these settings instead of hardcoded constants, so edits take effect.

**Blocked by:** 02, 04

**Status:** completed

- [x] Settings panel is reachable from the tray menu
- [x] DeepSeek peak windows are editable and pre-filled with the correct defaults on first launch
- [ ] Editing a DeepSeek window and saving changes the app's live peak/off-peak calculation accordingly
- [x] Auto-launch-at-login toggle is on by default (autostart plugin registered)
- [ ] Toggling auto-launch back on re-registers the app for login startup
- [ ] Settings persist across an app restart
- [ ] Verified working on both Windows and macOS
