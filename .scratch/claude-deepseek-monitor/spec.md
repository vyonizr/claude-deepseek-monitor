---
title: Claude Code / DeepSeek Compact Usage Overlay
label: ready-for-agent
status: ready
---

## Problem Statement

The user runs Claude Code (billed via session-window and weekly quotas) and also runs DeepSeek models through OpenCode, where DeepSeek charges 2× the standard rate during two daily windows (9:00–12:00 and 14:00–18:00 Beijing time / 01:00–04:00 and 06:00–10:00 UTC).

Today, finding out whether they are under- or over-using their Claude quota requires manually running the `/usage` slash command inside a Claude Code session every time — there is no passive, at-a-glance way to see this. Separately, there is no way to know, without doing the timezone math in their head, whether the current moment falls inside one of DeepSeek's 2× peak-pricing windows before firing off a DeepSeek request.

## Solution

A small, cross-platform (Windows + macOS) desktop overlay app that:

1. Periodically checks Claude Code's session-window and weekly quota usage (by automating the existing `/usage` command) and shows, at a glance, whether the user is pacing under or over what the elapsed time in each window would suggest.
2. Continuously shows whether the current local time falls within one of DeepSeek's 2× peak-pricing windows, converted correctly to the user's local timezone, and proactively notifies at the moment a peak window starts or ends.

The app is a small always-on-top floating widget plus a system tray icon, so the information is visible without the user needing to actively go look for it.

## User Stories

1. As a Claude Code user, I want to see my current session-window usage percentage without running `/usage`, so that I don't have to interrupt my work to check.
2. As a Claude Code user, I want to see my current weekly quota usage percentage without running `/usage`, so that I can plan longer-running work across the week.
3. As a Claude Code user, I want to know how much time is left before my session window resets, so that I know when my usage allowance refreshes.
4. As a Claude Code user, I want to know how much time is left before my weekly quota resets, so that I can plan my week's work accordingly.
5. As a Claude Code user, I want the app to tell me whether I'm "underusing" or "overusing" relative to how much of the current window has elapsed, so that I don't have to do the percentage-vs-time-elapsed math myself.
6. As a Claude Code user, I want this pacing indication for both the session window and the weekly window independently, so that I can distinguish a short-term burst from a longer-term trend.
7. As a Claude Code user, I want the usage numbers refreshed automatically in the background, so that the display stays current without manual polling.
8. As a Claude Code user, if the app can't parse the latest `/usage` output (e.g. because Claude Code changed its output format), I want to still see the last known values (clearly marked as stale) rather than a blank or crashed display, so that the widget stays useful even if a poll fails.
9. As a DeepSeek/OpenCode user, I want to see at a glance whether the current moment is inside one of DeepSeek's 2× peak-pricing windows, so that I can decide whether to delay a request.
10. As a DeepSeek/OpenCode user, I want the peak/off-peak windows (given to me in Beijing time and UTC) correctly converted to my local system timezone, so that I don't have to do the timezone math myself.
11. As a DeepSeek/OpenCode user, I want to be proactively notified via an OS-level notification exactly when a peak window starts, so that I know immediately if my rate just doubled.
12. As a DeepSeek/OpenCode user, I want to be proactively notified via an OS-level notification exactly when a peak window ends, so that I know immediately when standard pricing resumes.
13. As a DeepSeek/OpenCode user, I want the peak/off-peak state to also be visible persistently in the widget (not just as a one-off notification), so that I can check the current state at any later glance even if I missed the notification.
14. As a user who values screen real estate, I want the app to be a small, compact overlay rather than a full window, so that it doesn't get in the way of my other work.
15. As a user, I want the overlay to be draggable so I can position it wherever is convenient on my screen.
16. As a user, I want a system tray icon so that I can show/hide the widget or access settings/quit without the widget always being in the way.
17. As a user, I want the DeepSeek peak-hour windows to be editable in a settings panel (not just hardcoded), so that if DeepSeek changes its pricing schedule, I can update the app myself without waiting for a new release.
18. As a user, I want sensible hardcoded defaults for the DeepSeek peak windows (9:00–12:00 and 14:00–18:00 Beijing time) pre-filled in settings, so that the app works correctly out of the box without any configuration.
19. As a user, I want the app to launch automatically at system login, so that I don't have to remember to start it myself every session/reboot.
20. As a user, I want to be able to disable auto-launch-at-login in settings, so that I retain control if I don't want it starting automatically in the future.
21. As a user on Windows, I want the app to work correctly, since that is one of my two daily-driver OSes.
22. As a user on macOS, I want the app to work correctly, since that is my other daily-driver OS.
23. As a user, I want the background poll of `/usage` to happen roughly every 5 minutes, so that the display stays reasonably current without excessive CLI process spawning.
24. As a user, I want the widget's visual state (e.g. color coding) to distinguish "underusing," "on pace," "overusing" for Claude quotas, and "peak" vs "off-peak" for DeepSeek, so that I can interpret the state without reading exact numbers.

## Implementation Decisions

- **Architecture pattern:** functional core, imperative shell. A single pure "poll-cycle" function is the one seam for testing: `(raw /usage CLI text, current time, config, previous display state) → (new display state, notification events to fire)`. This pure function is responsible for:
  - Parsing the raw text output of `claude --print "/usage"` into structured data: session-used-%, session-reset-time, week-used-%, week-reset-time.
  - Computing the pacing indicator for both the session window and the weekly window, by comparing % of quota used against % of the window's total duration elapsed (window start → reset time), and classifying the result as underusing / on-pace / overusing (banding thresholds are an implementation detail left to the builder, e.g. a small tolerance band around "even pace").
  - Computing DeepSeek peak/off-peak status by comparing current time (converted from the app's configured windows, given in Beijing time, to whatever timezone comparison is needed) against the configured window list.
  - Diffing the previous display state against the new one to decide whether a DeepSeek peak-hour transition notification event should fire (edge-triggered: fire only on state change, not on every poll).
  - Handling the "can't parse `/usage` output" case by falling back to the previous display state's values, marked stale, rather than raising or blanking.

- **Imperative shell** (untested by unit tests, verified via manual/integration testing) is responsible for:
  - Spawning `claude --print "/usage"` as a subprocess every 5 minutes and capturing stdout as the raw text input to the poll-cycle function.
  - Reading the system clock and passing it into the poll-cycle function.
  - Persisting the previous display state between polls (in memory is sufficient; does not need to survive app restarts).
  - Rendering the floating widget UI from the current display state.
  - Firing native OS toast notifications for any notification events the poll-cycle function emits.
  - Registering/unregistering OS-level auto-launch-at-login, driven by a settings toggle.
  - Persisting user settings (DeepSeek window times, auto-launch toggle, widget position) to a local config file in the OS-appropriate app-data directory.

- **Tech stack:** Tauri (Rust backend, web frontend) chosen for small binary size/low idle memory versus Electron, and for first-class always-on-top/tray support versus needing separate native codebases per OS.

- **Platforms:** Windows and macOS. No Linux support in this scope (not excluded from the architecture, just not a target for this build).

- **UI shape:** A small (~220×140px), borderless, always-on-top, draggable floating widget showing:
  - Claude session-window pacing bar/indicator + % used + time to reset.
  - Claude weekly-quota pacing bar/indicator + % used + time to reset.
  - DeepSeek peak/off-peak indicator (current state, and ideally time remaining until the next transition).
  - A "stale" visual marker (e.g. dimmed/greyed) when the last poll's parse failed and displayed values are held over from the previous successful poll.
  
  Plus a system tray icon that: toggles widget show/hide on click, and offers a right-click menu with at least Settings and Quit.

- **Settings panel** contains, at minimum:
  - Editable DeepSeek peak windows (start/end time pairs), pre-filled with the defaults 9:00–12:00 and 14:00–18:00 Beijing time.
  - Auto-launch-at-login toggle (on by default).

- **Polling interval:** 5 minutes, fixed (not required to be user-configurable in this scope).

- **Data source for Claude usage:** `claude --print "/usage"` subprocess call. Confirmed working during spec discovery — sample raw output format:
  ```
  Current session: 8% used · resets Jul 13, 8:40pm (Asia/Jakarta)
  Current week (all models): 13% used · resets Jul 18, 4am (Asia/Jakarta)
  ```
  Note the reset timestamps are already rendered by the CLI in the local system timezone, which the parser should account for.

- **Notification delivery:** dual-channel — persistent visual state in the widget (always current) AND a native OS toast notification fired only at the moment of a DeepSeek peak-window transition (start or end), not on every poll.

## Testing Decisions

- The core unit-testing target is the single pure poll-cycle function described above. Good tests here supply raw `/usage` text samples (including malformed/unexpected ones), a fixed "current time," a config (window list), and a previous display state, then assert on the resulting display state and emitted notification events — testing external behavior (inputs → outputs) rather than internal parsing implementation details.
- Cases to cover: normal parse success; malformed/unparseable `/usage` output (asserting fallback-to-stale behavior); pacing classification at various time-elapsed vs quota-used ratios (underusing, on-pace, overusing, and boundary cases); DeepSeek status at times just inside/outside/on the boundary of a peak window; notification event firing exactly on state transition and not firing on repeated polls within the same state.
- The imperative shell (subprocess spawning, clock reads, OS notification firing, settings persistence, UI rendering, auto-launch registration) is not a target for unit tests; it should be verified manually by running the built app (start it, observe the widget updates on a real timer, verify a real OS toast appears at a real or simulated transition, verify tray icon behavior) since there is no existing test-runner/harness in this new project to establish prior art from.
- No prior art exists in this codebase (greenfield project) to reference for test conventions; the builder should follow standard Rust unit-testing conventions (`#[cfg(test)]` modules) for the pure poll-cycle function.

## Out of Scope

- Linux support.
- User-configurable polling interval.
- Historical usage charts/trends beyond the current pacing snapshot (the existing `stats-cache.json` daily history is not surfaced in this app).
- Reverse-engineering any private Anthropic API — usage data comes exclusively from automating the public `/usage` CLI command.
- Support for tracking usage across multiple machines/devices (the `/usage` command itself notes it only reflects local sessions on the current machine).
- Sound-based notifications (only visual widget state + OS toast, no audio).
- Any DeepSeek pricing windows or providers other than the two specified daily Beijing-time windows.

## Further Notes

- This spec was produced from a `/grill-me` interview (no existing codebase/repo at spec time — greenfield project at `E:\programming\projects\claude-deepseek-monitor`).
- No issue tracker was configured for this project (`/setup-matt-pocock-skills` has not been run), so this spec is stored locally at `.scratch/claude-deepseek-monitor/spec.md` rather than published to an external tracker. It carries the `ready-for-agent` label in its frontmatter for when `/to-tickets` or `/implement` picks it up.
- The pacing-classification thresholds (what counts as "underusing" vs "on-pace" vs "overusing") were intentionally left as an implementation detail rather than pinned to exact numeric bands, since the user did not specify exact tolerances during grilling — the builder should pick a reasonable default (e.g. ±10 percentage points around even pace) and note the choice.
