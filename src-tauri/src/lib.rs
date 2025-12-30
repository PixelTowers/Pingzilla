// ABOUTME: Main library for PingZilla - a menu bar ping monitor
// ABOUTME: Handles ping service, system tray, storage, and notifications

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
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

/// Statistics for a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingStatistics {
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub packet_loss_pct: f64,
    pub total_pings: usize,
    pub failed_pings: usize,
}

/// Menu bar display mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DisplayMode {
    IconOnly,
    IconAndPing,
    PingOnly,
}

/// Application state shared across the app
pub struct AppState {
    pub ping_history: Mutex<HashMap<String, VecDeque<PingResult>>>,
    pub targets: Mutex<Vec<String>>,
    pub primary_target: Mutex<String>,
    pub notification_threshold_ms: Mutex<u32>,
    pub last_notification: Mutex<Option<DateTime<Utc>>>,
    pub display_mode: Mutex<DisplayMode>,
}

impl Default for AppState {
    fn default() -> Self {
        let mut history = HashMap::new();
        history.insert("1.1.1.1".to_string(), VecDeque::with_capacity(1000));
        Self {
            ping_history: Mutex::new(history),
            targets: Mutex::new(vec!["1.1.1.1".to_string()]),
            primary_target: Mutex::new("1.1.1.1".to_string()),
            notification_threshold_ms: Mutex::new(400),
            last_notification: Mutex::new(None),
            display_mode: Mutex::new(DisplayMode::IconAndPing),
        }
    }
}

/// Get current ping value for a target (defaults to primary)
#[tauri::command]
async fn get_current_ping(
    target: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PingResult>, String> {
    let target = match target {
        Some(t) => t,
        None => state.primary_target.lock().await.clone(),
    };
    let history = state.ping_history.lock().await;
    Ok(history.get(&target).and_then(|h| h.back().cloned()))
}

/// Get ping history for a target (defaults to primary)
#[tauri::command]
async fn get_ping_history(
    target: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<PingResult>, String> {
    let target = match target {
        Some(t) => t,
        None => state.primary_target.lock().await.clone(),
    };
    let history = state.ping_history.lock().await;
    Ok(history
        .get(&target)
        .map(|h| h.iter().cloned().collect())
        .unwrap_or_default())
}

/// Get all targets
#[tauri::command]
async fn get_targets(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let targets = state.targets.lock().await;
    Ok(targets.clone())
}

/// Add a new target
#[tauri::command]
async fn add_target(target: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut targets = state.targets.lock().await;
    if !targets.contains(&target) {
        targets.push(target.clone());
        let mut history = state.ping_history.lock().await;
        history.insert(target, VecDeque::with_capacity(1000));
    }
    Ok(())
}

/// Remove a target
#[tauri::command]
async fn remove_target(target: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut targets = state.targets.lock().await;
    if targets.len() <= 1 {
        return Err("Cannot remove the last target".to_string());
    }
    targets.retain(|t| t != &target);

    let mut history = state.ping_history.lock().await;
    history.remove(&target);

    let mut primary = state.primary_target.lock().await;
    if *primary == target {
        *primary = targets.first().cloned().unwrap_or_default();
    }
    Ok(())
}

/// Set primary target (shown in tray)
#[tauri::command]
async fn set_primary_target(target: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let targets = state.targets.lock().await;
    if !targets.contains(&target) {
        return Err("Target not found".to_string());
    }
    drop(targets);

    let mut primary = state.primary_target.lock().await;
    *primary = target;
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
async fn get_settings(state: State<'_, Arc<AppState>>) -> Result<(String, u32, String), String> {
    let target = state.primary_target.lock().await.clone();
    let threshold = *state.notification_threshold_ms.lock().await;
    let display_mode = state.display_mode.lock().await.clone();
    let mode_str = match display_mode {
        DisplayMode::IconOnly => "icon_only",
        DisplayMode::IconAndPing => "icon_and_ping",
        DisplayMode::PingOnly => "ping_only",
    };
    Ok((target, threshold, mode_str.to_string()))
}

/// Set display mode and update tray immediately
#[tauri::command]
async fn set_display_mode(
    mode: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let display_mode = match mode.as_str() {
        "icon_only" => DisplayMode::IconOnly,
        "icon_and_ping" => DisplayMode::IconAndPing,
        "ping_only" => DisplayMode::PingOnly,
        _ => return Err("Invalid display mode".to_string()),
    };

    // Update state
    {
        let mut current_mode = state.display_mode.lock().await;
        *current_mode = display_mode.clone();
    }

    // Get current ping for primary target to update tray immediately
    let primary_target = state.primary_target.lock().await.clone();
    let history = state.ping_history.lock().await;
    let current_ping = history
        .get(&primary_target)
        .and_then(|h| h.back())
        .and_then(|r| r.latency_ms);

    // Update tray immediately based on display mode
    if let Some(tray) = app_handle.tray_by_id("main-tray") {
        let ping_text = match current_ping {
            Some(ms) => format!("{:.0}ms", ms),
            None => "---".to_string(),
        };

        // Load Godzilla icons based on latency
        let icon_happy = include_bytes!("../icons/pingzilla_happy.png");
        let icon_angry = include_bytes!("../icons/pinzilla_angry.png");
        let icon_sad = include_bytes!("../icons/pingzilla_sad.png");
        let icon_dead = include_bytes!("../icons/pingzilla_dead.png");
        let transparent_bytes = include_bytes!("../icons/transparent.png");

        // Choose icon based on latency
        let status_icon = match current_ping {
            Some(ms) if ms < 60.0 => icon_happy.as_slice(),
            Some(ms) if ms < 150.0 => icon_angry.as_slice(),
            Some(_) => icon_sad.as_slice(),
            None => icon_dead.as_slice(),
        };

        match display_mode {
            DisplayMode::IconOnly => {
                // Show icon, hide text
                if let Ok(icon) = Image::from_bytes(status_icon) {
                    let _ = tray.set_icon(Some(icon));
                    let _ = tray.set_icon_as_template(true);
                }
                let _ = tray.set_title(Some(""));
            }
            DisplayMode::IconAndPing => {
                // Show both icon and ping text
                if let Ok(icon) = Image::from_bytes(status_icon) {
                    let _ = tray.set_icon(Some(icon));
                    let _ = tray.set_icon_as_template(true);
                }
                let _ = tray.set_title(Some(&ping_text));
            }
            DisplayMode::PingOnly => {
                // Hide icon, show only ping text
                if let Ok(icon) = Image::from_bytes(transparent_bytes) {
                    let _ = tray.set_icon(Some(icon));
                    let _ = tray.set_icon_as_template(true);
                }
                let _ = tray.set_title(Some(&ping_text));
            }
        }
    }

    Ok(())
}

/// Get statistics for a target over a time period
#[tauri::command]
async fn get_statistics(
    target: Option<String>,
    minutes: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<PingStatistics, String> {
    let target = match target {
        Some(t) => t,
        None => state.primary_target.lock().await.clone(),
    };
    let minutes = minutes.unwrap_or(5);
    let cutoff = Utc::now() - chrono::Duration::minutes(minutes as i64);

    let history = state.ping_history.lock().await;
    let pings: Vec<&PingResult> = history
        .get(&target)
        .map(|h| h.iter().filter(|p| p.timestamp > cutoff).collect())
        .unwrap_or_default();

    let total_pings = pings.len();
    let failed_pings = pings.iter().filter(|p| p.latency_ms.is_none()).count();
    let successful: Vec<f64> = pings.iter().filter_map(|p| p.latency_ms).collect();

    let (min_ms, max_ms, avg_ms) = if successful.is_empty() {
        (None, None, None)
    } else {
        let min = successful.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = successful.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let avg = successful.iter().sum::<f64>() / successful.len() as f64;
        (Some(min), Some(max), Some(avg))
    };

    let packet_loss_pct = if total_pings > 0 {
        (failed_pings as f64 / total_pings as f64) * 100.0
    } else {
        0.0
    };

    Ok(PingStatistics {
        min_ms,
        max_ms,
        avg_ms,
        packet_loss_pct,
        total_pings,
        failed_pings,
    })
}

/// Perform a TCP connect to measure latency (works in App Sandbox)
async fn do_tcp_ping(target: &str, port: u16) -> Option<f64> {
    use std::time::Instant;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    // For IP addresses, connect directly. For hostnames, try to resolve first.
    let addr = if target.parse::<std::net::IpAddr>().is_ok() {
        format!("{}:{}", target, port)
    } else {
        format!("{}:{}", target, port)
    };

    let start = Instant::now();

    match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => Some(start.elapsed().as_secs_f64() * 1000.0),
        _ => None,
    }
}

/// Perform a single ping using system ping command (no root needed)
/// Uses tokio::process::Command for async execution with timeout to prevent
/// blocking the runtime if sandbox denies ping execution
async fn do_system_ping(target: &str) -> Option<f64> {
    use tokio::process::Command;
    use tokio::time::timeout;

    // 3-second timeout - if sandbox blocks ping, we move on quickly to TCP fallback
    let result = timeout(Duration::from_secs(3), async {
        let output = Command::new("ping")
            .args(["-c", "1", "-W", "2000", target])
            .output()
            .await
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
    })
    .await;

    result.ok().flatten()
}

/// Perform a ping with automatic fallback to TCP if system ping fails
/// This ensures the app works in the App Sandbox
async fn do_ping(target: &str) -> Option<f64> {
    // Try system ping first (more accurate ICMP timing)
    if let Some(ms) = do_system_ping(target).await {
        return Some(ms);
    }

    // Fallback to TCP connect measurement (works in sandbox)
    // Try DNS port first (works for DNS servers like 1.1.1.1)
    if let Some(ms) = do_tcp_ping(target, 53).await {
        return Some(ms);
    }

    // Then try HTTPS and HTTP ports (works for web servers)
    if let Some(ms) = do_tcp_ping(target, 443).await {
        return Some(ms);
    }

    do_tcp_ping(target, 80).await
}

/// Start the ping service background task - pings all targets
fn start_ping_service(app_handle: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut save_counter = 0u32;

        loop {
            let targets = state.targets.lock().await.clone();
            let primary_target = state.primary_target.lock().await.clone();

            for target in &targets {
                let latency_ms = do_ping(target).await;

                let result = PingResult {
                    timestamp: Utc::now(),
                    latency_ms,
                    target: target.clone(),
                };

                {
                    let mut history = state.ping_history.lock().await;
                    let target_history = history
                        .entry(target.clone())
                        .or_insert_with(|| VecDeque::with_capacity(1000));
                    target_history.push_back(result.clone());
                    while target_history.len() > 43200 {
                        target_history.pop_front();
                    }
                }

                // Update tray only for primary target
                if target == &primary_target {
                    let display_mode = state.display_mode.lock().await.clone();

                    if let Some(tray) = app_handle.tray_by_id("main-tray") {
                        let ping_text = match latency_ms {
                            Some(ms) => format!("{:.0}ms", ms),
                            None => "---".to_string(),
                        };

                        // Load Godzilla icons based on latency
                        let icon_happy = include_bytes!("../icons/pingzilla_happy.png");
                        let icon_angry = include_bytes!("../icons/pinzilla_angry.png");
                        let icon_sad = include_bytes!("../icons/pingzilla_sad.png");
                        let icon_dead = include_bytes!("../icons/pingzilla_dead.png");
                        let transparent_bytes = include_bytes!("../icons/transparent.png");

                        // Choose icon based on latency
                        let status_icon = match latency_ms {
                            Some(ms) if ms < 60.0 => icon_happy.as_slice(),
                            Some(ms) if ms < 150.0 => icon_angry.as_slice(),
                            Some(_) => icon_sad.as_slice(),
                            None => icon_dead.as_slice(),
                        };

                        match display_mode {
                            DisplayMode::IconOnly => {
                                // Show icon, hide text
                                if let Ok(icon) = Image::from_bytes(status_icon) {
                                    let _ = tray.set_icon(Some(icon));
                                    let _ = tray.set_icon_as_template(true);
                                }
                                let _ = tray.set_title(Some(""));
                            }
                            DisplayMode::IconAndPing => {
                                // Show both icon and ping text
                                if let Ok(icon) = Image::from_bytes(status_icon) {
                                    let _ = tray.set_icon(Some(icon));
                                    let _ = tray.set_icon_as_template(true);
                                }
                                let _ = tray.set_title(Some(&ping_text));
                            }
                            DisplayMode::PingOnly => {
                                // Hide icon, show only ping text
                                if let Ok(icon) = Image::from_bytes(transparent_bytes) {
                                    let _ = tray.set_icon(Some(icon));
                                    let _ = tray.set_icon_as_template(true);
                                }
                                let _ = tray.set_title(Some(&ping_text));
                            }
                        }
                    }
                }

                let _ = app_handle.emit("ping-update", &result);

                // Notifications for primary target only
                if target == &primary_target {
                    if let Some(ms) = latency_ms {
                        let threshold = *state.notification_threshold_ms.lock().await;
                        if ms > threshold as f64 {
                            let mut last_notif = state.last_notification.lock().await;
                            let should_notify = match *last_notif {
                                Some(last) => {
                                    Utc::now().signed_duration_since(last).num_seconds() > 60
                                }
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
                }
            }

            save_counter += 1;
            if save_counter >= 30 {
                save_counter = 0;
                let history = state.ping_history.lock().await;
                let targets = state.targets.lock().await;
                let primary = state.primary_target.lock().await;
                let _ = save_history(&history, &targets, &primary);
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// Saved data structure for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedData {
    history: HashMap<String, VecDeque<PingResult>>,
    targets: Vec<String>,
    primary_target: String,
    notification_threshold_ms: u32,
}

/// Save history to disk
fn save_history(
    history: &HashMap<String, VecDeque<PingResult>>,
    targets: &[String],
    primary_target: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(data_dir) = dirs::data_dir() {
        let app_dir = data_dir.join("pingzilla");
        std::fs::create_dir_all(&app_dir)?;
        let file_path = app_dir.join("history_v2.json");
        let data = SavedData {
            history: history.clone(),
            targets: targets.to_vec(),
            primary_target: primary_target.to_string(),
            notification_threshold_ms: 400,
        };
        let json = serde_json::to_string(&data)?;
        std::fs::write(file_path, json)?;
    }
    Ok(())
}

/// Load history from disk
fn load_history() -> (HashMap<String, VecDeque<PingResult>>, Vec<String>, String) {
    if let Some(data_dir) = dirs::data_dir() {
        // Try new format first
        let file_path_v2 = data_dir.join("pingzilla").join("history_v2.json");
        if let Ok(json) = std::fs::read_to_string(&file_path_v2) {
            if let Ok(data) = serde_json::from_str::<SavedData>(&json) {
                let cutoff = Utc::now() - chrono::Duration::hours(24);
                let filtered_history: HashMap<String, VecDeque<PingResult>> = data
                    .history
                    .into_iter()
                    .map(|(target, pings)| {
                        let filtered: VecDeque<PingResult> =
                            pings.into_iter().filter(|r| r.timestamp > cutoff).collect();
                        (target, filtered)
                    })
                    .collect();
                return (filtered_history, data.targets, data.primary_target);
            }
        }

        // Fall back to old format for migration
        let file_path = data_dir.join("pingzilla").join("history.json");
        if let Ok(json) = std::fs::read_to_string(file_path) {
            if let Ok(history) = serde_json::from_str::<VecDeque<PingResult>>(&json) {
                let cutoff = Utc::now() - chrono::Duration::hours(24);
                let filtered: VecDeque<PingResult> = history
                    .into_iter()
                    .filter(|r| r.timestamp > cutoff)
                    .collect();
                let target = filtered
                    .front()
                    .map(|r| r.target.clone())
                    .unwrap_or_else(|| "1.1.1.1".to_string());
                let mut map = HashMap::new();
                map.insert(target.clone(), filtered);
                return (map, vec![target.clone()], target);
            }
        }
    }

    let mut history = HashMap::new();
    history.insert("1.1.1.1".to_string(), VecDeque::new());
    (history, vec!["1.1.1.1".to_string()], "1.1.1.1".to_string())
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
    let (loaded_history, loaded_targets, loaded_primary) = load_history();

    let app_state = Arc::new(AppState {
        ping_history: Mutex::new(loaded_history),
        targets: Mutex::new(loaded_targets),
        primary_target: Mutex::new(loaded_primary),
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
            get_targets,
            add_target,
            remove_target,
            set_primary_target,
            set_notification_threshold,
            get_settings,
            get_statistics,
            set_display_mode,
        ])
        .setup(move |app| {
            // Show in Dock - required for ping to work in sandboxed App Store builds
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            let quit = MenuItem::with_id(app, "quit", "Quit PingZilla", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;

            // Start with happy Godzilla icon (will update based on ping latency)
            let icon_bytes = include_bytes!("../icons/pingzilla_happy.png");
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
