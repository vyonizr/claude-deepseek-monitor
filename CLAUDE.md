# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Tauri v2 desktop app (Rust backend + vanilla HTML/JS frontend): a 240×185px always-on-top overlay showing Claude Code usage pacing and DeepSeek peak-pricing windows. See `README.md` for features/prerequisites and `CONTEXT.md` for domain vocabulary and architectural decisions (ADR-001..007) — read both before making non-trivial changes.

## Commands

All commands run from `src-tauri/`:

```bash
cargo run              # build and launch the app
cargo test              # run poll_cycle unit tests (33 tests, functional core only)
cargo test <test_name>  # run a single test
cargo build --release   # release binary -> src-tauri/target/release/claude-deepseek-monitor.exe
```

There is no separate frontend build step or package.json — `dist/` is plain HTML/JS/CSS served directly by Tauri (`frontendDist` in `tauri.conf.json`).

## Architecture: functional core, imperative shell

All business logic lives in one pure function in `src-tauri/src/poll_cycle.rs`:

```
poll_cycle(raw_usage_text, current_time, config, previous_state) -> (new_display_state, notification_events)
```

- `src-tauri/src/poll_cycle.rs` — the pure core: types (`Pacing`, `DeepSeekStatus`, `Config`, `DisplayState`, `NotificationEvent`) and the `poll_cycle` function itself, plus all 33 unit tests. No I/O. This is where quota-pacing math, DeepSeek window logic, and staleness handling live, and where new logic/tests should go.
- `src-tauri/src/lib.rs` — the imperative shell: Tauri app setup, system tray, settings load/save (`settings.json` in the OS app data dir), spawning `claude --print "/usage"` as a subprocess, firing OS notifications, emitting state to the frontend, and the `#[tauri::command]` handlers (`get_initial_state`, `get_settings`, `save_settings`) invoked from JS.
- `src-tauri/src/main.rs` — trivial entry point, calls `lib::run()`.
- `dist/index.html` — the floating widget UI.
- `dist/settings.html` — the settings panel (DeepSeek window editing, auto-launch toggle).

When changing behavior (pacing thresholds, DeepSeek windows, staleness rules, notification triggering), prefer editing `poll_cycle.rs` and covering it with a unit test there, rather than adding logic to `lib.rs`. `lib.rs` should stay limited to I/O plumbing that calls into `poll_cycle()`.

Poll interval, DeepSeek windows, and auto-launch are all user-configurable via the settings panel, persisted to `settings.json`. The poll thread re-reads the interval from settings before each sleep, so a changed interval takes effect on the next cycle without an app restart.

## Testing notes

Only `poll_cycle()` is unit-tested. The imperative shell (`lib.rs`) is verified manually by running the app — there is no automated test harness for tray/subprocess/notification behavior.

## Agent skills

### Issue tracker

Local markdown issues under `.scratch/<feature>/issues/`, gitignored. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` at repo root (no separate `docs/adr/`; ADRs are embedded in its Decisions table). See `docs/agents/domain.md`.
