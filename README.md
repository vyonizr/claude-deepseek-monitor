# Claude / Codex / DeepSeek Monitor

A compact desktop overlay that shows Claude Code and optional Codex usage pacing alongside DeepSeek peak pricing windows.

**240×185px floating widget** → always-on-top, draggable, system tray icon. Enabling Codex expands it vertically without hiding the DeepSeek section. Refresh interval is configurable (default 5 minutes).

![widget preview](docs/widget-preview.png)

## Features

- **Claude Code session usage** — % used, time to reset, pacing ("UNDER PACE" / "ON PACE" / "OVER PACE")
- **Claude Code weekly quota** — % used, time to reset, pacing
- **Optional Codex rate limits** — authenticated through the local Codex CLI; displays every available primary and secondary window, reset time, and pace
- **DeepSeek peak pricing** — persistently shows peak/off-peak status with time to next transition
- **OS notifications** — fired exactly when a DeepSeek peak window starts or ends (edge-triggered, not every poll)
- **Source-local failure state** — a failed source retains its last values dimmed and reports a concise diagnostic without dimming the other sources
- **Settings panel** — editable DeepSeek windows (Beijing time), refresh interval, pacing thresholds, opacity, Codex toggle, and auto-launch
- **Tray icon** — click to show/hide widget, right-click for Settings and Quit

## Prerequisites

- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code/overview) — must be installed and authenticated (`claude --print "/usage"` must work)
- [Codex CLI](https://github.com/openai/codex) — optional; enable it only after the local CLI is installed and authenticated (`codex login`). Credentials remain owned by the CLI and are never stored by this monitor.
- Windows or macOS

### Codex setup

Codex monitoring is off by default. Install and authenticate the Codex CLI, then enable **Enable Codex** in Settings. The widget polls a short-lived local app-server exchange on the shared refresh cycle. It displays all available primary and secondary rate-limit windows; pacing is based on percentage used versus percentage of time elapsed when reset duration metadata is available. If Codex is unavailable, logged out, outdated, or times out, its section explains the failure and retains prior values when possible while Claude and DeepSeek remain live.

## Development

```bash
# Build and run
cd src-tauri
cargo run

# Run unit tests (poll_cycle function)
cargo test
```

### Project structure

```
src-tauri/src/
├── main.rs              # Binary entry point
├── lib.rs               # Tauri app shell: tray, polling, settings, notifications
└── poll_cycle.rs        # Pure poll-cycle function + 33 unit tests (functional core)

dist/
├── index.html           # Floating widget UI
└── settings.html        # Settings panel
```

### Architecture

**Functional core, imperative shell.**

All business logic lives in a single pure function `poll_cycle()`:

```
(raw_usage_text, current_time, config, previous_state)
    → (new_display_state, notification_events)
```

The Tauri shell handles all I/O: spawning `claude --print "/usage"`, reading the clock, firing OS notifications, rendering the widget, persisting settings.

### Design decisions

| Decision | Choice |
|----------|--------|
| Framework | [Tauri v2](https://v2.tauri.app) (Rust backend, web frontend) |
| Poll interval | User-configurable (default 5 minutes, min 1) |
| Pacing threshold | ±1 percentage point around even pace (label only, no percentage); 100%-used override forces Overusing |
| DeepSeek windows | 09:00–12:00 and 14:00–18:00 Beijing time (UTC+8, no DST) |
| Settings storage | `settings.json` in OS app data directory |
| Auto-launch | `tauri-plugin-autostart` (on by default) |
| Widget resizing | Native resize command preserves the current top edge/position when possible and clamps the complete widget to a monitor |

### Testing

The pure `poll_cycle()` function and serialization/window-geometry seams are unit-tested. The imperative shell is verified manually, including Codex state transitions and multi-monitor placement.

## Building

```bash
cd src-tauri
cargo build --release
```

The built binary will be at `src-tauri/target/release/claude-deepseek-monitor.exe`.

## License

MIT
