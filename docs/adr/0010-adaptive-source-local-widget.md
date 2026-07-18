# Adaptive source-local widget

The widget keeps Claude, optional Codex, and DeepSeek in one vertical flow. Codex enablement changes only the widget height; the native shell preserves the current position when possible and clamps the complete rectangle to the monitor when expansion would move it off-screen. Source-local freshness is rendered in the corresponding section, so a Codex failure does not dim live Claude or DeepSeek data.

The frontend requests geometry changes through a Tauri command rather than directly managing native window coordinates. This keeps position persistence and multi-monitor safety in the imperative shell while leaving source state and display serialization explicit at the frontend boundary.
