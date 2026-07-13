# 01 — Poll-cycle core logic

**What to build:** The single pure "poll-cycle" function that is the heart of the app's logic: given raw `claude --print "/usage"` CLI text, the current time, the DeepSeek window configuration, and the previous display state, it produces the new display state and any notification events to fire. This includes:

- Parsing raw `/usage` text into structured data (session % used, session reset time, week % used, week reset time). Handle malformed/unparseable input by falling back to the previous display state's values, marked stale, rather than erroring or blanking.
- Computing a pacing classification (underusing / on-pace / overusing) for both the session window and the weekly window, by comparing % of quota used against % of the window's duration elapsed.
- Computing DeepSeek peak/off-peak status by comparing the current time against the configured peak windows (default: 9:00–12:00 and 14:00–18:00 Beijing time), correctly handling timezone conversion.
- Diffing previous vs. new DeepSeek status to emit a notification event only on a state transition (edge-triggered), not on every poll.

This module has no UI and no subprocess/clock/notification I/O of its own — it's a pure function, fully exercised by unit tests, and does not depend on the app shell existing.

**Blocked by:** None — can start immediately

**Status:** completed

- [x] Given valid `/usage` sample text, returns correctly parsed session %, session reset time, week %, and week reset time
- [x] Given malformed/unparseable `/usage` text, returns the previous display state's values marked stale, without erroring
- [x] Given a quota-used % and elapsed-time %, classifies pacing correctly at under/on-pace/over boundary cases for both session and weekly windows
- [x] Given a current time and the default DeepSeek windows, correctly reports peak vs. off-peak status, including at window boundary edges
- [x] Given a previous state and a new state with a changed DeepSeek status, emits exactly one notification event; given no change, emits none
- [x] Unit tests cover all of the above cases using standard Rust `#[cfg(test)]` conventions
