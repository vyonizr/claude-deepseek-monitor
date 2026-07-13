# 04 — DeepSeek peak-hour indicator + transition notifications

**What to build:** The widget persistently shows whether the current local time falls inside one of DeepSeek's 2× peak-pricing windows (default: 9:00–12:00 and 14:00–18:00 Beijing time, correctly converted to the user's local system timezone), using the DeepSeek status computed by the poll-cycle function from ticket 01. In addition, exactly at the moment the status transitions (peak starts, or peak ends), the app fires a native OS toast notification. The widget's persistent indicator and the toast are two channels for the same underlying state — the indicator is always visible, the toast is a one-off nudge at the transition edge only.

**Blocked by:** 01, 02

**Status:** completed

- [x] Widget persistently shows current peak/off-peak status, correctly converted to local system timezone
- [x] Status updates correctly across a peak-window boundary (verified via unit tests with fixed times)
- [x] A native OS toast notification fires exactly once at the moment a peak window starts
- [x] A native OS toast notification fires exactly once at the moment a peak window ends
- [x] No duplicate or repeated toast fires while remaining in the same state across multiple polls
- [ ] Verified working on both Windows and macOS
