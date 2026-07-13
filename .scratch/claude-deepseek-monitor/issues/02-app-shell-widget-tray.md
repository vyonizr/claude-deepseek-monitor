# 02 — App shell: floating widget + tray icon

**What to build:** The Tauri application shell that later tickets will populate with real data. On launch, the app shows a small (~220×140px), borderless, always-on-top widget window with placeholder content, which the user can drag to reposition anywhere on screen. A system tray icon is also present: clicking it toggles the widget's visibility (show/hide), and right-clicking it offers a menu with at least a Quit item (a Settings item placeholder is fine here — ticket 5 wires it up). The app runs on both Windows and macOS.

**Blocked by:** None — can start immediately

**Status:** completed

- [x] Launching the app shows a small, borderless, always-on-top widget window with placeholder content
- [x] The widget window can be dragged to any position on screen
- [x] A system tray icon appears on launch
- [x] Clicking the tray icon toggles the widget's visibility (show/hide)
- [x] Right-clicking the tray icon shows a menu with a working Quit item that cleanly exits the app
- [x] Settings menu item opens a settings window
- [ ] Verified working on both Windows and macOS
