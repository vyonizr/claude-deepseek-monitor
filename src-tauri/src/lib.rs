pub mod poll_cycle;

use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::Menu,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_autostart::ManagerExt;

pub struct AppState {
    pub poll_cycle_state: poll_cycle::DisplayState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SavedSettings {
    deepseek_windows: Vec<WindowConfig>,
    auto_launch: bool,
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
    }
}

fn to_json(state: &poll_cycle::DisplayState) -> serde_json::Value {
    serde_json::json!({
        "session_pct": state.session_used_pct.map(|v| format!("{:.0}%", v)),
        "session_reset": state.session_reset_time_text,
        "session_pacing": state.session_pacing.as_ref().map(|p| match p {
            poll_cycle::Pacing::Underusing => "under",
            poll_cycle::Pacing::OnPace => "onpace",
            poll_cycle::Pacing::Overusing => "over",
        }),
        "week_pct": state.week_used_pct.map(|v| format!("{:.0}%", v)),
        "week_reset": state.week_reset_time_text,
        "week_pacing": state.week_pacing.as_ref().map(|p| match p {
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
        "stale": state.stale,
        "diagnostic": state.diagnostic,
    })
}

fn emit_state_update(app: &tauri::AppHandle, state: &poll_cycle::DisplayState) {
    let _ = app.emit("state-update", to_json(state));
}

#[tauri::command]
fn get_initial_state(app: tauri::AppHandle) -> serde_json::Value {
    let state = app.state::<Mutex<AppState>>();
    let state = state.lock().unwrap();
    to_json(&state.poll_cycle_state)
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
            match std::process::Command::new(name)
                .args(["--print", "/usage"])
                .output()
            {
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

fn run_poll_cycle(app: &tauri::AppHandle) {
    eprintln!("[monitor] run_poll_cycle starting");

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

    let local_now = chrono::Local::now();
    let fixed_offset = *local_now.offset();
    let current_time = local_now.with_timezone(&fixed_offset);

    let config = settings_to_config(&load_settings(app));

    eprintln!("[monitor] running poll_cycle()");
    let state_container = app.state::<Mutex<AppState>>();
    let mut state_lock = state_container.lock().unwrap();

    let (mut new_state, events) = poll_cycle::poll_cycle(
        raw_text.as_deref(),
        &current_time,
        &config,
        &state_lock.poll_cycle_state,
    );

    eprintln!("[monitor] poll_cycle done, events={}, stale={}, session_pct={:?}, week_pct={:?}",
        events.len(), new_state.stale, new_state.session_used_pct, new_state.week_used_pct);

    // If the subprocess ran but parsing failed, show the raw output as diagnostic
    if let Some(ref text) = raw_text {
        if new_state.stale && new_state.session_used_pct.is_none() {
            let preview: String = text.chars().take(200).collect();
            eprintln!("[monitor] PARSE FAILED. Raw output (200 chars):\n---\n{preview}\n---");
            new_state.diagnostic = Some(format!("Unexpected output: {preview}"));
        }
    }

    if let Some(diag) = cmd_diagnostic {
        new_state.diagnostic = Some(diag);
    }

    let display_state = new_state.clone();
    state_lock.poll_cycle_state = new_state;
    drop(state_lock);
    drop(state_container);

    eprintln!("[monitor] firing notifications and emitting state");
    fire_notifications(app, &events);
    emit_state_update(app, &display_state);
    eprintln!("[monitor] run_poll_cycle done");
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

    let saved = SavedSettings {
        deepseek_windows: windows,
        auto_launch,
    };

    save_settings_to_disk(&app, &saved);

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
        .manage(Mutex::new(AppState {
            poll_cycle_state: poll_cycle::DisplayState::default(),
        }))
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
                        .inner_size(320.0, 280.0)
                        .resizable(false)
                        .build();
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Run initial poll
            run_poll_cycle(app.handle());

            // Start polling loop
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(300));
                    run_poll_cycle(&handle);
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
            }
        });
}
