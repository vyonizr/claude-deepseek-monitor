# Context: Claude / Codex / DeepSeek Monitor

## Domain Language

| Term | Definition |
|------|------------|
| Usage source | A coding-agent service whose quota windows are monitored. Claude Code is the core usage source; Codex is an optional source that, when disabled, is neither polled nor displayed. _Avoid_: Provider, agent toggle. |
| Session window | Claude Code's per-session usage quota. Resets after the session lifetime (~hours). Shown in `/usage` as "Current session: X% used". |
| Weekly quota | Claude Code's rolling 7-day usage allowance. Resets on a fixed weekly schedule. Shown in `/usage` as "Current week (all models): X% used". |
| Codex rate-limit window | A Codex usage allowance reported with its percentage used, reset time, and duration. Every available primary and secondary window is displayed and paced independently; labels are derived from duration rather than assumed names. _Avoid_: Codex session, Codex weekly quota. |
| Pacing | Comparison of % of quota used vs % of time elapsed in the window. Classified as **Underusing** (diff < -under_threshold), **OnPace** (within the configured range), **Overusing** (diff > +over_threshold). Under and Over thresholds are independently configurable (1–20pp, default 1pp each). A special override: if `used_pct >= 100%` before the window ends, pacing is **Overusing** regardless of diff. Rust enum variants serialize to `"under"` / `"onpace"` / `"over"`; `dist/index.html` maps those to the display labels. |
| Source stale | State of one enabled usage source when its latest poll fails. That source retains its previous values dimmed and reports a source-specific diagnostic without affecting other sources. _Avoid_: Stale display, globally stale. |
| Awaiting session | State of the Claude *session* row specifically when `/usage`'s week line parses successfully but no line starting with `"Current session:"` is present at all (i.e. no session has been started since the last reset). Distinct from Source stale: the Claude source remains fresh, `session_used_pct` shows `0`, pacing is suppressed (no ON/UNDER/OVER PACE badge), and the reset-time text reads `"Not started"`. Any messier session-line failure (present but malformed) makes the Claude source stale instead. |
| DeepSeek peak window | One of two daily Beijing-time windows (09:00–12:00 and 14:00–18:00 BJT / 01:00–04:00 and 06:00–10:00 UTC) during which DeepSeek charges 2× standard rate. |
| Poll cycle | A pure function `(raw_usage_text: Option<&str>, current_time, config, previous_state) -> (new_state, notification_events)` — the single unit-testable seam. Accepts `None` for the subprocess-failure case. All app logic lives here. |
| Imperative shell | The Tauri application layer that spawns subprocesses, reads the clock, fires notifications, and renders the widget. Not unit-tested. |

## Files

| Path | Role |
|------|------|
| `src-tauri/src/poll_cycle.rs` | Pure poll-cycle function + 33 unit tests. The functional core. |
| `src-tauri/src/lib.rs` | Imperative shell: Tauri app setup, tray, polling loop, settings persistence, notifications. |
| `src-tauri/src/main.rs` | Binary entry point — calls `lib::run()`. |
| `dist/index.html` | Floating widget UI (vanilla HTML/JS, ~240×185px). |
| `dist/settings.html` | Settings panel (DeepSeek windows + auto-launch toggle). |
| `src-tauri/capabilities/default.json` | Tauri v2 permission capabilities. |

## How pacing is calculated

`compute_pacing(used_pct, elapsed_pct, under_threshold, over_threshold)` compares two percentages and buckets the `diff = used_pct - elapsed_pct` against the two thresholds:

- `diff < -under_threshold` → **Underusing**
- `diff > +over_threshold` → **Overusing**
- otherwise → **OnPace** (boundary values of exactly ±threshold are `OnPace`, since the comparison is strict `<`/`>`)

Both thresholds are user-configurable (1–20pp, default 1pp each) via the Settings panel, replacing the formerly hardcoded `PACING_THRESHOLD = 1.0`.

A special override: if `used_pct >= 100.0` and `elapsed_pct < 100.0`, pacing is **Overusing** — the quota is exhausted before the window ends, so you're definitively behind pace regardless of the diff. This override is unchanged by configurable thresholds.

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
| ADR-004 | Configurable pacing thresholds (1–20pp), no percentage prefix on labels | Users configure their own sensitivity via the Settings panel (defaults 1pp each, matching the original hardcoded threshold). The 100%-override guard and the no-percentage-clutter-in-labels decision are unchanged. |
| ADR-005 | Beijing time = UTC+8 hardcoded | No DST, no timezone database dependency needed. |
| ADR-006 | Local issue tracker (markdown files) | No GitHub/Linear configured for this project. |
| ADR-007 | Detect "Awaiting session" structurally, not by string-matching `/usage`'s exact wording | The dimmed Stale overlay was firing whenever a session reset passed without the user starting a new one, making the widget look unresponsive even though it was polling correctly. The exact replacement text `/usage` prints in that state is unconfirmed, so matching on a specific phrase would be brittle and could silently break if Claude Code rewords it. Instead, the parser was split so session/week lines parse independently; "week parses, no `Current session:` line at all" is the narrow trigger for Awaiting session, while any other session-parse failure still falls back to Stale. |
| ADR-008 | Window position persisted via `tauri-plugin-window-state` (position only), with an off-screen safety check in `poll_cycle.rs` | Widget has no title bar/taskbar entry to manually reposition if it restores off-screen (e.g. after a monitor is unplugged); falls back to top-right corner of primary monitor when the restored rect doesn't overlap any current monitor. |

## Final source-local widget boundary

The widget always orders enabled sources as Claude, Codex, then DeepSeek. Codex is optional and its enabled/loading/available/unavailable/source-stale state is serialized independently from Claude freshness. A Codex source failure dims only retained Codex values and leaves Claude and DeepSeek visually live.

The native shell owns widget geometry. The frontend requests a target height when Codex is enabled or disabled; the shell preserves the current position when the new rectangle fits and clamps the complete rectangle to the monitor otherwise. Existing window identifiers, app-data settings storage, and persisted position behavior remain unchanged.

All planned features implemented — graceful poll-thread shutdown (channel-based, with bounded confirmation), exponential backoff on repeated Claude subprocess failures (capped at 8× base interval), configurable poll interval (re-read from settings each cycle, no restart needed), "Awaiting session" state for clean session-line absence, source-local Codex diagnostics and stale retention, and adaptive widget geometry.
