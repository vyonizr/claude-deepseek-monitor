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

pub struct AppState {
    pub poll_cycle_state: poll_cycle::DisplayState,
}

impl AppState {
    fn config(&self) -> poll_cycle::Config {
        poll_cycle::Config::default()
    }
}

fn emit_state_update(app: &tauri::AppHandle, state: &poll_cycle::DisplayState) {
    let payload = serde_json::json!({
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
    });
    let _ = app.emit("state-update", payload);
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

fn run_poll_cycle(app: &tauri::AppHandle) {
    let raw_text = std::process::Command::new("claude")
        .args(["--print", "/usage"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string());

    let local_now = chrono::Local::now();
    let fixed_offset = *local_now.offset();
    let current_time = local_now.with_timezone(&fixed_offset);

    let state_container = app.state::<Mutex<AppState>>();
    let mut state_lock = state_container.lock().unwrap();

    let (new_state, events) = poll_cycle::poll_cycle(
        raw_text.as_deref(),
        &current_time,
        &state_lock.config(),
        &state_lock.poll_cycle_state,
    );

    let display_state = new_state.clone();
    state_lock.poll_cycle_state = new_state;
    drop(state_lock);
    drop(state_container);

    fire_notifications(app, &events);
    emit_state_update(app, &display_state);
}

#[tauri::command]
fn get_settings() -> serde_json::Value {
    serde_json::json!({
        "deepseek_windows": [
            { "start_hour": 9, "end_hour": 12, "label": "09:00–12:00 BJT" },
            { "start_hour": 14, "end_hour": 18, "label": "14:00–18:00 BJT" }
        ],
        "auto_launch": true
    })
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
        .invoke_handler(tauri::generate_handler![get_settings])
        .setup(|app| {
            let icon_image = Image::from_bytes(include_bytes!("../icons/32x32.png"))
                .expect("failed to decode tray icon");

            let settings_item =
                tauri::menu::MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let quit_item =
                tauri::menu::MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;

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
