# Poll Codex with a short-lived app server

When Codex monitoring is enabled, start a short-lived `codex app-server` subprocess for each poll, read `account/rateLimits/read`, and then terminate it. This preserves the monitor's poll-oriented architecture and avoids persistent-daemon ownership, reconnection, and shutdown complexity at the cost of process-start overhead on each poll.
