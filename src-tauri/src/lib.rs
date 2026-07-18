pub mod poll_cycle;

use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::Menu,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_autostart::ManagerExt;

pub struct AppState {
    pub poll_cycle_state: poll_cycle::DisplayState,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SavedSettings {
    deepseek_windows: Vec<WindowConfig>,
    auto_launch: bool,
    #[serde(default = "default_poll_interval_minutes")]
    poll_interval_minutes: u32,
    #[serde(default = "default_under_pace_threshold")]
    under_pace_threshold: u32,
    #[serde(default = "default_over_pace_threshold")]
    over_pace_threshold: u32,
    #[serde(default = "default_widget_opacity")]
    widget_opacity: f64,
}

fn default_under_pace_threshold() -> u32 {
    1
}

fn default_over_pace_threshold() -> u32 {
    1
}

fn default_poll_interval_minutes() -> u32 {
    5
}

fn default_widget_opacity() -> f64 {
    0.92
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WindowConfig {
    start_hour: u8,
    end_hour: u8,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            deepseek_windows: vec![
                WindowConfig { start_hour: 9, end_hour: 12 },
                WindowConfig { start_hour: 14, end_hour: 18 },
            ],
            auto_launch: true,
            poll_interval_minutes: default_poll_interval_minutes(),
            under_pace_threshold: default_under_pace_threshold(),
            over_pace_threshold: default_over_pace_threshold(),
            widget_opacity: default_widget_opacity(),
        }
    }
}

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

fn load_settings(app: &tauri::AppHandle) -> SavedSettings {
    let path = match settings_path(app) {
        Ok(p) => p,
        Err(_) => return SavedSettings::default(),
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings_to_disk(app: &tauri::AppHandle, settings: &SavedSettings) {
    if let Ok(path) = settings_path(app) {
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(&path, &json);
        }
    }
}

fn settings_to_config(settings: &SavedSettings) -> poll_cycle::Config {
    poll_cycle::Config {
        deepseek_windows: settings.deepseek_windows.iter().map(|w| {
            poll_cycle::DeepSeekWindow {
                start_hour: w.start_hour,
                end_hour: w.end_hour,
            }
        }).collect(),
        under_pace_threshold: settings.under_pace_threshold as f64,
        over_pace_threshold: settings.over_pace_threshold as f64,
    }
}

fn to_json(state: &poll_cycle::DisplayState, app: &tauri::AppHandle) -> serde_json::Value {
    let claude = &state.claude;
    let opacity = load_settings(app).widget_opacity;
    serde_json::json!({
        "session_pct": claude.session_used_pct.map(|v| format!("{:.0}%", v)),
        "session_reset": claude.session_reset_time_text,
        "session_pacing": claude.session_pacing.as_ref().map(|p| match p {
            poll_cycle::Pacing::Underusing => "under",
            poll_cycle::Pacing::OnPace => "onpace",
            poll_cycle::Pacing::Overusing => "over",
        }),
        "week_pct": claude.week_used_pct.map(|v| format!("{:.0}%", v)),
        "week_reset": claude.week_reset_time_text,
        "week_pacing": claude.week_pacing.as_ref().map(|p| match p {
            poll_cycle::Pacing::Underusing => "under",
            poll_cycle::Pacing::OnPace => "onpace",
            poll_cycle::Pacing::Overusing => "over",
        }),
        "deepseek_peak": matches!(state.deepseek_status, poll_cycle::DeepSeekStatus::Peak { .. }),
        "deepseek_label": match &state.deepseek_status {
            poll_cycle::DeepSeekStatus::Peak { window_label } => Some(window_label),
            _ => None,
        },
        "next_transition": state.next_transition_info,
        "stale": claude.stale,
        "diagnostic": claude.diagnostic,
        "widget_opacity": opacity,
    })
}

fn emit_state_update(app: &tauri::AppHandle, state: &poll_cycle::DisplayState) {
    let _ = app.emit("state-update", to_json(state, app));
}

#[tauri::command]
fn get_initial_state(app: tauri::AppHandle) -> serde_json::Value {
    let state = app.state::<Mutex<AppState>>();
    let state = state.lock().unwrap();
    to_json(&state.poll_cycle_state, &app)
}

fn fire_notifications(
    app: &tauri::AppHandle,
    events: &[poll_cycle::NotificationEvent],
) {
    for event in events {
        match event {
            poll_cycle::NotificationEvent::DeepSeekPeakStarted { window_label } => {
                let _ = app.notification()
                    .builder()
                    .title("DeepSeek Peak Pricing Started")
                    .body(format!("2× pricing window {} is now active. Requests will cost double.", window_label))
                    .show();
            }
            poll_cycle::NotificationEvent::DeepSeekPeakEnded => {
                let _ = app.notification()
                    .builder()
                    .title("DeepSeek Peak Pricing Ended")
                    .body("Standard pricing has resumed. Requests are back to normal rates.")
                    .show();
            }
        }
    }
}

fn run_claude_command() -> Result<String, String> {
    let cmd = if cfg!(target_os = "windows") {
        // On Windows npm installs CLI wrappers as .cmd files;
        // try claude.cmd first, fall back to bare claude.
        let candidates = ["claude.cmd", "claude"];
        let mut last_err = "claude not found on PATH".to_string();
        for name in &candidates {
            let mut command = std::process::Command::new(name);
            command.args(["--print", "/usage"]);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            match command.output() {
                Ok(out) if out.status.success() => {
                    return String::from_utf8(out.stdout)
                        .map(|s| s.trim().to_string())
                        .map_err(|e| format!("claude output is not UTF-8: {e}"));
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    last_err = format!("{name} exited with code {:?}: {stderr}", out.status.code());
                }
                Err(e) => {
                    last_err = format!("{name}: {e}");
                }
            }
        }
        Err(last_err)
    } else {
        match std::process::Command::new("claude")
            .args(["--print", "/usage"])
            .output()
        {
            Ok(out) if out.status.success() => {
                String::from_utf8(out.stdout)
                    .map(|s| s.trim().to_string())
                    .map_err(|e| format!("claude output is not UTF-8: {e}"))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(format!("claude exited with code {:?}: {stderr}", out.status.code()))
            }
            Err(e) => Err(format!("claude: {e}")),
        }
    };

    cmd
}

/// Runs one poll cycle and returns whether it succeeded (used by the poll
/// loop to decide whether to apply backoff before the next attempt).
fn run_poll_cycle(app: &tauri::AppHandle) -> bool {
    eprintln!("[monitor] run_poll_cycle starting");

    let local_now = chrono::Local::now();
    let fixed_offset = *local_now.offset();
    let current_time = local_now.with_timezone(&fixed_offset);

    let config = settings_to_config(&load_settings(app));

    let state_container = app.state::<Mutex<AppState>>();
    let mut state_lock = state_container.lock().unwrap();

    // The claude CLI occasionally returns a malformed/incomplete response on its
    // first invocation right after the app cold-starts (auth/session warmup),
    // then succeeds immediately after. Retry once before surfacing a diagnostic,
    // rather than showing a scary error for a whole poll interval.
    const MAX_ATTEMPTS: u32 = 2;
    let mut attempt = 1;
    let (mut new_state, events, raw_text, cmd_diagnostic) = loop {
        let (raw_text, cmd_diagnostic) = match run_claude_command() {
            Ok(text) => {
                eprintln!("[monitor] claude OK, {} bytes received", text.len());
                (Some(text), None)
            }
            Err(diag) => {
                eprintln!("[monitor] claude FAIL: {diag}");
                (None, Some(diag))
            }
        };

        eprintln!("[monitor] running poll_cycle() (attempt {attempt})");
        let (new_state, events) = poll_cycle::poll_cycle(
            raw_text.as_deref(),
            &current_time,
            &config,
            &state_lock.poll_cycle_state,
        );

        eprintln!("[monitor] poll_cycle done, events={}, stale={}, session_pct={:?}, week_pct={:?}",
            events.len(), new_state.claude.stale, new_state.claude.session_used_pct, new_state.claude.week_used_pct);

        let parse_failed = raw_text.is_some()
            && new_state.claude.stale
            && new_state.claude.session_used_pct.is_none();

        if parse_failed && attempt < MAX_ATTEMPTS {
            eprintln!("[monitor] parse failed on attempt {attempt}, retrying claude command");
            attempt += 1;
            continue;
        }

        break (new_state, events, raw_text, cmd_diagnostic);
    };

    // If the subprocess ran but parsing failed, show the raw output as diagnostic
    if let Some(ref text) = raw_text {
        if new_state.claude.stale && new_state.claude.session_used_pct.is_none() {
            let preview: String = text.chars().take(200).collect();
            eprintln!("[monitor] PARSE FAILED. Raw output (200 chars):\n---\n{preview}\n---");
            new_state.claude.diagnostic = Some(format!("Unexpected output: {preview}"));
        }
    }

    if let Some(diag) = cmd_diagnostic {
        new_state.claude.diagnostic = Some(diag);
    }

    let succeeded = new_state.claude.diagnostic.is_none();
    state_lock.consecutive_failures = if succeeded {
        0
    } else {
        state_lock.consecutive_failures + 1
    };

    let display_state = new_state.clone();
    state_lock.poll_cycle_state = new_state;
    drop(state_lock);
    drop(state_container);

    eprintln!("[monitor] firing notifications and emitting state");
    fire_notifications(app, &events);
    emit_state_update(app, &display_state);
    eprintln!("[monitor] run_poll_cycle done");
    succeeded
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> serde_json::Value {
    let settings = load_settings(&app);
    serde_json::json!({
        "deepseek_windows": settings.deepseek_windows.iter().map(|w| {
            serde_json::json!({
                "start_hour": w.start_hour,
                "end_hour": w.end_hour,
                "label": poll_cycle::deepseek_window_label(&poll_cycle::DeepSeekWindow {
                    start_hour: w.start_hour,
                    end_hour: w.end_hour,
                }),
            })
        }).collect::<Vec<_>>(),
        "auto_launch": settings.auto_launch,
        "poll_interval_minutes": settings.poll_interval_minutes,
        "under_pace_threshold": settings.under_pace_threshold,
        "over_pace_threshold": settings.over_pace_threshold,
        "widget_opacity": settings.widget_opacity,
    })
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: serde_json::Value) -> Result<(), String> {
    let auto_launch = settings.get("auto_launch").and_then(|v| v.as_bool()).unwrap_or(true);
    let windows = settings.get("deepseek_windows").and_then(|v| v.as_array()).map(|arr| {
        arr.iter().filter_map(|w| {
            Some(WindowConfig {
                start_hour: w.get("start_hour")?.as_u64()? as u8,
                end_hour: w.get("end_hour")?.as_u64()? as u8,
            })
        }).collect::<Vec<_>>()
    }).unwrap_or_default();
    let poll_interval_minutes = settings.get("poll_interval_minutes")
        .and_then(|v| v.as_u64())
        .map(|v| (v as u32).max(1))
        .unwrap_or_else(default_poll_interval_minutes);

    let under_pace_threshold = settings.get("under_pace_threshold")
        .and_then(|v| v.as_u64())
        .map(|v| (v as u32).clamp(1, 20))
        .unwrap_or_else(default_under_pace_threshold);

    let over_pace_threshold = settings.get("over_pace_threshold")
        .and_then(|v| v.as_u64())
        .map(|v| (v as u32).clamp(1, 20))
        .unwrap_or_else(default_over_pace_threshold);

    let widget_opacity = settings.get("widget_opacity")
        .and_then(|v| v.as_f64())
        .map(|v| v.clamp(0.3, 1.0))
        .unwrap_or_else(default_widget_opacity);

    let saved = SavedSettings {
        deepseek_windows: windows,
        auto_launch,
        poll_interval_minutes,
        under_pace_threshold,
        over_pace_threshold,
        widget_opacity,
    };

    save_settings_to_disk(&app, &saved);

    {
        let state = app.state::<Mutex<AppState>>();
        let current = state.lock().unwrap().poll_cycle_state.clone();
        emit_state_update(&app, &current);
    }

    {
        let autostart = app.autolaunch();
        let current = autostart.is_enabled().unwrap_or(false);
        if auto_launch && !current {
            let _ = autostart.enable();
        } else if !auto_launch && current {
            let _ = autostart.disable();
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_window_state::Builder::default()
            .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
            .with_denylist(&["settings"])
            .build())
        .manage(Mutex::new(AppState {
            poll_cycle_state: poll_cycle::DisplayState::default(),
            consecutive_failures: 0,
        }))
        .manage(Mutex::new(None::<PollThreadControl>))
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, get_initial_state])
        .setup(|app| {
            let icon_image = Image::from_bytes(include_bytes!("../icons/32x32.png"))
                .expect("failed to decode tray icon");

            let settings_item =
                tauri::menu::MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let quit_item =
                tauri::menu::MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;

            // Apply autostart from saved settings
            let handle = app.handle();
            let saved = load_settings(handle);
            if saved.auto_launch {
                let autostart = handle.autolaunch();
                let _ = autostart.enable();
            }

            TrayIconBuilder::new()
                .icon(icon_image)
                .tooltip("Claude / DeepSeek Monitor")
                .menu(&menu)
                .on_tray_icon_event(move |tray, event| {
                    let app = tray.app_handle();
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "settings" => {
                        let _ = WebviewWindowBuilder::new(
                            app,
                            "settings",
                            WebviewUrl::App("settings.html".into()),
                        )
                        .title("Settings")
                        .inner_size(320.0, 360.0)
                        .resizable(false)
                        .build();
                    }
                    "quit" => {
                        signal_poll_thread_shutdown(app);
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let monitors = app.available_monitors().unwrap_or_default();
                if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
                    let window_rect = poll_cycle::Rect {
                        x: pos.x,
                        y: pos.y,
                        width: size.width,
                        height: size.height,
                    };
                    let monitor_rects: Vec<poll_cycle::Rect> = monitors.iter().map(|m| {
                        let mpos = m.position();
                        let msize = m.size();
                        poll_cycle::Rect {
                            x: mpos.x,
                            y: mpos.y,
                            width: msize.width,
                            height: msize.height,
                        }
                    }).collect();
                    if !poll_cycle::is_position_visible(&window_rect, &monitor_rects) {
                        if let Some(primary) = app.primary_monitor().unwrap_or(None) {
                            let ppos = primary.position();
                            let psize = primary.size();
                            let margin = 20i32;
                            let new_x = (ppos.x + psize.width as i32 - 240i32 - margin).max(ppos.x);
                            let new_y = ppos.y + margin;
                            let _ = window.set_position(PhysicalPosition::new(new_x, new_y));
                        }
                    }
                }
            }

            // Run initial poll
            run_poll_cycle(app.handle());

            // Start polling loop. A shutdown channel lets us interrupt the
            // sleep immediately on quit instead of waiting out the (possibly
            // backed-off) interval. The done channel lets the shutdown
            // trigger block briefly for confirmation that the loop actually
            // exited, rather than firing-and-forgetting the signal while the
            // process dies out from under the thread anyway.
            let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
            let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
            *app.state::<Mutex<Option<PollThreadControl>>>()
                .lock()
                .unwrap() = Some(PollThreadControl { shutdown_tx, done_rx });

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    let interval_minutes = load_settings(&handle).poll_interval_minutes.max(1);
                    let base_secs = interval_minutes as u64 * 60;
                    let failures = handle
                        .state::<Mutex<AppState>>()
                        .lock()
                        .unwrap()
                        .consecutive_failures;
                    let delay = poll_cycle::next_poll_delay_secs(base_secs, failures);

                    match shutdown_rx.recv_timeout(Duration::from_secs(delay)) {
                        Ok(()) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            run_poll_cycle(&handle);
                        }
                    }
                }
                eprintln!("[monitor] poll thread shutting down");
                let _ = done_tx.send(());
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                signal_poll_thread_shutdown(app_handle);
            }
        });
}

struct PollThreadControl {
    shutdown_tx: std::sync::mpsc::Sender<()>,
    done_rx: std::sync::mpsc::Receiver<()>,
}

/// Signals the poll thread to stop and blocks briefly (bounded, so a stuck
/// subprocess can't hang app exit) for confirmation that it actually did.
fn signal_poll_thread_shutdown(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<Option<PollThreadControl>>>();
    let guard = state.lock().unwrap();
    if let Some(control) = guard.as_ref() {
        let _ = control.shutdown_tx.send(());
        let _ = control.done_rx.recv_timeout(Duration::from_secs(2));
    }
}
