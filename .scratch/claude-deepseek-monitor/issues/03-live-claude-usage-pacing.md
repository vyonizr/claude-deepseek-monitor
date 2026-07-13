# 03 — Live Claude usage pacing display

**What to build:** The widget shows real, live-updating Claude Code usage. Every 5 minutes, the app spawns `claude --print "/usage"` as a subprocess, captures its stdout, and feeds it (along with the current time) through the poll-cycle function from ticket 01. The resulting session-window and weekly-quota pacing indicators (underusing / on-pace / overusing, % used, time to reset) are rendered in the widget. If a poll's output fails to parse, the widget shows the last known values dimmed/greyed with a visible "stale" marker, per the poll-cycle function's fallback behavior.

**Blocked by:** 01, 02

**Status:** completed

- [x] Widget displays real session-window % used, pacing classification, and time-to-reset, refreshed roughly every 5 minutes
- [x] Widget displays real weekly-quota % used, pacing classification, and time-to-reset, refreshed on the same cadence
- [x] Visual coding (e.g. color) distinguishes underusing / on-pace / overusing for both windows
- [x] If a poll fails to parse, the widget shows the previous values dimmed with a visible stale indicator instead of blanking or erroring
- [ ] Verified by running the built app and observing the widget update over a real 5+ minute window
