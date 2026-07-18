use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike};

#[derive(Debug, Clone, PartialEq)]
pub enum Pacing {
    Underusing,
    OnPace,
    Overusing,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeepSeekStatus {
    Peak { window_label: String },
    OffPeak,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotificationEvent {
    DeepSeekPeakStarted { window_label: String },
    DeepSeekPeakEnded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeepSeekWindow {
    pub start_hour: u8,
    pub end_hour: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub deepseek_windows: Vec<DeepSeekWindow>,
    pub under_pace_threshold: f64,
    pub over_pace_threshold: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            deepseek_windows: vec![
                DeepSeekWindow {
                    start_hour: 9,
                    end_hour: 12,
                },
                DeepSeekWindow {
                    start_hour: 14,
                    end_hour: 18,
                },
            ],
            under_pace_threshold: 1.0,
            over_pace_threshold: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeState {
    pub session_used_pct: Option<f64>,
    pub session_reset_time_text: Option<String>,
    pub session_pacing: Option<Pacing>,
    pub session_window_start: Option<String>,
    pub week_used_pct: Option<f64>,
    pub week_reset_time_text: Option<String>,
    pub week_pacing: Option<Pacing>,
    pub stale: bool,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexWindowState {
    pub label: String,
    pub used_pct: f64,
    pub reset_time_text: Option<String>,
    pub pacing: Option<Pacing>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexState {
    pub enabled: bool,
    pub loading: bool,
    pub available: bool,
    pub stale: bool,
    pub diagnostic: Option<String>,
    pub primary: Option<CodexWindowState>,
    pub windows: Vec<CodexWindowState>,
}

impl Default for CodexState {
    fn default() -> Self {
        Self {
            enabled: false,
            loading: false,
            available: false,
            stale: false,
            diagnostic: None,
            primary: None,
            windows: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexPollResult {
    Success { windows: Vec<CodexPollWindow> },
    Failure { diagnostic: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexPollWindow {
    pub kind: CodexWindowKind,
    pub used_pct: f64,
    pub reset_at: Option<chrono::DateTime<FixedOffset>>,
    pub reset_time_text: Option<String>,
    pub window_duration_mins: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodexWindowKind {
    Primary,
    Secondary,
}

pub fn codex_failure_diagnostic(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("login") || lower.contains("auth") {
        "Codex login required: run `codex login`.".into()
    } else if lower.contains("method not found")
        || lower.contains("unknown method")
        || lower.contains("protocol")
        || lower.contains("compatible snapshot")
        || lower.contains("usable windows")
        || lower.contains("missing rate-limit")
        || lower.contains("missing usedpercent")
    {
        "Codex CLI update required for rate-limit monitoring.".into()
    } else {
        "Codex rate-limit poll failed.".into()
    }
}

/// A successful initialize response is enough to attempt the rate-limit
/// request. Codex has changed the shape of its advertised capabilities across
/// CLI releases, so the rate-limit response is the authoritative compatibility
/// check rather than a particular `result.capabilities` object shape.
pub fn has_codex_initialize_result(response: &serde_json::Value) -> bool {
    response
        .get("result")
        .map(serde_json::Value::is_object)
        .unwrap_or(false)
}

pub fn logical_size_to_physical(width: u32, height: u32, scale_factor: f64) -> (u32, u32) {
    let scale_factor = scale_factor.max(0.01);
    (
        ((width as f64) * scale_factor).round().max(1.0) as u32,
        ((height as f64) * scale_factor).round().max(1.0) as u32,
    )
}

/// Parse the current and legacy app-server response shapes into the pure
/// poll-cycle model. Only the named Codex group and its primary/secondary
/// windows are relevant; controls such as credits are deliberately ignored.
pub fn parse_codex_response(response: &serde_json::Value) -> Result<CodexPollResult, String> {
    let result = response
        .get("result")
        .ok_or_else(|| "missing rate-limit response".to_string())?;
    let snapshot = result
        .get("rateLimitsByLimitId")
        .and_then(|groups| groups.get("codex"))
        .filter(|snapshot| !snapshot.is_null())
        .or_else(|| result.get("rateLimits"))
        .ok_or_else(|| "rate-limit response has no compatible snapshot".to_string())?;
    let mut windows = Vec::new();
    for (kind, names) in [
        (CodexWindowKind::Primary, ["primary", "primaryWindow"]),
        (CodexWindowKind::Secondary, ["secondary", "secondaryWindow"]),
    ] {
        if let Some(window) = names.iter().find_map(|name| snapshot.get(name)) {
            if window.is_null() {
                continue;
            }
            let name = if kind == CodexWindowKind::Primary {
                "primary"
            } else {
                "secondary"
            };
            let used_pct = window
                .get("usedPercent")
                .or_else(|| window.get("used_percent"))
                .and_then(|value| value.as_f64())
                .ok_or_else(|| format!("{name} rate-limit window is missing usedPercent"))?;
            let window_duration_mins = window
                .get("windowDurationMins")
                .or_else(|| window.get("window_duration_mins"))
                .and_then(|value| value.as_i64());
            let reset_value = window.get("resetsAt").or_else(|| window.get("resets_at"));
            let reset_at = reset_value
                .and_then(|value| value.as_i64())
                .and_then(|seconds| DateTime::<chrono::Utc>::from_timestamp(seconds, 0))
                .map(|date| date.with_timezone(&FixedOffset::east_opt(0).unwrap()))
                .or_else(|| {
                    reset_value
                        .and_then(|value| value.as_str())
                        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                });
            let reset_time_text = reset_value
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .or_else(|| reset_at.as_ref().map(DateTime::to_rfc3339));
            windows.push(CodexPollWindow {
                kind,
                used_pct,
                reset_at,
                reset_time_text,
                window_duration_mins,
            });
        }
    }
    if windows.is_empty() {
        return Err("rate-limit response has no usable windows".into());
    }
    Ok(CodexPollResult::Success { windows })
}

impl Default for ClaudeState {
    fn default() -> Self {
        Self {
            session_used_pct: None,
            session_reset_time_text: None,
            session_pacing: None,
            session_window_start: None,
            week_used_pct: None,
            week_reset_time_text: None,
            week_pacing: None,
            stale: false,
            diagnostic: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayState {
    pub claude: ClaudeState,
    pub codex: CodexState,
    pub deepseek_status: DeepSeekStatus,
    pub next_transition_info: Option<String>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            claude: ClaudeState::default(),
            codex: CodexState::default(),
            deepseek_status: DeepSeekStatus::OffPeak,
            next_transition_info: None,
        }
    }
}

impl DisplayState {
    pub fn new_diagnostic(msg: impl Into<String>) -> Self {
        Self {
            claude: ClaudeState {
                diagnostic: Some(msg.into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

const SESSION_WINDOW_HOURS: i64 = 5;

/// Session and week lines are parsed independently so a missing/malformed
/// session line (e.g. no session started since the last reset) doesn't force
/// a total parse failure when the week line is perfectly readable.
struct ParsedUsage {
    session: Option<(f64, String)>,
    session_line_present: bool,
    week: Option<(f64, String)>,
}

fn parse_usage_sections(text: &str) -> ParsedUsage {
    let mut session_pct: Option<f64> = None;
    let mut session_reset: Option<String> = None;
    let mut session_line_present = false;
    let mut week_pct: Option<f64> = None;
    let mut week_reset: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Current session:") {
            session_line_present = true;
            if let Some(pct) = extract_percentage(line) {
                session_pct = Some(pct);
            }
            if let Some(reset) = extract_reset_time(line) {
                session_reset = Some(reset);
            }
        } else if line.starts_with("Current week (all models):") {
            if let Some(pct) = extract_percentage(line) {
                week_pct = Some(pct);
            }
            if let Some(reset) = extract_reset_time(line) {
                week_reset = Some(reset);
            }
        }
    }

    ParsedUsage {
        session: session_pct.zip(session_reset),
        session_line_present,
        week: week_pct.zip(week_reset),
    }
}

#[cfg(test)]
fn parse_claude_usage_text(text: &str) -> Option<(f64, String, f64, String)> {
    let parsed = parse_usage_sections(text);
    match (parsed.session, parsed.week) {
        (Some((sp, sr)), Some((wp, wr))) => Some((sp, sr, wp, wr)),
        _ => None,
    }
}

fn extract_percentage(line: &str) -> Option<f64> {
    let pct_str = line.split('%').next()?;
    let token = pct_str.split_whitespace().last()?;
    token.parse::<f64>().ok()
}

fn extract_reset_time(line: &str) -> Option<String> {
    let after_resets = line.split("resets ").nth(1)?;
    let trimmed = after_resets.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_reset_datetime(
    reset_text: &str,
    local_offset: FixedOffset,
    current_year: i32,
) -> Option<DateTime<FixedOffset>> {
    let without_tz = reset_text.split(" (").next()?;
    let clean = without_tz.trim();

    // Split on comma: "Jul 18, 4am" → date="Jul 18", time_part="4am"
    let (date_str, time_str) = {
        let comma = clean.find(',')?;
        (&clean[..comma], clean[comma + 1..].trim())
    };

    let naive_date =
        NaiveDate::parse_from_str(&format!("{} {}", date_str, current_year), "%b %d %Y").ok()?;

    // Time part may be like "4am", "4:40pm", "8:40pm", "4:00am"
    let time_lower = time_str.to_lowercase();
    let is_pm = time_lower.contains("pm");
    let without_ampm = time_lower
        .replace("am", "")
        .replace("pm", "")
        .trim()
        .to_string();

    let (hour, minute) = if let Some(pos) = without_ampm.find(':') {
        let h: u32 = without_ampm[..pos].parse().ok()?;
        let m: u32 = without_ampm[pos + 1..].parse().ok()?;
        (h, m)
    } else {
        let h: u32 = without_ampm.parse().ok()?;
        (h, 0)
    };

    let hour_24 = if is_pm && hour != 12 {
        hour + 12
    } else if !is_pm && hour == 12 {
        0
    } else {
        hour
    };

    let naive = naive_date.and_hms_opt(hour_24, minute, 0)?;
    naive.and_local_timezone(local_offset).single()
}

fn compute_pacing(
    used_pct: f64,
    elapsed_pct: f64,
    under_threshold: f64,
    over_threshold: f64,
) -> Pacing {
    if used_pct >= 100.0 && elapsed_pct < 100.0 {
        return Pacing::Overusing;
    }
    let diff = used_pct - elapsed_pct;
    if diff < -under_threshold {
        Pacing::Underusing
    } else if diff > over_threshold {
        Pacing::Overusing
    } else {
        Pacing::OnPace
    }
}

/// Backoff multiplier caps growth so a persistently broken `claude` subprocess
/// doesn't push the poll interval out indefinitely.
const MAX_BACKOFF_MULTIPLIER: u64 = 8;

/// Computes the delay before the next poll attempt, doubling on each
/// consecutive failure and capping at `MAX_BACKOFF_MULTIPLIER` × the
/// user-configured interval. Resets to the base interval once failures clear.
pub fn next_poll_delay_secs(base_interval_secs: u64, consecutive_failures: u32) -> u64 {
    if consecutive_failures == 0 {
        return base_interval_secs;
    }
    let multiplier = 2u64.saturating_pow(consecutive_failures.min(10));
    base_interval_secs.saturating_mul(multiplier.min(MAX_BACKOFF_MULTIPLIER))
}

fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("UTC+8 is valid")
}

fn to_beijing_time(dt: &DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    dt.with_timezone(&beijing_offset())
}

fn is_in_peak_window(bj_hour: u32, config: &Config) -> Option<&DeepSeekWindow> {
    for window in &config.deepseek_windows {
        let start = window.start_hour as u32;
        let end = window.end_hour as u32;
        if bj_hour >= start && bj_hour < end {
            return Some(window);
        }
    }
    None
}

pub fn deepseek_window_label(window: &DeepSeekWindow) -> String {
    format!("{:02}:00–{:02}:00 BJT", window.start_hour, window.end_hour)
}

fn time_until_next_transition(
    bj_dt: &DateTime<FixedOffset>,
    config: &Config,
) -> Option<(String, bool)> {
    let bj_hour = bj_dt.hour();
    let bj_minute = bj_dt.minute();

    let mut candidates: Vec<(u32, u32, bool)> = Vec::new();

    for window in &config.deepseek_windows {
        candidates.push((window.start_hour as u32, 0, true));
        candidates.push((window.end_hour as u32, 0, false));
    }

    candidates.sort();

    let current_minutes = bj_hour * 60 + bj_minute;

    for &(target_h, target_m, is_start) in &candidates {
        let target_minutes = target_h * 60 + target_m;
        if target_minutes > current_minutes {
            let diff = target_minutes - current_minutes;
            if diff < 60 {
                return Some((format!("{}m", diff), is_start));
            } else {
                return Some((format!("{}h{}m", diff / 60, diff % 60), is_start));
            }
        }
    }

    let first = candidates.first()?;
    let diff = (24 * 60 - current_minutes) + (first.0 * 60 + first.1);
    if diff < 60 {
        Some((format!("{}m", diff), first.2))
    } else {
        Some((format!("{}h{}m", diff / 60, diff % 60), first.2))
    }
}

fn compute_deepseek_status(
    current_time: &DateTime<FixedOffset>,
    config: &Config,
) -> (DeepSeekStatus, Option<String>) {
    let bj_time = to_beijing_time(current_time);
    let bj_hour = bj_time.hour();

    if let Some(window) = is_in_peak_window(bj_hour, &config) {
        let next_info = time_until_next_transition(&bj_time, config)
            .map(|(info, _)| format!("Peak ends in {}", info));
        (
            DeepSeekStatus::Peak {
                window_label: deepseek_window_label(window),
            },
            next_info,
        )
    } else {
        match time_until_next_transition(&bj_time, config) {
            Some((info, true)) => (
                DeepSeekStatus::OffPeak,
                Some(format!("Next peak in {}", info)),
            ),
            Some((info, false)) => (
                DeepSeekStatus::OffPeak,
                Some(format!("Off-peak until {}", info)),
            ),
            None => (DeepSeekStatus::OffPeak, None),
        }
    }
}

pub fn poll_cycle(
    raw_usage_text: Option<&str>,
    current_time: &DateTime<FixedOffset>,
    config: &Config,
    previous_state: &DisplayState,
    codex_enabled: bool,
    codex_result: Option<&CodexPollResult>,
) -> (DisplayState, Vec<NotificationEvent>) {
    let parsed = raw_usage_text.map(parse_usage_sections);

    let awaiting_session = matches!(
        &parsed,
        Some(p) if p.week.is_some() && p.session.is_none() && !p.session_line_present
    );

    let full = parsed.as_ref().and_then(|p| match (&p.session, &p.week) {
        (Some((sp, sr)), Some((wp, wr))) => Some((*sp, sr.clone(), *wp, wr.clone())),
        _ => None,
    });

    let (mut display, usage_ok) = if let Some((sp, sr, wp, wr)) = full {
        let local_offset = *current_time.offset();
        let current_year = current_time.year();
        let session_reset_dt = parse_reset_datetime(&sr, local_offset, current_year);
        let week_reset_dt = parse_reset_datetime(&wr, local_offset, current_year);

        let session_start_str =
            if previous_state.claude.session_reset_time_text.as_deref() != Some(&sr) {
                session_reset_dt.map(|dt| (dt - Duration::hours(SESSION_WINDOW_HOURS)).to_rfc3339())
            } else {
                previous_state.claude.session_window_start.clone()
            };

        let session_window_start = session_start_str
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());

        let session_pacing = match (session_window_start, session_reset_dt) {
            (Some(start), Some(reset)) if start < reset => {
                let total_dur = reset - start;
                let elapsed = *current_time - start;
                if total_dur.num_seconds() > 0 {
                    let elapsed_pct =
                        (elapsed.num_seconds() as f64 / total_dur.num_seconds() as f64) * 100.0;
                    let elapsed_pct = elapsed_pct.clamp(0.0, 100.0);
                    Some(compute_pacing(
                        sp,
                        elapsed_pct,
                        config.under_pace_threshold,
                        config.over_pace_threshold,
                    ))
                } else {
                    previous_state.claude.session_pacing.clone()
                }
            }
            _ => previous_state.claude.session_pacing.clone(),
        };

        let week_pacing = match week_reset_dt {
            Some(reset) => {
                let window_start = reset - Duration::days(7);
                let total_dur = reset - window_start;
                let elapsed = *current_time - window_start;
                if total_dur.num_seconds() > 0 {
                    let elapsed_pct =
                        (elapsed.num_seconds() as f64 / total_dur.num_seconds() as f64) * 100.0;
                    let elapsed_pct = elapsed_pct.clamp(0.0, 100.0);
                    Some(compute_pacing(
                        wp,
                        elapsed_pct,
                        config.under_pace_threshold,
                        config.over_pace_threshold,
                    ))
                } else {
                    previous_state.claude.week_pacing.clone()
                }
            }
            _ => previous_state.claude.week_pacing.clone(),
        };

        let claude = ClaudeState {
            session_used_pct: Some(sp),
            session_reset_time_text: Some(sr),
            session_pacing,
            session_window_start: session_start_str,
            week_used_pct: Some(wp),
            week_reset_time_text: Some(wr),
            week_pacing,
            ..Default::default()
        };

        (
            DisplayState {
                claude,
                ..Default::default()
            },
            true,
        )
    } else if awaiting_session {
        let (wp, wr) = parsed
            .as_ref()
            .and_then(|p| p.week.clone())
            .expect("awaiting_session implies week is Some");
        let local_offset = *current_time.offset();
        let current_year = current_time.year();
        let week_reset_dt = parse_reset_datetime(&wr, local_offset, current_year);

        let week_pacing = match week_reset_dt {
            Some(reset) => {
                let window_start = reset - Duration::days(7);
                let total_dur = reset - window_start;
                let elapsed = *current_time - window_start;
                if total_dur.num_seconds() > 0 {
                    let elapsed_pct =
                        (elapsed.num_seconds() as f64 / total_dur.num_seconds() as f64) * 100.0;
                    let elapsed_pct = elapsed_pct.clamp(0.0, 100.0);
                    Some(compute_pacing(
                        wp,
                        elapsed_pct,
                        config.under_pace_threshold,
                        config.over_pace_threshold,
                    ))
                } else {
                    previous_state.claude.week_pacing.clone()
                }
            }
            _ => previous_state.claude.week_pacing.clone(),
        };

        let claude = ClaudeState {
            session_used_pct: Some(0.0),
            session_reset_time_text: Some("Not started".to_string()),
            session_pacing: None,
            session_window_start: None,
            week_used_pct: Some(wp),
            week_reset_time_text: Some(wr),
            week_pacing,
            ..Default::default()
        };

        (
            DisplayState {
                claude,
                ..Default::default()
            },
            true,
        )
    } else {
        let mut display = previous_state.clone();
        display.claude.stale = raw_usage_text.is_some();
        // carry diagnostic forward instead of clearing it
        (display, false)
    };

    if usage_ok {
        display.claude.stale = false;
    } else if raw_usage_text.is_none() {
        display.claude.stale = true;
    }

    display.codex = update_codex_state(
        codex_enabled,
        codex_result,
        current_time,
        config,
        &previous_state.codex,
    );

    let (ds_status, ds_info) = compute_deepseek_status(current_time, config);
    display.deepseek_status = ds_status.clone();
    display.next_transition_info = ds_info;

    let mut events = Vec::new();
    if display.deepseek_status != previous_state.deepseek_status {
        match &display.deepseek_status {
            DeepSeekStatus::Peak { window_label } => {
                events.push(NotificationEvent::DeepSeekPeakStarted {
                    window_label: window_label.clone(),
                });
            }
            DeepSeekStatus::OffPeak => {
                events.push(NotificationEvent::DeepSeekPeakEnded);
            }
        }
    }

    (display, events)
}

fn update_codex_state(
    enabled: bool,
    result: Option<&CodexPollResult>,
    current_time: &DateTime<FixedOffset>,
    config: &Config,
    previous: &CodexState,
) -> CodexState {
    if !enabled {
        return CodexState::default();
    }

    let Some(result) = result else {
        let mut state = previous.clone();
        state.enabled = true;
        state.loading = true;
        return state;
    };

    match result {
        CodexPollResult::Failure { diagnostic } => CodexState {
            enabled: true,
            loading: false,
            available: previous.available,
            stale: previous.available,
            diagnostic: Some(diagnostic.clone()),
            primary: previous.primary.clone(),
            windows: previous.windows.clone(),
        },
        CodexPollResult::Success { windows } => {
            let primary_index = windows
                .iter()
                .position(|window| window.kind == CodexWindowKind::Primary);
            let windows = windows
                .iter()
                .map(|window| {
                    let pacing = match (window.reset_at, window.window_duration_mins) {
                        (Some(reset), Some(duration)) if duration > 0 => {
                            let start = reset - Duration::minutes(duration);
                            let total = reset - start;
                            let elapsed = (*current_time - start).num_seconds() as f64;
                            let elapsed_pct = if total.num_seconds() > 0 {
                                (elapsed / total.num_seconds() as f64 * 100.0).clamp(0.0, 100.0)
                            } else {
                                return CodexWindowState {
                                    label: window_label(window),
                                    used_pct: window.used_pct,
                                    reset_time_text: window.reset_time_text.clone(),
                                    pacing: None,
                                };
                            };
                            Some(compute_pacing(
                                window.used_pct,
                                elapsed_pct,
                                config.under_pace_threshold,
                                config.over_pace_threshold,
                            ))
                        }
                        _ => None,
                    };
                    CodexWindowState {
                        label: window_label(window),
                        used_pct: window.used_pct,
                        reset_time_text: window.reset_time_text.clone(),
                        pacing,
                    }
                })
                .collect::<Vec<_>>();
            let primary = primary_index.and_then(|index| windows.get(index).cloned());
            CodexState {
                enabled: true,
                loading: false,
                available: true,
                stale: false,
                diagnostic: None,
                primary,
                windows,
            }
        }
    }
}

fn window_label(window: &CodexPollWindow) -> String {
    window
        .window_duration_mins
        .map(codex_duration_label)
        .unwrap_or_else(|| match window.kind {
            CodexWindowKind::Primary => "Primary".into(),
            CodexWindowKind::Secondary => "Secondary".into(),
        })
}

pub fn codex_duration_label(minutes: i64) -> String {
    if minutes % (24 * 60) == 0 {
        let days = minutes / (24 * 60);
        format!("{}-day", days)
    } else if minutes % 60 == 0 {
        let hours = minutes / 60;
        format!("{}-hour", hours)
    } else {
        format!("{}-minute", minutes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn is_position_visible(window_rect: &Rect, monitor_rects: &[Rect]) -> bool {
    monitor_rects.iter().any(|m| rects_overlap(window_rect, m))
}

pub fn clamp_window_position(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    monitor: &Rect,
) -> (i32, i32) {
    let max_x = monitor
        .x
        .saturating_add(monitor.width as i32)
        .saturating_sub(width as i32);
    let max_y = monitor
        .y
        .saturating_add(monitor.height as i32)
        .saturating_sub(height as i32);
    (
        x.clamp(monitor.x, max_x.max(monitor.x)),
        y.clamp(monitor.y, max_y.max(monitor.y)),
    )
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    let a_x2 = a.x.saturating_add(a.width as i32);
    let a_y2 = a.y.saturating_add(a.height as i32);
    let b_x2 = b.x.saturating_add(b.width as i32);
    let b_y2 = b.y.saturating_add(b.height as i32);

    a.x < b_x2 && a_x2 > b.x && a.y < b_y2 && a_y2 > b.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use chrono::TimeZone;

    fn local_offset() -> FixedOffset {
        FixedOffset::east_opt(7 * 3600).unwrap()
    }

    fn make_time(hour: u32, min: u32, sec: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(7 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 7, 13, hour, min, sec)
            .unwrap()
    }

    fn beijing_time(hour: u32, min: u32, sec: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 7, 13, hour, min, sec)
            .unwrap()
    }

    const VALID_USAGE_TEXT: &str = "Current session: 8% used \u{00b7} resets Jul 13, 8:40pm (Asia/Jakarta)\nCurrent week (all models): 13% used \u{00b7} resets Jul 18, 4am (Asia/Jakarta)";

    #[test]
    fn test_parse_valid_usage() {
        let result = parse_claude_usage_text(VALID_USAGE_TEXT);
        assert!(result.is_some(), "parse_claude_usage_text returned None");
        let (sp, sr, wp, wr) = result.unwrap();
        assert_eq!(sp, 8.0, "session pct should be 8, got {sp}");
        assert!(sr.contains("Jul 13"), "sr '{sr}' should contain 'Jul 13'");
        assert_eq!(wp, 13.0, "week pct should be 13, got {wp}");
        assert!(wr.contains("Jul 18"), "wr '{wr}' should contain 'Jul 18'");
    }

    const FABLE_USAGE_TEXT: &str = "Current session: 3% used \u{00b7} resets Jul 14, 2:10am (Asia/Jakarta)\nCurrent week (all models): 14% used \u{00b7} resets Jul 18, 4am (Asia/Jakarta)\nCurrent week (Fable): 0% used";

    #[test]
    fn test_parse_usage_with_fable_line() {
        let result = parse_claude_usage_text(FABLE_USAGE_TEXT);
        assert!(result.is_some(), "should parse despite Fable line");
        let (sp, sr, wp, wr) = result.unwrap();
        assert_eq!(sp, 3.0, "session pct should be 3, got {sp}");
        assert_eq!(
            wp, 14.0,
            "week pct should be 14 (all models), got {wp} — Fable line overwrote it"
        );
        assert!(
            wr.contains("Jul 18"),
            "week reset should be Jul 18, got '{wr}'"
        );
    }

    #[test]
    fn test_parse_malformed_text_returns_none() {
        let result = parse_claude_usage_text("garbage output that doesn't match");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_partial_text_returns_none() {
        let result = parse_claude_usage_text(
            "Current session: 8% used \u{00b7} resets Jul 13, 8:40pm (Asia/Jakarta)",
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_text_with_missing_percentage() {
        let result = parse_claude_usage_text("Current session: used \u{00b7} resets Jul 13, 8:40pm (Asia/Jakarta)\nCurrent week (all models): 13% used \u{00b7} resets Jul 18, 4am (Asia/Jakarta)");
        assert!(result.is_none());
    }

    const WEEK_ONLY_USAGE_TEXT: &str = "No active session \u{00b7} run claude to start one\nCurrent week (all models): 13% used \u{00b7} resets Jul 18, 4am (Asia/Jakarta)";

    #[test]
    fn test_poll_cycle_with_no_session_line_is_awaiting_not_stale() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState {
            claude: ClaudeState {
                session_used_pct: Some(8.0),
                session_reset_time_text: Some("Jul 13, 8:40pm (Asia/Jakarta)".into()),
                week_used_pct: Some(10.0),
                week_reset_time_text: Some("Jul 18, 4am (Asia/Jakarta)".into()),
                ..Default::default()
            },
            ..Default::default()
        };

        let (display, _events) = poll_cycle(
            Some(WEEK_ONLY_USAGE_TEXT),
            &now,
            &config,
            &prev,
            false,
            None,
        );

        assert!(
            !display.claude.stale,
            "awaiting session should not be treated as stale"
        );
        assert_eq!(display.claude.session_used_pct, Some(0.0));
        assert_eq!(
            display.claude.session_reset_time_text,
            Some("Not started".to_string())
        );
        assert_eq!(
            display.claude.session_pacing, None,
            "no pacing badge while awaiting a session"
        );
        assert_eq!(
            display.claude.week_used_pct,
            Some(13.0),
            "week should keep updating normally"
        );
    }

    #[test]
    fn test_poll_cycle_with_malformed_session_line_still_stale() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState {
            claude: ClaudeState {
                session_used_pct: Some(8.0),
                session_reset_time_text: Some("Jul 13, 8:40pm (Asia/Jakarta)".into()),
                week_used_pct: Some(10.0),
                week_reset_time_text: Some("Jul 18, 4am (Asia/Jakarta)".into()),
                diagnostic: Some("previous diagnostic".into()),
                ..Default::default()
            },
            ..Default::default()
        };

        // Session line present but missing its percentage — a real corruption,
        // not the clean "no session line at all" case, so it must stay Stale.
        let text = "Current session: used \u{00b7} resets Jul 13, 8:40pm (Asia/Jakarta)\nCurrent week (all models): 13% used \u{00b7} resets Jul 18, 4am (Asia/Jakarta)";

        let (display, _events) = poll_cycle(Some(text), &now, &config, &prev, false, None);

        assert!(
            display.claude.stale,
            "malformed session line should remain Stale, not Awaiting session"
        );
        assert_eq!(
            display.claude.session_used_pct,
            Some(8.0),
            "previous value carried forward while stale"
        );
        assert_eq!(
            display.claude.diagnostic.as_deref(),
            Some("previous diagnostic")
        );
        assert!(
            matches!(display.deepseek_status, DeepSeekStatus::Peak { .. }),
            "DeepSeek should remain independently computed"
        );
    }

    #[test]
    fn test_poll_cycle_with_valid_usage() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState::default();

        let (display, _events) =
            poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev, false, None);

        assert!(!display.claude.stale);
        assert_eq!(display.claude.session_used_pct, Some(8.0));
        assert_eq!(display.claude.week_used_pct, Some(13.0));
        assert!(display
            .claude
            .session_reset_time_text
            .unwrap()
            .contains("Jul 13, 8:40pm"));
    }

    #[test]
    fn test_poll_cycle_with_none_text_marks_stale() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState::default();

        let (display, _events) = poll_cycle(None, &now, &config, &prev, false, None);

        assert!(display.claude.stale);
        assert_eq!(display.claude.session_used_pct, None);
    }

    #[test]
    fn test_poll_cycle_with_malformed_text_uses_previous() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState {
            claude: ClaudeState {
                session_used_pct: Some(8.0),
                session_reset_time_text: Some("Jul 13, 8:40pm (Asia/Jakarta)".into()),
                week_used_pct: Some(13.0),
                week_reset_time_text: Some("Jul 18, 4am (Asia/Jakarta)".into()),
                ..Default::default()
            },
            ..Default::default()
        };

        let (display, _events) = poll_cycle(
            Some("garbage malformed text"),
            &now,
            &config,
            &prev,
            false,
            None,
        );

        assert!(display.claude.stale);
        assert_eq!(display.claude.session_used_pct, Some(8.0));
        assert_eq!(display.claude.week_used_pct, Some(13.0));
    }

    #[test]
    fn test_codex_disabled_ignores_result_and_is_not_displayed() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let result = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 25.0,
                reset_at: None,
                reset_time_text: None,
                window_duration_mins: Some(300),
            }],
        };

        let (display, events) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &now,
            &config,
            &DisplayState::default(),
            false,
            Some(&result),
        );

        assert_eq!(display.codex, CodexState::default());
        assert!(
            events.is_empty()
                || events
                    .iter()
                    .all(|event| matches!(event, NotificationEvent::DeepSeekPeakStarted { .. }))
        );
    }

    #[test]
    fn test_codex_primary_window_is_available_and_paced_without_notifications() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let reset_at = chrono::DateTime::parse_from_rfc3339("2026-07-13T15:00:00+07:00").unwrap();
        let result = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 50.0,
                reset_at: Some(reset_at),
                reset_time_text: Some(reset_at.to_rfc3339()),
                window_duration_mins: Some(300),
            }],
        };

        let (display, events) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &now,
            &config,
            &DisplayState::default(),
            true,
            Some(&result),
        );

        let primary = display
            .codex
            .primary
            .as_ref()
            .expect("primary window should be displayed");
        assert!(display.codex.available);
        assert!(!display.codex.stale);
        assert_eq!(primary.label, "5-hour");
        assert_eq!(primary.used_pct, 50.0);
        assert_eq!(primary.pacing, Some(Pacing::Underusing));
        assert!(events
            .iter()
            .all(|event| matches!(event, NotificationEvent::DeepSeekPeakStarted { .. })));
    }

    #[test]
    fn test_codex_primary_and_secondary_windows_are_both_displayed() {
        let now = make_time(14, 0, 0);
        let reset = chrono::DateTime::parse_from_rfc3339("2026-07-13T15:00:00+07:00").unwrap();
        let result = CodexPollResult::Success {
            windows: vec![
                CodexPollWindow {
                    kind: CodexWindowKind::Primary,
                    used_pct: 50.0,
                    reset_at: Some(reset),
                    reset_time_text: Some("primary reset".into()),
                    window_duration_mins: Some(300),
                },
                CodexPollWindow {
                    kind: CodexWindowKind::Secondary,
                    used_pct: 25.0,
                    reset_at: Some(reset),
                    reset_time_text: Some("secondary reset".into()),
                    window_duration_mins: Some(7 * 24 * 60),
                },
            ],
        };
        let (display, events) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &now,
            &Config::default(),
            &DisplayState::default(),
            true,
            Some(&result),
        );
        assert_eq!(display.codex.windows.len(), 2);
        assert_eq!(display.codex.windows[0].label, "5-hour");
        assert_eq!(display.codex.windows[1].label, "7-day");
        assert!(events.iter().all(|event| matches!(
            event,
            NotificationEvent::DeepSeekPeakStarted { .. } | NotificationEvent::DeepSeekPeakEnded
        )));
    }

    #[test]
    fn test_codex_missing_optional_secondary_is_success() {
        let result = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 10.0,
                reset_at: None,
                reset_time_text: None,
                window_duration_mins: None,
            }],
        };
        let (display, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(14, 0, 0),
            &Config::default(),
            &DisplayState::default(),
            true,
            Some(&result),
        );
        assert!(display.codex.available);
        assert_eq!(display.codex.windows.len(), 1);
        assert_eq!(display.codex.windows[0].pacing, None);
        assert_eq!(display.codex.windows[0].used_pct, 10.0);
    }

    #[test]
    fn test_codex_fresh_incomplete_metadata_does_not_reuse_old_pace() {
        let previous = CodexState {
            enabled: true,
            available: true,
            primary: Some(CodexWindowState {
                label: "5-hour".into(),
                used_pct: 10.0,
                reset_time_text: None,
                pacing: Some(Pacing::Underusing),
            }),
            windows: vec![],
            ..Default::default()
        };
        let previous = DisplayState {
            codex: previous,
            ..Default::default()
        };
        let result = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 90.0,
                reset_at: None,
                reset_time_text: None,
                window_duration_mins: None,
            }],
        };
        let (display, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(14, 0, 0),
            &Config::default(),
            &previous,
            true,
            Some(&result),
        );
        assert_eq!(display.codex.windows[0].pacing, None);
    }

    #[test]
    fn test_codex_pacing_clamps_after_reset_and_exhaustion_before_reset() {
        let reset = chrono::DateTime::parse_from_rfc3339("2026-07-13T15:00:00+07:00").unwrap();
        let exhausted = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 100.0,
                reset_at: Some(reset),
                reset_time_text: None,
                window_duration_mins: Some(300),
            }],
        };
        let (before, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(12, 0, 0),
            &Config::default(),
            &DisplayState::default(),
            true,
            Some(&exhausted),
        );
        assert_eq!(before.codex.windows[0].pacing, Some(Pacing::Overusing));
        let (after, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(16, 0, 0),
            &Config::default(),
            &DisplayState::default(),
            true,
            Some(&exhausted),
        );
        assert_eq!(after.codex.windows[0].pacing, Some(Pacing::OnPace));
    }

    #[test]
    fn test_codex_first_poll_failure_is_visible_unavailable() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let result = CodexPollResult::Failure {
            diagnostic: "Codex login required: run `codex login`.".into(),
        };

        let (display, _events) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &now,
            &config,
            &DisplayState::default(),
            true,
            Some(&result),
        );

        assert!(display.codex.enabled);
        assert!(!display.codex.loading);
        assert!(!display.codex.available);
        assert!(!display.codex.stale);
        assert!(display.codex.primary.is_none());
        assert_eq!(
            display.codex.diagnostic.as_deref(),
            Some("Codex login required: run `codex login`.")
        );
        assert!(!display.claude.stale);
    }

    #[test]
    fn test_codex_failure_retains_only_codex_values_and_claude_stays_live() {
        let retained = CodexWindowState {
            label: "5-hour".into(),
            used_pct: 31.0,
            reset_time_text: Some("old reset".into()),
            pacing: Some(Pacing::OnPace),
        };
        let previous = DisplayState {
            codex: CodexState {
                enabled: true,
                available: true,
                primary: Some(retained.clone()),
                windows: vec![retained],
                ..Default::default()
            },
            ..Default::default()
        };
        let result = CodexPollResult::Failure {
            diagnostic: "Codex rate-limit request timed out.".into(),
        };
        let (display, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(14, 0, 0),
            &Config::default(),
            &previous,
            true,
            Some(&result),
        );
        assert!(display.codex.stale);
        assert_eq!(display.codex.windows[0].used_pct, 31.0);
        assert_eq!(
            display.codex.diagnostic.as_deref(),
            Some("Codex rate-limit request timed out.")
        );
        assert!(!display.claude.stale);
        assert!(display.claude.diagnostic.is_none());
    }

    #[test]
    fn test_codex_success_recovers_and_replaces_retained_values() {
        let previous = DisplayState {
            codex: CodexState {
                enabled: true,
                available: true,
                stale: true,
                primary: Some(CodexWindowState {
                    label: "5-hour".into(),
                    used_pct: 31.0,
                    reset_time_text: Some("old reset".into()),
                    pacing: None,
                }),
                windows: vec![],
                diagnostic: Some("temporary failure".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 44.0,
                reset_at: None,
                reset_time_text: Some("new reset".into()),
                window_duration_mins: None,
            }],
        };
        let (display, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(14, 0, 0),
            &Config::default(),
            &previous,
            true,
            Some(&result),
        );
        assert!(!display.codex.stale);
        assert_eq!(display.codex.windows[0].used_pct, 44.0);
        assert_eq!(
            display.codex.windows[0].reset_time_text.as_deref(),
            Some("new reset")
        );
        assert!(display.codex.diagnostic.is_none());
    }

    #[test]
    fn test_late_codex_result_is_ignored_after_disablement() {
        let previous = DisplayState {
            codex: CodexState {
                enabled: true,
                available: true,
                primary: Some(CodexWindowState {
                    label: "5-hour".into(),
                    used_pct: 31.0,
                    reset_time_text: None,
                    pacing: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = CodexPollResult::Success {
            windows: vec![CodexPollWindow {
                kind: CodexWindowKind::Primary,
                used_pct: 99.0,
                reset_at: None,
                reset_time_text: None,
                window_duration_mins: None,
            }],
        };
        let (display, _) = poll_cycle(
            Some(VALID_USAGE_TEXT),
            &make_time(14, 0, 0),
            &Config::default(),
            &previous,
            false,
            Some(&result),
        );
        assert_eq!(display.codex, CodexState::default());
        assert!(!display.claude.stale);
    }

    #[test]
    fn test_classified_codex_failures_remain_concise_and_actionable() {
        assert_eq!(
            codex_failure_diagnostic("authentication required"),
            "Codex login required: run `codex login`."
        );
        assert_eq!(
            codex_failure_diagnostic("method not found"),
            "Codex CLI update required for rate-limit monitoring."
        );
        assert_eq!(
            codex_failure_diagnostic("unexpected server error"),
            "Codex rate-limit poll failed."
        );
        for diagnostic in [
            "Codex CLI not found on PATH.",
            "Codex login required: run `codex login`.",
            "Codex CLI update required for rate-limit monitoring.",
            "Codex rate-limit request timed out.",
            "Codex rate-limit poll failed.",
        ] {
            let result = CodexPollResult::Failure {
                diagnostic: diagnostic.into(),
            };
            let (display, _) = poll_cycle(
                Some(VALID_USAGE_TEXT),
                &make_time(14, 0, 0),
                &Config::default(),
                &DisplayState::default(),
                true,
                Some(&result),
            );
            assert_eq!(display.codex.diagnostic.as_deref(), Some(diagnostic));
            assert!(diagnostic.len() < 100);
        }
    }

    #[test]
    fn test_codex_initialize_accepts_current_and_legacy_result_shapes() {
        assert!(has_codex_initialize_result(&serde_json::json!({
            "result": {
                "serverInfo": { "name": "codex" },
                "capabilities": { "experimentalApi": true }
            }
        })));
        assert!(has_codex_initialize_result(&serde_json::json!({
            "result": { "serverInfo": { "name": "codex", "version": "1.2.3" } }
        })));
        assert!(!has_codex_initialize_result(&serde_json::json!({
            "result": "1.2.3"
        })));
    }

    #[test]
    fn test_logical_size_to_physical_preserves_scaled_widget_dimensions() {
        assert_eq!(logical_size_to_physical(240, 420, 1.0), (240, 420));
        assert_eq!(logical_size_to_physical(240, 420, 1.25), (300, 525));
        assert_eq!(logical_size_to_physical(240, 420, 1.5), (360, 630));
        assert_eq!(logical_size_to_physical(240, 420, 2.0), (480, 840));
    }

    #[test]
    fn test_initialize_without_capabilities_proceeds_to_rate_limit_parsing() {
        let initialize = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "serverInfo": { "name": "codex" } }
        });
        assert!(has_codex_initialize_result(&initialize));

        let rate_limits = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "rateLimitsByLimitId": {
                    "codex": {
                        "primary": {
                            "usedPercent": 42.0,
                            "resetsAt": 1783951200,
                            "windowDurationMins": 300
                        }
                    }
                }
            }
        });
        let CodexPollResult::Success { windows } = parse_codex_response(&rate_limits).unwrap()
        else {
            panic!("rate-limit response should parse after initialize");
        };
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_pct, 42.0);
    }

    #[test]
    fn test_scaled_expansion_clamps_physical_rectangle_at_monitor_edge() {
        let (width, height) = logical_size_to_physical(240, 420, 1.5);
        assert_eq!(
            clamp_window_position(
                2800,
                1400,
                width,
                height,
                &Rect {
                    x: 0,
                    y: 0,
                    width: 2880,
                    height: 1620,
                }
            ),
            (2520, 990)
        );
    }

    #[test]
    fn test_codex_failure_does_not_change_claude_backoff_schedule() {
        assert_eq!(next_poll_delay_secs(300, 0), 300);
        assert_eq!(next_poll_delay_secs(300, 1), 600);
        assert_eq!(next_poll_delay_secs(300, 2), 1200);
    }

    #[test]
    fn test_codex_duration_labels_are_literal() {
        assert_eq!(codex_duration_label(90), "90-minute");
        assert_eq!(codex_duration_label(5 * 60), "5-hour");
        assert_eq!(codex_duration_label(7 * 24 * 60), "7-day");
    }

    #[test]
    fn test_codex_response_named_bucket_precedes_legacy_and_ignores_other_groups() {
        let response = serde_json::json!({ "result": {
            "rateLimitsByLimitId": { "other": { "primary": { "usedPercent": 1.0 } }, "codex": { "primary": { "usedPercent": 42.0, "windowDurationMins": 90 } } },
            "rateLimits": { "primary": { "usedPercent": 2.0, "windowDurationMins": 5 } }
        } });
        let CodexPollResult::Success { windows } = parse_codex_response(&response).unwrap() else {
            panic!("expected success")
        };
        assert_eq!(
            (windows[0].used_pct, windows[0].window_duration_mins),
            (42.0, Some(90))
        );
    }

    #[test]
    fn test_codex_response_legacy_fallback_and_optional_secondary() {
        let response = serde_json::json!({ "result": { "rateLimitsByLimitId": { "other": {} }, "rateLimits": {
            "primary": { "usedPercent": 12.0, "resetsAt": "2026-07-18T10:00:00Z" }
        } } });
        let CodexPollResult::Success { windows } = parse_codex_response(&response).unwrap() else {
            panic!("expected success")
        };
        assert_eq!(windows.len(), 1);
        assert_eq!(
            windows[0].reset_time_text.as_deref(),
            Some("2026-07-18T10:00:00Z")
        );
        assert_eq!(windows[0].kind, CodexWindowKind::Primary);
    }

    #[test]
    fn test_codex_response_ignores_credits_spend_histories_and_reset_credits() {
        let response = serde_json::json!({ "result": { "rateLimitsByLimitId": { "codex": {
            "primary": { "usedPercent": 12.0 }, "credits": { "usedPercent": 99.0 }, "spend": { "usedPercent": 88.0 },
            "tokenHistory": { "usedPercent": 77.0 }, "resetCredits": { "usedPercent": 66.0 }
        } } } });
        let CodexPollResult::Success { windows } = parse_codex_response(&response).unwrap() else {
            panic!("expected success")
        };
        assert_eq!(windows.len(), 1);
    }

    #[test]
    fn test_poll_delay_no_failures_uses_base_interval() {
        assert_eq!(next_poll_delay_secs(300, 0), 300);
    }

    #[test]
    fn test_poll_delay_doubles_per_failure() {
        assert_eq!(next_poll_delay_secs(300, 1), 600);
        assert_eq!(next_poll_delay_secs(300, 2), 1200);
    }

    #[test]
    fn test_poll_delay_caps_at_max_multiplier() {
        assert_eq!(next_poll_delay_secs(300, 3), 2400);
        assert_eq!(next_poll_delay_secs(300, 10), 2400);
    }

    #[test]
    fn test_pacing_underusing() {
        assert_eq!(compute_pacing(10.0, 50.0, 1.0, 1.0), Pacing::Underusing);
    }

    #[test]
    fn test_pacing_on_pace() {
        assert_eq!(compute_pacing(50.0, 50.0, 1.0, 1.0), Pacing::OnPace);
        assert_eq!(compute_pacing(50.5, 50.0, 1.0, 1.0), Pacing::OnPace);
        assert_eq!(compute_pacing(49.5, 50.0, 1.0, 1.0), Pacing::OnPace);
    }

    #[test]
    fn test_pacing_overusing() {
        assert_eq!(compute_pacing(70.0, 50.0, 1.0, 1.0), Pacing::Overusing);
    }

    #[test]
    fn test_pacing_boundaries() {
        assert_eq!(compute_pacing(49.0, 50.0, 1.0, 1.0), Pacing::OnPace);
        assert_eq!(compute_pacing(51.0, 50.0, 1.0, 1.0), Pacing::OnPace);
        assert_eq!(compute_pacing(48.0, 50.0, 1.0, 1.0), Pacing::Underusing);
        assert_eq!(compute_pacing(52.0, 50.0, 1.0, 1.0), Pacing::Overusing);
    }

    #[test]
    fn test_pacing_100_percent_before_reset_overusing() {
        assert_eq!(compute_pacing(100.0, 50.0, 1.0, 1.0), Pacing::Overusing);
    }

    #[test]
    fn test_pacing_100_percent_at_reset_time_on_pace() {
        assert_eq!(compute_pacing(100.0, 100.0, 1.0, 1.0), Pacing::OnPace);
    }

    #[test]
    fn test_pacing_non_default_thresholds() {
        assert_eq!(
            compute_pacing(45.0, 50.0, 5.0, 10.0),
            Pacing::OnPace,
            "diff -5, under=5 → OnPace (diff >= -5)"
        );
        assert_eq!(
            compute_pacing(44.0, 50.0, 5.0, 10.0),
            Pacing::Underusing,
            "diff -6, under=5 → Underusing"
        );
        assert_eq!(
            compute_pacing(60.0, 50.0, 5.0, 10.0),
            Pacing::OnPace,
            "diff +10, over=10 → OnPace (diff <= 10)"
        );
        assert_eq!(
            compute_pacing(61.0, 50.0, 5.0, 10.0),
            Pacing::Overusing,
            "diff +11, over=10 → Overusing"
        );
    }

    #[test]
    fn test_deepseek_off_peak_morning() {
        let now = beijing_time(8, 0, 0);
        let config = Config::default();
        let (status, info) = compute_deepseek_status(&now, &config);
        assert_eq!(status, DeepSeekStatus::OffPeak);
        assert!(info.unwrap().contains("Next peak"));
    }

    #[test]
    fn test_deepseek_peak_first_window() {
        let now = beijing_time(10, 0, 0);
        let config = Config::default();
        let (status, info) = compute_deepseek_status(&now, &config);
        assert_eq!(
            status,
            DeepSeekStatus::Peak {
                window_label: "09:00–12:00 BJT".into()
            }
        );
        assert!(info.unwrap().contains("Peak ends"));
    }

    #[test]
    fn test_deepseek_peak_second_window() {
        let now = beijing_time(16, 30, 0);
        let config = Config::default();
        let (status, info) = compute_deepseek_status(&now, &config);
        assert_eq!(
            status,
            DeepSeekStatus::Peak {
                window_label: "14:00–18:00 BJT".into()
            }
        );
        assert!(info.unwrap().contains("Peak ends"));
    }

    #[test]
    fn test_deepseek_off_peak_afternoon_gap() {
        let now = beijing_time(13, 0, 0);
        let config = Config::default();
        let (status, info) = compute_deepseek_status(&now, &config);
        assert_eq!(status, DeepSeekStatus::OffPeak);
        assert!(info.unwrap().contains("Next peak"));
    }

    #[test]
    fn test_deepseek_boundary_start_exact() {
        let config = Config::default();
        let now = beijing_time(9, 0, 0);
        let (status, _) = compute_deepseek_status(&now, &config);
        assert_eq!(
            status,
            DeepSeekStatus::Peak {
                window_label: "09:00–12:00 BJT".into()
            }
        );
    }

    #[test]
    fn test_deepseek_boundary_end_exact() {
        let config = Config::default();
        let now = beijing_time(12, 0, 0);
        let (status, _) = compute_deepseek_status(&now, &config);
        assert_eq!(status, DeepSeekStatus::OffPeak);
    }

    #[test]
    fn test_deepseek_notification_on_transition_to_peak() {
        let config = Config::default();
        let prev = DisplayState {
            deepseek_status: DeepSeekStatus::OffPeak,
            ..Default::default()
        };
        let now = beijing_time(10, 0, 0);

        let (_display, events) =
            poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev, false, None);

        assert_eq!(events.len(), 1);
        match &events[0] {
            NotificationEvent::DeepSeekPeakStarted { window_label } => {
                assert_eq!(window_label, "09:00–12:00 BJT");
            }
            _ => panic!("Expected DeepSeekPeakStarted"),
        }
    }

    #[test]
    fn test_deepseek_notification_on_transition_to_offpeak() {
        let config = Config::default();
        let prev = DisplayState {
            deepseek_status: DeepSeekStatus::Peak {
                window_label: "09:00–12:00 BJT".into(),
            },
            ..Default::default()
        };
        let now = beijing_time(13, 0, 0);

        let (_display, events) =
            poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev, false, None);

        assert_eq!(events.len(), 1);
        match &events[0] {
            NotificationEvent::DeepSeekPeakEnded => {}
            _ => panic!("Expected DeepSeekPeakEnded"),
        }
    }

    #[test]
    fn test_deepseek_no_notification_when_state_unchanged() {
        let config = Config::default();
        let prev = DisplayState {
            deepseek_status: DeepSeekStatus::OffPeak,
            ..Default::default()
        };
        let now = beijing_time(13, 0, 0);

        let (_display, events) =
            poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev, false, None);

        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_different_local_timezone_detects_peak_correctly() {
        let now = FixedOffset::west_opt(5 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 7, 13, 21, 0, 0)
            .unwrap();
        let config = Config::default();
        let (status, _) = compute_deepseek_status(&now, &config);
        assert_eq!(
            status,
            DeepSeekStatus::Peak {
                window_label: "09:00–12:00 BJT".into()
            }
        );
    }

    #[test]
    fn test_weekly_pacing_calculation() {
        let week_reset = "Jul 18, 4:00am (Asia/Jakarta)";
        let local_offset = FixedOffset::east_opt(7 * 3600).unwrap();
        let reset_dt = parse_reset_datetime(week_reset, local_offset, 2026).unwrap();

        let window_start = reset_dt - Duration::days(7);

        let now = window_start + Duration::hours(24);
        let total_secs = (reset_dt - window_start).num_seconds();
        let elapsed_secs = (now - window_start).num_seconds();
        let elapsed_pct = (elapsed_secs as f64 / total_secs as f64) * 100.0;

        assert!((elapsed_pct - 14.2857).abs() < 0.1);
    }

    #[test]
    fn test_parse_reset_datetime_format() {
        let local_offset = FixedOffset::east_opt(7 * 3600).unwrap();
        let result = parse_reset_datetime("Jul 13, 8:40pm (Asia/Jakarta)", local_offset, 2026);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 13);
        assert_eq!(dt.hour(), 20);
        assert_eq!(dt.minute(), 40);
    }

    #[test]
    fn test_parse_reset_datetime_whole_hour_no_minutes() {
        let local_offset = FixedOffset::east_opt(7 * 3600).unwrap();
        let result = parse_reset_datetime("Jul 18, 4am (Asia/Jakarta)", local_offset, 2026);
        assert!(result.is_some(), "should parse '4am' without minutes");
        let dt = result.unwrap();
        assert_eq!(dt.hour(), 4);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_reset_datetime_without_timezone() {
        let local_offset = FixedOffset::east_opt(7 * 3600).unwrap();
        let result = parse_reset_datetime("Jul 18, 4:00am (UTC)", local_offset, 2026);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 7);
        assert_eq!(dt.day(), 18);
        assert_eq!(dt.hour(), 4);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_rects_overlap_full_containment() {
        let a = Rect {
            x: 100,
            y: 100,
            width: 240,
            height: 185,
        };
        let b = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(rects_overlap(&a, &b));
        assert!(rects_overlap(&b, &a));
    }

    #[test]
    fn test_rects_no_overlap_separate() {
        let a = Rect {
            x: -2000,
            y: -2000,
            width: 240,
            height: 185,
        };
        let b = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(!rects_overlap(&a, &b));
    }

    #[test]
    fn test_rects_overlap_partial_edge() {
        let a = Rect {
            x: 1800,
            y: 100,
            width: 240,
            height: 185,
        };
        let b = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(rects_overlap(&a, &b));
    }

    #[test]
    fn test_rects_touching_edge_no_overlap() {
        let a = Rect {
            x: 1920,
            y: 0,
            width: 240,
            height: 185,
        };
        let b = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(!rects_overlap(&a, &b));
    }

    #[test]
    fn test_is_position_visible_fully_on_one_monitor() {
        let window = Rect {
            x: 100,
            y: 100,
            width: 240,
            height: 185,
        };
        let monitors = vec![Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert!(is_position_visible(&window, &monitors));
    }

    #[test]
    fn test_is_position_visible_fully_off_all_monitors() {
        let window = Rect {
            x: -2000,
            y: -2000,
            width: 240,
            height: 185,
        };
        let monitors = vec![Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert!(!is_position_visible(&window, &monitors));
    }

    #[test]
    fn test_is_position_visible_partially_overlapping_edge() {
        let window = Rect {
            x: 1800,
            y: 100,
            width: 240,
            height: 185,
        };
        let monitors = vec![Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert!(is_position_visible(&window, &monitors));
    }

    #[test]
    fn test_is_position_visible_spanning_two_adjacent_monitors() {
        let window = Rect {
            x: 1900,
            y: 100,
            width: 240,
            height: 185,
        };
        let monitors = vec![
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Rect {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];
        assert!(is_position_visible(&window, &monitors));
    }

    #[test]
    fn test_is_position_visible_empty_monitor_list() {
        let window = Rect {
            x: 100,
            y: 100,
            width: 240,
            height: 185,
        };
        let monitors: Vec<Rect> = vec![];
        assert!(!is_position_visible(&window, &monitors));
    }

    #[test]
    fn test_clamp_window_position_preserves_position_when_it_fits() {
        assert_eq!(
            clamp_window_position(
                100,
                200,
                240,
                185,
                &Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                }
            ),
            (100, 200)
        );
    }

    #[test]
    fn test_clamp_window_position_keeps_expanded_widget_on_screen() {
        assert_eq!(
            clamp_window_position(
                1800,
                900,
                240,
                420,
                &Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                }
            ),
            (1680, 660)
        );
    }

    #[test]
    fn test_clamp_window_position_handles_window_taller_than_monitor() {
        assert_eq!(
            clamp_window_position(
                10,
                20,
                240,
                1200,
                &Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                }
            ),
            (10, 0)
        );
    }
}
