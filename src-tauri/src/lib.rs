// ABOUTME: Main library for PingZilla - a menu bar ping monitor
// ABOUTME: Handles ping service, system tray, storage, and notifications

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

/// A single ping measurement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub timestamp: DateTime<Utc>,
    pub latency_ms: Option<f64>,
    pub target: String,
}

/// Application state shared across the app
pub struct AppState {
    pub ping_history: Mutex<VecDeque<PingResult>>,
    pub ping_target: Mutex<String>,
    pub notification_threshold_ms: Mutex<u32>,
    pub last_notification: Mutex<Option<DateTime<Utc>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            ping_history: Mutex::new(VecDeque::with_capacity(1000)),
            ping_target: Mutex::new("8.8.8.8".to_string()),
            notification_threshold_ms: Mutex::new(400),
            last_notification: Mutex::new(None),
        }
    }
}

/// Get current ping value
#[tauri::command]
async fn get_current_ping(state: State<'_, Arc<AppState>>) -> Result<Option<PingResult>, String> {
    let history = state.ping_history.lock().await;
    Ok(history.back().cloned())
}

/// Get ping history
#[tauri::command]
async fn get_ping_history(state: State<'_, Arc<AppState>>) -> Result<Vec<PingResult>, String> {
    let history = state.ping_history.lock().await;
    Ok(history.iter().cloned().collect())
}

/// Set ping target
#[tauri::command]
async fn set_ping_target(target: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut ping_target = state.ping_target.lock().await;
    *ping_target = target;
    Ok(())
}

/// Set notification threshold
#[tauri::command]
async fn set_notification_threshold(
    threshold_ms: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut threshold = state.notification_threshold_ms.lock().await;
    *threshold = threshold_ms;
    Ok(())
}

/// Get current settings
#[tauri::command]
async fn get_settings(state: State<'_, Arc<AppState>>) -> Result<(String, u32), String> {
    let target = state.ping_target.lock().await.clone();
    let threshold = *state.notification_threshold_ms.lock().await;
    Ok((target, threshold))
}

/// Perform a single ping using system ping command (no root needed)
async fn do_ping(target: &str) -> Option<f64> {
    use std::process::Command;

    let output = Command::new("ping")
        .args(["-c", "1", "-W", "2000", target])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse "time=12.345 ms" from output
    for line in stdout.lines() {
        if let Some(time_idx) = line.find("time=") {
            let time_str = &line[time_idx + 5..];
            if let Some(ms_idx) = time_str.find(" ms") {
                if let Ok(ms) = time_str[..ms_idx].parse::<f64>() {
                    return Some(ms);
                }
            }
        }
    }

    None
}

/// Start the ping service background task
fn start_ping_service(app_handle: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut save_counter = 0u32;

        loop {
            let target = state.ping_target.lock().await.clone();
            let latency_ms = do_ping(&target).await;

            let result = PingResult {
                timestamp: Utc::now(),
                latency_ms,
                target: target.clone(),
            };

            {
                let mut history = state.ping_history.lock().await;
                history.push_back(result.clone());
                while history.len() > 43200 {
                    history.pop_front();
                }
            }

            let tray_title = match latency_ms {
                Some(ms) => format!("{:.0}ms", ms),
                None => "---".to_string(),
            };

            if let Some(tray) = app_handle.tray_by_id("main-tray") {
                let _ = tray.set_title(Some(&tray_title));
            }

            let _ = app_handle.emit("ping-update", &result);

            if let Some(ms) = latency_ms {
                let threshold = *state.notification_threshold_ms.lock().await;
                if ms > threshold as f64 {
                    let mut last_notif = state.last_notification.lock().await;
                    let should_notify = match *last_notif {
                        Some(last) => Utc::now().signed_duration_since(last).num_seconds() > 60,
                        None => true,
                    };

                    if should_notify {
                        *last_notif = Some(Utc::now());
                        let _ = app_handle
                            .notification()
                            .builder()
                            .title("PingZilla Alert")
                            .body(format!("High latency detected: {:.0}ms", ms))
                            .show();
                    }
                }
            }

            save_counter += 1;
            if save_counter >= 30 {
                save_counter = 0;
                let history = state.ping_history.lock().await;
                let _ = save_history(&history);
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// Save history to disk
fn save_history(history: &VecDeque<PingResult>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(data_dir) = dirs::data_dir() {
        let app_dir = data_dir.join("pingzilla");
        std::fs::create_dir_all(&app_dir)?;
        let file_path = app_dir.join("history.json");
        let json = serde_json::to_string(history)?;
        std::fs::write(file_path, json)?;
    }
    Ok(())
}

/// Load history from disk
fn load_history() -> VecDeque<PingResult> {
    if let Some(data_dir) = dirs::data_dir() {
        let file_path = data_dir.join("pingzilla").join("history.json");
        if let Ok(json) = std::fs::read_to_string(file_path) {
            if let Ok(history) = serde_json::from_str::<VecDeque<PingResult>>(&json) {
                let cutoff = Utc::now() - chrono::Duration::hours(24);
                return history
                    .into_iter()
                    .filter(|r| r.timestamp > cutoff)
                    .collect();
            }
        }
    }
    VecDeque::new()
}

/// Position window below tray icon (macOS)
fn position_window_at_tray(window: &tauri::WebviewWindow, tray_rect: tauri::Rect) {
    let scale = window.scale_factor().unwrap_or(2.0);

    // Get window size in logical pixels
    let window_size = window.outer_size().unwrap_or(tauri::PhysicalSize {
        width: 320,
        height: 400,
    });
    let window_width = (window_size.width as f64 / scale) as i32;

    // Get tray position - the rect gives us physical coordinates
    let tray_x = match tray_rect.position {
        tauri::Position::Physical(p) => (p.x as f64 / scale) as i32,
        tauri::Position::Logical(l) => l.x as i32,
    };
    let tray_y = match tray_rect.position {
        tauri::Position::Physical(p) => (p.y as f64 / scale) as i32,
        tauri::Position::Logical(l) => l.y as i32,
    };
    let tray_width = match tray_rect.size {
        tauri::Size::Physical(p) => (p.width as f64 / scale) as i32,
        tauri::Size::Logical(l) => l.width as i32,
    };
    let tray_height = match tray_rect.size {
        tauri::Size::Physical(p) => (p.height as f64 / scale) as i32,
        tauri::Size::Logical(l) => l.height as i32,
    };

    // Center window under tray icon
    let x = tray_x + (tray_width / 2) - (window_width / 2);
    let y = tray_y + tray_height + 5;

    let _ = window.set_position(tauri::LogicalPosition::new(x, y));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let loaded_history = load_history();

    let app_state = Arc::new(AppState {
        ping_history: Mutex::new(loaded_history),
        ..Default::default()
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            get_current_ping,
            get_ping_history,
            set_ping_target,
            set_notification_threshold,
            get_settings,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let quit = MenuItem::with_id(app, "quit", "Quit PingZilla", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;

            let icon_bytes = include_bytes!("../icons/32x32.png");
            let icon = Image::from_bytes(icon_bytes)?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .icon_as_template(true)
                .title("...")
                .tooltip("PingZilla - Network Monitor (Right-click to quit)")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    let app = tray.app_handle();

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                position_window_at_tray(&window, rect);
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            start_ping_service(app.handle().clone(), app_state.clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
