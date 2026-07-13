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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            deepseek_windows: vec![
                DeepSeekWindow { start_hour: 9, end_hour: 12 },
                DeepSeekWindow { start_hour: 14, end_hour: 18 },
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayState {
    pub session_used_pct: Option<f64>,
    pub session_reset_time_text: Option<String>,
    pub session_pacing: Option<Pacing>,
    pub session_window_start: Option<String>,
    pub week_used_pct: Option<f64>,
    pub week_reset_time_text: Option<String>,
    pub week_pacing: Option<Pacing>,
    pub deepseek_status: DeepSeekStatus,
    pub next_transition_info: Option<String>,
    pub stale: bool,
    pub diagnostic: Option<String>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            session_used_pct: None,
            session_reset_time_text: None,
            session_pacing: None,
            session_window_start: None,
            week_used_pct: None,
            week_reset_time_text: None,
            week_pacing: None,
            deepseek_status: DeepSeekStatus::OffPeak,
            next_transition_info: None,
            stale: false,
            diagnostic: None,
        }
    }
}

impl DisplayState {
    pub fn new_diagnostic(msg: impl Into<String>) -> Self {
        Self {
            diagnostic: Some(msg.into()),
            ..Default::default()
        }
    }
}

const PACING_THRESHOLD: f64 = 15.0;

fn parse_claude_usage_text(text: &str) -> Option<(f64, String, f64, String)> {
    let mut session_pct: Option<f64> = None;
    let mut session_reset: Option<String> = None;
    let mut week_pct: Option<f64> = None;
    let mut week_reset: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Current session:") {
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

    match (session_pct, session_reset, week_pct, week_reset) {
        (Some(sp), Some(sr), Some(wp), Some(wr)) => Some((sp, sr, wp, wr)),
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
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

fn parse_reset_datetime(reset_text: &str, local_offset: FixedOffset, current_year: i32) -> Option<DateTime<FixedOffset>> {
    let without_tz = reset_text.split(" (").next()?;
    let clean = without_tz.trim();

    // Split on comma: "Jul 18, 4am" → date="Jul 18", time_part="4am"
    let (date_str, time_str) = {
        let comma = clean.find(',')?;
        (&clean[..comma], clean[comma + 1..].trim())
    };

    let naive_date = NaiveDate::parse_from_str(
        &format!("{} {}", date_str, current_year),
        "%b %d %Y",
    ).ok()?;

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

    let hour_24 = if is_pm && hour != 12 { hour + 12 }
        else if !is_pm && hour == 12 { 0 }
        else { hour };

    let naive = naive_date.and_hms_opt(hour_24, minute, 0)?;
    naive.and_local_timezone(local_offset).single()
}

fn compute_pacing(used_pct: f64, elapsed_pct: f64) -> Pacing {
    let diff = used_pct - elapsed_pct;
    if diff < -PACING_THRESHOLD {
        Pacing::Underusing
    } else if diff > PACING_THRESHOLD {
        Pacing::Overusing
    } else {
        Pacing::OnPace
    }
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

fn time_until_next_transition(bj_dt: &DateTime<FixedOffset>, config: &Config) -> Option<(String, bool)> {
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
            Some((info, true)) => {
                (
                    DeepSeekStatus::OffPeak,
                    Some(format!("Next peak in {}", info)),
                )
            }
            Some((info, false)) => {
                (
                    DeepSeekStatus::OffPeak,
                    Some(format!("Off-peak until {}", info)),
                )
            }
            None => (DeepSeekStatus::OffPeak, None),
        }
    }
}

pub fn poll_cycle(
    raw_usage_text: Option<&str>,
    current_time: &DateTime<FixedOffset>,
    config: &Config,
    previous_state: &DisplayState,
) -> (DisplayState, Vec<NotificationEvent>) {
    let parsed = raw_usage_text.and_then(parse_claude_usage_text);

    let (mut display, usage_ok) = if let Some((sp, sr, wp, wr)) = parsed {
        let local_offset = *current_time.offset();
        let current_year = current_time.year();
        let session_reset_dt = parse_reset_datetime(&sr, local_offset, current_year);
        let week_reset_dt = parse_reset_datetime(&wr, local_offset, current_year);

        let session_start_str = if previous_state.session_reset_time_text.as_deref() != Some(&sr) {
            Some(current_time.to_rfc3339())
        } else {
            previous_state.session_window_start.clone()
        };

        let session_window_start = session_start_str.as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());

        let session_pacing = match (session_window_start, session_reset_dt) {
            (Some(start), Some(reset)) if start < reset => {
                let total_dur = reset - start;
                let elapsed = *current_time - start;
                if total_dur.num_seconds() > 0 {
                    let elapsed_pct = (elapsed.num_seconds() as f64 / total_dur.num_seconds() as f64) * 100.0;
                    Some(compute_pacing(sp, elapsed_pct))
                } else {
                    previous_state.session_pacing.clone()
                }
            }
            _ => previous_state.session_pacing.clone(),
        };

        let week_pacing = match week_reset_dt {
            Some(reset) => {
                let window_start = reset - Duration::days(7);
                let total_dur = reset - window_start;
                let elapsed = *current_time - window_start;
                if total_dur.num_seconds() > 0 {
                    let elapsed_pct = (elapsed.num_seconds() as f64 / total_dur.num_seconds() as f64) * 100.0;
                    let elapsed_pct = elapsed_pct.clamp(0.0, 100.0);
                    Some(compute_pacing(wp, elapsed_pct))
                } else {
                    previous_state.week_pacing.clone()
                }
            }
            _ => previous_state.week_pacing.clone(),
        };

        let display = DisplayState {
            session_used_pct: Some(sp),
            session_reset_time_text: Some(sr),
            session_pacing,
            session_window_start: session_start_str,
            week_used_pct: Some(wp),
            week_reset_time_text: Some(wr),
            week_pacing,
            ..Default::default()
        };

        (display, true)
    } else {
        let mut display = previous_state.clone();
        display.stale = raw_usage_text.is_some();
        // carry diagnostic forward instead of clearing it
        (display, false)
    };

    if usage_ok {
        display.stale = false;
    } else if raw_usage_text.is_none() {
        display.stale = true;
    }

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
        assert_eq!(wp, 14.0, "week pct should be 14 (all models), got {wp} — Fable line overwrote it");
        assert!(wr.contains("Jul 18"), "week reset should be Jul 18, got '{wr}'");
    }

    #[test]
    fn test_parse_malformed_text_returns_none() {
        let result = parse_claude_usage_text("garbage output that doesn't match");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_partial_text_returns_none() {
        let result = parse_claude_usage_text("Current session: 8% used \u{00b7} resets Jul 13, 8:40pm (Asia/Jakarta)");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_text_with_missing_percentage() {
        let result = parse_claude_usage_text("Current session: used \u{00b7} resets Jul 13, 8:40pm (Asia/Jakarta)\nCurrent week (all models): 13% used \u{00b7} resets Jul 18, 4am (Asia/Jakarta)");
        assert!(result.is_none());
    }

    #[test]
    fn test_poll_cycle_with_valid_usage() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState::default();

        let (display, _events) = poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev);

        assert!(!display.stale);
        assert_eq!(display.session_used_pct, Some(8.0));
        assert_eq!(display.week_used_pct, Some(13.0));
        assert!(display.session_reset_time_text.unwrap().contains("Jul 13, 8:40pm"));
    }

    #[test]
    fn test_poll_cycle_with_none_text_marks_stale() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState::default();

        let (display, _events) = poll_cycle(None, &now, &config, &prev);

        assert!(display.stale);
        assert_eq!(display.session_used_pct, None);
    }

    #[test]
    fn test_poll_cycle_with_malformed_text_uses_previous() {
        let now = make_time(14, 0, 0);
        let config = Config::default();
        let prev = DisplayState {
            session_used_pct: Some(8.0),
            session_reset_time_text: Some("Jul 13, 8:40pm (Asia/Jakarta)".into()),
            week_used_pct: Some(13.0),
            week_reset_time_text: Some("Jul 18, 4am (Asia/Jakarta)".into()),
            ..Default::default()
        };

        let (display, _events) = poll_cycle(Some("garbage malformed text"), &now, &config, &prev);

        assert!(display.stale);
        assert_eq!(display.session_used_pct, Some(8.0));
        assert_eq!(display.week_used_pct, Some(13.0));
    }

    #[test]
    fn test_pacing_underusing() {
        assert_eq!(compute_pacing(10.0, 50.0), Pacing::Underusing);
    }

    #[test]
    fn test_pacing_on_pace() {
        assert_eq!(compute_pacing(45.0, 50.0), Pacing::OnPace);
        assert_eq!(compute_pacing(55.0, 50.0), Pacing::OnPace);
        assert_eq!(compute_pacing(50.0, 50.0), Pacing::OnPace);
    }

    #[test]
    fn test_pacing_overusing() {
        assert_eq!(compute_pacing(70.0, 50.0), Pacing::Overusing);
    }

    #[test]
    fn test_pacing_boundaries() {
        assert_eq!(compute_pacing(35.0, 50.0), Pacing::OnPace);
        assert_eq!(compute_pacing(65.0, 50.0), Pacing::OnPace);
        assert_eq!(compute_pacing(34.0, 50.0), Pacing::Underusing);
        assert_eq!(compute_pacing(66.0, 50.0), Pacing::Overusing);
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
        assert_eq!(status, DeepSeekStatus::Peak { window_label: "09:00–12:00 BJT".into() });
        assert!(info.unwrap().contains("Peak ends"));
    }

    #[test]
    fn test_deepseek_peak_second_window() {
        let now = beijing_time(16, 30, 0);
        let config = Config::default();
        let (status, info) = compute_deepseek_status(&now, &config);
        assert_eq!(status, DeepSeekStatus::Peak { window_label: "14:00–18:00 BJT".into() });
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
        assert_eq!(status, DeepSeekStatus::Peak { window_label: "09:00–12:00 BJT".into() });
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

        let (_display, events) = poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev);

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
            deepseek_status: DeepSeekStatus::Peak { window_label: "09:00–12:00 BJT".into() },
            ..Default::default()
        };
        let now = beijing_time(13, 0, 0);

        let (_display, events) = poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev);

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

        let (_display, events) = poll_cycle(Some(VALID_USAGE_TEXT), &now, &config, &prev);

        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_different_local_timezone_detects_peak_correctly() {
        let now = FixedOffset::west_opt(5 * 3600).unwrap()
            .with_ymd_and_hms(2026, 7, 13, 21, 0, 0)
            .unwrap();
        let config = Config::default();
        let (status, _) = compute_deepseek_status(&now, &config);
        assert_eq!(status, DeepSeekStatus::Peak { window_label: "09:00–12:00 BJT".into() });
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
}
