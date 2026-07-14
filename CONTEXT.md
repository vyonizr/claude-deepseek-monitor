# Context: Claude DeepSeek Monitor

## Domain Language

| Term | Definition |
|------|------------|
| Session window | Claude Code's per-session usage quota. Resets after the session lifetime (~hours). Shown in `/usage` as "Current session: X% used". |
| Weekly quota | Claude Code's rolling 7-day usage allowance. Resets on a fixed weekly schedule. Shown in `/usage` as "Current week (all models): X% used". |
| Pacing | Comparison of % of quota used vs % of time elapsed in the window. Classified as **Underusing** (ahead of pace, diff < -10pp, shown as `"UNDER PACE"`), **OnPace** (within ±10pp, shown as `"ON PACE"`), **Overusing** (behind pace, diff > +10pp, shown as `"OVER PACE"`). Rust enum variants serialize to `"under"` / `"onpace"` / `"over"`; `dist/index.html` maps those to the display labels. |
| Stale | State of the display when the last poll failed to parse `/usage` output. Previous values shown dimmed. |
| DeepSeek peak window | One of two daily Beijing-time windows (09:00–12:00 and 14:00–18:00 BJT / 01:00–04:00 and 06:00–10:00 UTC) during which DeepSeek charges 2× standard rate. |
| Poll cycle | A pure function `(raw_usage_text, current_time, config, previous_state) -> (new_state, notification_events)` — the single unit-testable seam. All app logic lives here. |
| Imperative shell | The Tauri application layer that spawns subprocesses, reads the clock, fires notifications, and renders the widget. Not unit-tested. |

## Files

| Path | Role |
|------|------|
| `src-tauri/src/poll_cycle.rs` | Pure poll-cycle function + 29 unit tests. The functional core. |
| `src-tauri/src/lib.rs` | Imperative shell: Tauri app setup, tray, polling loop, settings persistence, notifications. |
| `src-tauri/src/main.rs` | Binary entry point — calls `lib::run()`. |
| `dist/index.html` | Floating widget UI (vanilla HTML/JS, ~240×185px). |
| `dist/settings.html` | Settings panel (DeepSeek windows + auto-launch toggle). |
| `src-tauri/capabilities/default.json` | Tauri v2 permission capabilities. |

## How pacing is calculated

`compute_pacing(used_pct, elapsed_pct)` compares two percentages and buckets the `diff = used_pct - elapsed_pct` against `PACING_THRESHOLD` (currently `10.0`):

- `diff < -10` → **Underusing** (using quota slower than time is passing)
- `diff > +10` → **Overusing**
- otherwise → **OnPace** (boundary values of exactly ±10 are `OnPace`, since the comparison is strict `<`/`>`)

Only the label is displayed — the percentage diff is not shown in the UI.

`used_pct` comes straight from the parsed `/usage` text. `elapsed_pct` is derived differently per window:

- **Session window**: `elapsed_pct = (current_time - session_window_start) / (session_reset_dt - session_window_start) * 100`. `session_window_start` isn't reported by `/usage` — it's inferred as the first poll where the session's reset-time text changes from the previous poll (i.e. a new session started), and persisted in `DisplayState` across cycles.
- **Week window**: `/usage` only reports the reset datetime, so the window start is assumed to be `week_reset_dt - 7 days`. `elapsed_pct` is clamped to `[0, 100]`.

If the window hasn't actually started yet (`start >= reset`) or its duration is zero/negative, pacing falls back to the previous cycle's value rather than being recomputed.

## Architecture

**Functional core, imperative shell.** One pure `poll_cycle()` function is the sole testing seam. The imperative shell (Tauri) is responsible for all I/O: subprocess, clock, notifications, UI rendering, settings persistence.

## Decisions

| ID | Decision | Rationale |
|----|----------|-----------|
| ADR-001 | Tauri v2 (not Electron) for desktop shell | Smaller binary, lower idle memory, native tray and always-on-top support. |
| ADR-002 | Single pure function as test seam | Avoids mocking subprocess/clock/notifications. Tests supply inputs, assert outputs. |
| ADR-003 | User-configurable poll interval (default 5 min, min 1) | Lets users trade CLI spawn frequency for freshness; poll thread re-reads the interval from settings each cycle so changes apply without restart. |
| ADR-004 | ±10pp pacing threshold, no percentage prefix on labels | Threshold at ±10pp catches attention early without being abrupt. The percentage diff was removed to reduce visual clutter — just "OVER PACE" / "UNDER PACE". |
| ADR-005 | Beijing time = UTC+8 hardcoded | No DST, no timezone database dependency needed. |
| ADR-006 | Local issue tracker (markdown files) | No GitHub/Linear configured for this project. |

## State

Initial implementation complete (2 commits). Tickets 01–05 implemented. Next: graceful shutdown, poll backoff, edge cases.
