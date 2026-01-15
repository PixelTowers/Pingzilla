// ABOUTME: Main library for PingZilla - a menu bar ping monitor
// ABOUTME: Handles ping service, system tray, storage, and notifications

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, Wry,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

/// Method used to measure ping latency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PingMethod {
    Icmp,      // Real ICMP ping via system command
    TcpDns,    // TCP connect to port 53 (DNS)
    TcpHttps,  // TCP connect to port 443
    TcpHttp,   // TCP connect to port 80
}

/// A single ping measurement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub timestamp: DateTime<Utc>,
    pub latency_ms: Option<f64>,
    pub target: String,
    #[serde(default)]
    pub method: Option<PingMethod>,
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

/// User's public IP info (for VPN verification)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpInfo {
    pub ip: String,
    pub country: String,
    pub country_code: String,
    pub city: Option<String>,
    pub isp: Option<String>,
}

/// Site monitor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMonitor {
    pub url: String,
    pub name: Option<String>,
    pub enabled: bool,
}

/// Site status from monitoring check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteStatus {
    pub url: String,
    pub is_up: bool,
    pub latency_ms: Option<f64>,
    pub last_check: DateTime<Utc>,
    pub last_down: Option<DateTime<Utc>>,
}

/// Network change type for VPN drop detection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkChangeType {
    IpChanged,
    CountryChanged, // VPN likely dropped!
    IspChanged,
    Initial,
}

/// Network change event emitted when IP/country/ISP changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkChangeEvent {
    pub change_type: NetworkChangeType,
    pub previous: Option<IpInfo>,
    pub current: IpInfo,
    pub timestamp: DateTime<Utc>,
    pub is_expected: bool, // Manual refresh vs automatic
}

/// Network stability tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStability {
    pub changes_last_hour: u32,
    pub last_country_change: Option<DateTime<Utc>>,
    pub last_ip_change: Option<DateTime<Utc>>,
}

impl Default for NetworkStability {
    fn default() -> Self {
        Self {
            changes_last_hour: 0,
            last_country_change: None,
            last_ip_change: None,
        }
    }
}

/// VPN protection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnProtectionSettings {
    pub enabled: bool,
    pub check_interval_secs: u32,
    pub alert_on_country_change: bool,
    pub alert_on_ip_change: bool,
    pub expected_country: Option<String>,
}

impl Default for VpnProtectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 60, // 60s default to save battery
            alert_on_country_change: true,
            alert_on_ip_change: true,
            expected_country: None,
        }
    }
}

/// Cached tray state to avoid unnecessary updates
#[derive(Debug, Clone, PartialEq)]
pub struct TrayState {
    pub icon_type: TrayIconType,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrayIconType {
    Happy,
    Angry,
    Sad,
    Dead,
    Transparent,
}

/// Application state shared across the app
pub struct AppState {
    pub ping_history: Mutex<HashMap<String, VecDeque<PingResult>>>,
    pub targets: Mutex<Vec<String>>,
    pub primary_target: Mutex<String>,
    pub notification_threshold_ms: Mutex<u32>,
    pub last_notification: Mutex<Option<DateTime<Utc>>>,
    pub display_mode: Mutex<DisplayMode>,
    pub ip_info: Mutex<Option<IpInfo>>,
    pub ip_info_last_check: Mutex<Option<DateTime<Utc>>>,
    pub site_monitors: Mutex<Vec<SiteMonitor>>,
    pub site_statuses: Mutex<HashMap<String, SiteStatus>>,
    // VPN drop detection fields
    pub vpn_settings: Mutex<VpnProtectionSettings>,
    pub network_stability: Mutex<NetworkStability>,
    pub network_change_history: Mutex<VecDeque<NetworkChangeEvent>>,
    pub last_ip_check_was_manual: Mutex<bool>,
    pub last_vpn_notification: Mutex<Option<DateTime<Utc>>>,
    // Tray state cache to avoid unnecessary updates
    pub last_tray_state: Mutex<Option<TrayState>>,
    // Battery optimization: sleep/wake and visibility tracking
    pub is_system_sleeping: AtomicBool,
    pub is_window_visible: AtomicBool,
    // User-configurable ping interval (in seconds)
    pub ping_interval_secs: Mutex<u32>,
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
            ip_info: Mutex::new(None),
            ip_info_last_check: Mutex::new(None),
            site_monitors: Mutex::new(Vec::new()),
            site_statuses: Mutex::new(HashMap::new()),
            // VPN drop detection defaults
            vpn_settings: Mutex::new(VpnProtectionSettings::default()),
            network_stability: Mutex::new(NetworkStability::default()),
            network_change_history: Mutex::new(VecDeque::with_capacity(100)),
            last_ip_check_was_manual: Mutex::new(false),
            last_vpn_notification: Mutex::new(None),
            // Tray state cache
            last_tray_state: Mutex::new(None),
            // Battery optimization defaults
            is_system_sleeping: AtomicBool::new(false),
            is_window_visible: AtomicBool::new(false),
            // Default ping interval: 10 seconds
            ping_interval_secs: Mutex::new(10),
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

/// Get user's public IP info (for VPN verification)
/// Uses ip-api.com - free API, no key required, 45 req/min limit
/// Caches result for 5 minutes to avoid rate limiting
#[tauri::command]
async fn get_my_ip_info(
    force_refresh: Option<bool>,
    state: State<'_, Arc<AppState>>,
) -> Result<IpInfo, String> {
    let force = force_refresh.unwrap_or(false);

    // Check cache first (unless force refresh)
    if !force {
        let cached = state.ip_info.lock().await.clone();
        let last_check = *state.ip_info_last_check.lock().await;

        if let (Some(info), Some(checked_at)) = (cached, last_check) {
            // Cache for 5 minutes
            if Utc::now().signed_duration_since(checked_at).num_seconds() < 300 {
                return Ok(info);
            }
        }
    }

    // Fetch from API
    let resp = reqwest::get("http://ip-api.com/json/")
        .await
        .map_err(|e| format!("Failed to fetch IP info: {}", e))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse IP info: {}", e))?;

    let info = IpInfo {
        ip: data["query"].as_str().unwrap_or("Unknown").to_string(),
        country: data["country"].as_str().unwrap_or("Unknown").to_string(),
        country_code: data["countryCode"].as_str().unwrap_or("").to_string(),
        city: data["city"].as_str().map(String::from),
        isp: data["isp"].as_str().map(String::from),
    };

    // Update cache
    *state.ip_info.lock().await = Some(info.clone());
    *state.ip_info_last_check.lock().await = Some(Utc::now());

    Ok(info)
}

/// Fetch IP info directly from API (no caching, for VPN monitoring)
async fn fetch_ip_info_internal() -> Result<IpInfo, String> {
    let resp = reqwest::get("http://ip-api.com/json/")
        .await
        .map_err(|e| format!("Failed to fetch IP info: {}", e))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse IP info: {}", e))?;

    Ok(IpInfo {
        ip: data["query"].as_str().unwrap_or("Unknown").to_string(),
        country: data["country"].as_str().unwrap_or("Unknown").to_string(),
        country_code: data["countryCode"].as_str().unwrap_or("").to_string(),
        city: data["city"].as_str().map(String::from),
        isp: data["isp"].as_str().map(String::from),
    })
}

/// Detect network changes between previous and current IP info
fn detect_network_change(
    prev: &IpInfo,
    current: &IpInfo,
    is_manual: bool,
) -> Option<NetworkChangeEvent> {
    // Check for country change (VPN dropped!)
    if prev.country_code != current.country_code {
        return Some(NetworkChangeEvent {
            change_type: NetworkChangeType::CountryChanged,
            previous: Some(prev.clone()),
            current: current.clone(),
            timestamp: Utc::now(),
            is_expected: is_manual,
        });
    }

    // Check for IP change (same country)
    if prev.ip != current.ip {
        return Some(NetworkChangeEvent {
            change_type: NetworkChangeType::IpChanged,
            previous: Some(prev.clone()),
            current: current.clone(),
            timestamp: Utc::now(),
            is_expected: is_manual,
        });
    }

    // Check for ISP change only
    if prev.isp != current.isp {
        return Some(NetworkChangeEvent {
            change_type: NetworkChangeType::IspChanged,
            previous: Some(prev.clone()),
            current: current.clone(),
            timestamp: Utc::now(),
            is_expected: is_manual,
        });
    }

    None
}

/// Check if we should send a VPN notification based on settings and rate limiting
fn should_send_vpn_notification(
    change: &NetworkChangeEvent,
    settings: &VpnProtectionSettings,
    last_notification: Option<DateTime<Utc>>,
) -> bool {
    // Don't notify for manual refreshes
    if change.is_expected {
        return false;
    }

    // Rate limit: max 1 notification per 30 seconds
    if let Some(last) = last_notification {
        if Utc::now().signed_duration_since(last).num_seconds() < 30 {
            return false;
        }
    }

    match change.change_type {
        NetworkChangeType::CountryChanged => settings.alert_on_country_change,
        NetworkChangeType::IpChanged => settings.alert_on_ip_change,
        NetworkChangeType::IspChanged => false, // Never notify for ISP-only changes
        NetworkChangeType::Initial => false,
    }
}

/// Get VPN protection settings
#[tauri::command]
async fn get_vpn_settings(state: State<'_, Arc<AppState>>) -> Result<VpnProtectionSettings, String> {
    let settings = state.vpn_settings.lock().await;
    Ok(settings.clone())
}

/// Set VPN protection settings
#[tauri::command]
async fn set_vpn_settings(
    settings: VpnProtectionSettings,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    *state.vpn_settings.lock().await = settings;
    Ok(())
}

/// Get network stability info
#[tauri::command]
async fn get_network_stability(state: State<'_, Arc<AppState>>) -> Result<NetworkStability, String> {
    let stability = state.network_stability.lock().await;
    Ok(stability.clone())
}

/// Acknowledge and dismiss an IP change alert
#[tauri::command]
async fn acknowledge_ip_change(_state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // Mark the current IP as the new baseline (no action needed for basic impl)
    // The frontend will dismiss the alert banner
    Ok(())
}

/// Set window visibility for adaptive ping interval (battery optimization)
/// Called from frontend on focus/blur events
#[tauri::command]
async fn set_window_visible(visible: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.is_window_visible.store(visible, Ordering::Relaxed);
    Ok(())
}

/// Get ping interval setting (in seconds)
#[tauri::command]
async fn get_ping_interval(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    let interval = *state.ping_interval_secs.lock().await;
    Ok(interval)
}

/// Set ping interval (in seconds, min 5, max 120)
#[tauri::command]
async fn set_ping_interval(interval_secs: u32, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if interval_secs < 5 {
        return Err("Ping interval must be at least 5 seconds".to_string());
    }
    if interval_secs > 120 {
        return Err("Ping interval must be at most 120 seconds".to_string());
    }
    *state.ping_interval_secs.lock().await = interval_secs;
    Ok(())
}

/// Get all site monitors
#[tauri::command]
async fn get_site_monitors(state: State<'_, Arc<AppState>>) -> Result<Vec<SiteMonitor>, String> {
    let monitors = state.site_monitors.lock().await;
    Ok(monitors.clone())
}

/// Add a new site monitor (max 10)
#[tauri::command]
async fn add_site_monitor(
    url: String,
    name: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut monitors = state.site_monitors.lock().await;

    if monitors.len() >= 10 {
        return Err("Maximum of 10 site monitors allowed".to_string());
    }

    if monitors.iter().any(|m| m.url == url) {
        return Err("Site already being monitored".to_string());
    }

    monitors.push(SiteMonitor {
        url,
        name,
        enabled: true,
    });

    Ok(())
}

/// Remove a site monitor
#[tauri::command]
async fn remove_site_monitor(url: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut monitors = state.site_monitors.lock().await;
    monitors.retain(|m| m.url != url);

    let mut statuses = state.site_statuses.lock().await;
    statuses.remove(&url);

    Ok(())
}

/// Get current site statuses
#[tauri::command]
async fn get_site_statuses(
    state: State<'_, Arc<AppState>>,
) -> Result<HashMap<String, SiteStatus>, String> {
    let statuses = state.site_statuses.lock().await;
    Ok(statuses.clone())
}

/// Perform ICMP ping using surge-ping (true ICMP, no root required on macOS)
/// This uses the non-privileged SOCK_DGRAM + IPPROTO_ICMP socket facility
async fn do_icmp_ping(target: &str) -> Option<f64> {
    use std::net::IpAddr;
    use std::time::Instant;
    use surge_ping::{Client, Config, PingIdentifier, PingSequence};
    use tokio::time::timeout;

    // Resolve hostname to IP address
    let ip: IpAddr = if let Ok(ip) = target.parse() {
        ip
    } else {
        // DNS resolution for hostnames
        let addrs = tokio::net::lookup_host(format!("{}:0", target))
            .await
            .ok()?;
        addrs.into_iter().next()?.ip()
    };

    // Generate random identifier before async operations (ThreadRng is not Send)
    let identifier: u16 = rand::random();

    // Create surge-ping client with default config (tries DGRAM first, then RAW)
    let client = Client::new(&Config::default()).ok()?;
    let mut pinger = client.pinger(ip, PingIdentifier(identifier)).await;

    let start = Instant::now();

    // 2-second timeout for the ping itself, 3-second outer timeout
    match timeout(Duration::from_secs(3), pinger.ping(PingSequence(0), &[])).await {
        Ok(Ok((_, rtt))) => {
            // surge-ping returns the round-trip time directly
            Some(rtt.as_secs_f64() * 1000.0)
        }
        _ => {
            // Log for debugging (optional - helps identify sandbox blocks)
            let elapsed = start.elapsed();
            if elapsed > Duration::from_secs(2) {
                // Timed out - likely sandbox blocking
            }
            None
        }
    }
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

/// Perform a ping with automatic fallback to TCP if ICMP ping fails
/// This ensures the app works in the App Sandbox
/// Returns (latency_ms, method_used) tuple
async fn do_ping(target: &str) -> (Option<f64>, Option<PingMethod>) {
    // Try surge-ping ICMP first (true ICMP, may work in sandbox via DGRAM socket)
    if let Some(ms) = do_icmp_ping(target).await {
        return (Some(ms), Some(PingMethod::Icmp));
    }

    // Fallback to TCP connect measurement (guaranteed to work in sandbox)
    // Try DNS port first (works for DNS servers like 1.1.1.1)
    if let Some(ms) = do_tcp_ping(target, 53).await {
        return (Some(ms), Some(PingMethod::TcpDns));
    }

    // Then try HTTPS and HTTP ports (works for web servers)
    if let Some(ms) = do_tcp_ping(target, 443).await {
        return (Some(ms), Some(PingMethod::TcpHttps));
    }

    match do_tcp_ping(target, 80).await {
        Some(ms) => (Some(ms), Some(PingMethod::TcpHttp)),
        None => (None, None),
    }
}

/// Check if a site is up by connecting to it
/// Parses URL to determine host and port
async fn check_site(url: &str) -> SiteStatus {
    use std::time::Instant;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    let start = Instant::now();

    // Parse URL to extract host and port
    let (host, port) = if url.starts_with("https://") {
        (url.trim_start_matches("https://").split('/').next().unwrap_or(url), 443)
    } else if url.starts_with("http://") {
        (url.trim_start_matches("http://").split('/').next().unwrap_or(url), 80)
    } else {
        // Assume it's a hostname/IP, try HTTPS first
        (url.split('/').next().unwrap_or(url), 443)
    };

    // Remove port from host if included (e.g., "example.com:8080")
    let (host, port) = if let Some(idx) = host.find(':') {
        let custom_port = host[idx + 1..].parse().unwrap_or(port);
        (&host[..idx], custom_port)
    } else {
        (host, port)
    };

    let addr = format!("{}:{}", host, port);

    let result = timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await;

    match result {
        Ok(Ok(_)) => SiteStatus {
            url: url.to_string(),
            is_up: true,
            latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0),
            last_check: Utc::now(),
            last_down: None,
        },
        _ => SiteStatus {
            url: url.to_string(),
            is_up: false,
            latency_ms: None,
            last_check: Utc::now(),
            last_down: Some(Utc::now()),
        },
    }
}

/// Check all monitored sites and return whether any status changed
async fn check_all_sites(app_handle: &AppHandle, state: &Arc<AppState>) -> bool {
    let monitors = state.site_monitors.lock().await.clone();
    let mut any_changed = false;

    for monitor in monitors.iter().filter(|m| m.enabled) {
        // Check previous status
        let was_up = {
            let statuses = state.site_statuses.lock().await;
            statuses.get(&monitor.url).map(|s| s.is_up).unwrap_or(true)
        };

        // Check site
        let status = check_site(&monitor.url).await;
        let is_up = status.is_up;

        // Track if status changed
        if was_up != is_up {
            any_changed = true;
        }

        // Update status with last_down from previous if site is now up
        let final_status = if is_up {
            let prev_last_down = {
                let statuses = state.site_statuses.lock().await;
                statuses.get(&monitor.url).and_then(|s| s.last_down)
            };
            SiteStatus {
                last_down: prev_last_down,
                ..status
            }
        } else {
            status
        };

        // Send notification if site went down
        if was_up && !is_up {
            let site_name = monitor.name.as_deref().unwrap_or(&monitor.url);
            let _ = app_handle
                .notification()
                .builder()
                .title("Site Down Alert")
                .body(format!("{} is not responding", site_name))
                .show();
        }

        // Update status
        state
            .site_statuses
            .lock()
            .await
            .insert(monitor.url.clone(), final_status);
    }

    // Only emit event if something changed (saves CPU/battery)
    if any_changed {
        let statuses = state.site_statuses.lock().await.clone();
        let _ = app_handle.emit("site-status-update", &statuses);
    }

    any_changed
}

/// Check IP for VPN drop detection
async fn check_ip_change(app_handle: &AppHandle, state: &Arc<AppState>) {
    let settings = state.vpn_settings.lock().await.clone();

    // Skip if VPN protection is disabled
    if !settings.enabled {
        return;
    }

    // Fetch current IP (bypass cache)
    if let Ok(current) = fetch_ip_info_internal().await {
        let previous = state.ip_info.lock().await.clone();
        let is_manual = *state.last_ip_check_was_manual.lock().await;
        *state.last_ip_check_was_manual.lock().await = false;

        // Detect changes if we have a previous IP
        if let Some(prev) = &previous {
            if let Some(change) = detect_network_change(prev, &current, is_manual) {
                // Emit event to frontend
                let _ = app_handle.emit("network-change", &change);

                // Update network stability tracking
                {
                    let mut stability = state.network_stability.lock().await;
                    match change.change_type {
                        NetworkChangeType::CountryChanged => {
                            stability.last_country_change = Some(Utc::now());
                            stability.changes_last_hour += 1;
                        }
                        NetworkChangeType::IpChanged => {
                            stability.last_ip_change = Some(Utc::now());
                            stability.changes_last_hour += 1;
                        }
                        _ => {}
                    }
                }

                // Add to change history
                {
                    let mut history = state.network_change_history.lock().await;
                    history.push_back(change.clone());
                    // Keep last 100 changes
                    if history.len() > 100 {
                        history.pop_front();
                    }
                }

                // Send notification for critical changes
                let last_notif = *state.last_vpn_notification.lock().await;
                if should_send_vpn_notification(&change, &settings, last_notif) {
                    let (title, body) = match change.change_type {
                        NetworkChangeType::CountryChanged => (
                            "VPN Alert: Location Changed!",
                            format!(
                                "{} → {}. Check your VPN connection.",
                                prev.country, current.country
                            ),
                        ),
                        NetworkChangeType::IpChanged => (
                            "Network: IP Changed",
                            format!("Your IP address changed to {}", current.ip),
                        ),
                        _ => ("Network Change", "Network configuration changed".to_string()),
                    };

                    let _ = app_handle
                        .notification()
                        .builder()
                        .title(title)
                        .body(body)
                        .show();

                    *state.last_vpn_notification.lock().await = Some(Utc::now());
                }
            }
        }

        // Update current IP state
        *state.ip_info.lock().await = Some(current);
        *state.ip_info_last_check.lock().await = Some(Utc::now());
    }
}

/// Determine which icon type to use based on latency
fn get_icon_type_for_latency(latency_ms: Option<f64>) -> TrayIconType {
    match latency_ms {
        Some(ms) if ms < 60.0 => TrayIconType::Happy,
        Some(ms) if ms < 150.0 => TrayIconType::Angry,
        Some(_) => TrayIconType::Sad,
        None => TrayIconType::Dead,
    }
}

/// Update tray only if state has changed (saves CPU/battery)
fn update_tray_if_changed(
    tray: &tauri::tray::TrayIcon,
    new_state: &TrayState,
    last_state: &mut Option<TrayState>,
    display_mode: &DisplayMode,
    // Pre-loaded icons to avoid repeated PNG decoding
    icons: &TrayIcons,
) {
    // Skip update if nothing changed
    if let Some(ref last) = last_state {
        if last == new_state {
            return;
        }
    }

    // Get the right icon bytes
    let icon_bytes = match new_state.icon_type {
        TrayIconType::Happy => icons.happy,
        TrayIconType::Angry => icons.angry,
        TrayIconType::Sad => icons.sad,
        TrayIconType::Dead => icons.dead,
        TrayIconType::Transparent => icons.transparent,
    };

    match display_mode {
        DisplayMode::IconOnly => {
            if let Ok(icon) = Image::from_bytes(icon_bytes) {
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_icon_as_template(true);
            }
            let _ = tray.set_title(Some(""));
        }
        DisplayMode::IconAndPing => {
            if let Ok(icon) = Image::from_bytes(icon_bytes) {
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_icon_as_template(true);
            }
            let _ = tray.set_title(Some(&new_state.title));
        }
        DisplayMode::PingOnly => {
            if let Ok(icon) = Image::from_bytes(icons.transparent) {
                let _ = tray.set_icon(Some(icon));
                let _ = tray.set_icon_as_template(true);
            }
            let _ = tray.set_title(Some(&new_state.title));
        }
    }

    *last_state = Some(new_state.clone());
}

/// Pre-loaded icon bytes to avoid repeated include_bytes! calls
struct TrayIcons {
    happy: &'static [u8],
    angry: &'static [u8],
    sad: &'static [u8],
    dead: &'static [u8],
    transparent: &'static [u8],
}

/// Save history to disk asynchronously (non-blocking)
async fn save_history_async(state: &Arc<AppState>) {
    let history = state.ping_history.lock().await.clone();
    let targets = state.targets.lock().await.clone();
    let primary = state.primary_target.lock().await.clone();
    let site_monitors = state.site_monitors.lock().await.clone();
    let vpn_settings = state.vpn_settings.lock().await.clone();
    let ping_interval = *state.ping_interval_secs.lock().await;

    // Spawn blocking file I/O in a separate thread to not block async runtime
    let _ = tokio::task::spawn_blocking(move || {
        // Ignore error - can't send Box<dyn Error> across threads
        let _ = save_history(&history, &targets, &primary, &site_monitors, &vpn_settings, ping_interval);
    })
    .await;
}

/// Unified background service - consolidates ping, site monitoring, and VPN check into ONE timer
/// This dramatically reduces CPU wake-ups (from 3 independent timers to 1)
/// Battery optimization: adaptive interval (10s visible, 30s hidden), pauses during system sleep
fn start_unified_background_service(app_handle: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut tick_count = 0u64;
        let mut last_interval_secs = 10u64; // Track for consistent tick calculations

        // Pre-load icons once (not on every ping!)
        let icons = TrayIcons {
            happy: include_bytes!("../icons/pingzilla_happy.png"),
            angry: include_bytes!("../icons/pinzilla_angry.png"),
            sad: include_bytes!("../icons/pingzilla_sad.png"),
            dead: include_bytes!("../icons/pingzilla_dead.png"),
            transparent: include_bytes!("../icons/transparent.png"),
        };

        loop {
            // === SLEEP CHECK: Skip all work if system is sleeping ===
            if state.is_system_sleeping.load(Ordering::Relaxed) {
                // Wait briefly and check again - don't do any work
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }

            tick_count += 1;

            // === PING (every tick) ===
            {
                let targets = state.targets.lock().await.clone();
                let primary_target = state.primary_target.lock().await.clone();

                for target in &targets {
                    let (latency_ms, method) = do_ping(target).await;

                    let result = PingResult {
                        timestamp: Utc::now(),
                        latency_ms,
                        target: target.clone(),
                        method,
                    };

                    {
                        let mut history = state.ping_history.lock().await;
                        let target_history = history
                            .entry(target.clone())
                            .or_insert_with(|| VecDeque::with_capacity(1000));
                        target_history.push_back(result.clone());
                        // Keep 24 hours worth (varies by interval, use conservative estimate)
                        while target_history.len() > 8640 {
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

                            let icon_type = match &display_mode {
                                DisplayMode::PingOnly => TrayIconType::Transparent,
                                _ => get_icon_type_for_latency(latency_ms),
                            };

                            let new_state = TrayState {
                                icon_type,
                                title: ping_text,
                            };

                            let mut last_state = state.last_tray_state.lock().await;
                            update_tray_if_changed(&tray, &new_state, &mut last_state, &display_mode, &icons);
                        }
                    }

                    let _ = app_handle.emit("ping-update", &result);

                    // Rebuild the tray menu with current data (for native menu display)
                    if target == &primary_target {
                        if let Some(tray) = app_handle.tray_by_id("main-tray") {
                            if let Ok(menu) = build_dynamic_menu(&app_handle, &state).await {
                                let _ = tray.set_menu(Some(menu));
                            }
                        }
                    }

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
            }

            // === SITE MONITORING (every ~60 seconds) ===
            // At 10s interval: tick 6, 12, 18... At 30s interval: tick 2, 4, 6...
            let site_check_interval = if last_interval_secs == 10 { 6 } else { 2 };
            if tick_count % site_check_interval == 0 {
                let _ = check_all_sites(&app_handle, &state).await;
            }

            // === VPN/IP CHECK (every ~60 seconds, offset) ===
            let vpn_check_interval = if last_interval_secs == 10 { 6 } else { 2 };
            let vpn_offset = if last_interval_secs == 10 { 3 } else { 1 };
            if tick_count % vpn_check_interval == vpn_offset {
                check_ip_change(&app_handle, &state).await;
            }

            // === SAVE HISTORY (every ~5 minutes) ===
            let save_interval = if last_interval_secs == 10 { 30 } else { 10 };
            if tick_count % save_interval == 0 {
                save_history_async(&state).await;
            }

            // === PING INTERVAL: use user's configured setting ===
            let interval_secs = *state.ping_interval_secs.lock().await;
            last_interval_secs = interval_secs as u64;

            tokio::time::sleep(Duration::from_secs(interval_secs as u64)).await;
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
    #[serde(default)]
    site_monitors: Vec<SiteMonitor>,
    #[serde(default)]
    vpn_settings: VpnProtectionSettings,
    #[serde(default = "default_ping_interval")]
    ping_interval_secs: u32,
}

fn default_ping_interval() -> u32 {
    10
}

/// Save history to disk
fn save_history(
    history: &HashMap<String, VecDeque<PingResult>>,
    targets: &[String],
    primary_target: &str,
    site_monitors: &[SiteMonitor],
    vpn_settings: &VpnProtectionSettings,
    ping_interval_secs: u32,
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
            site_monitors: site_monitors.to_vec(),
            vpn_settings: vpn_settings.clone(),
            ping_interval_secs,
        };
        let json = serde_json::to_string(&data)?;
        std::fs::write(file_path, json)?;
    }
    Ok(())
}

/// Load history from disk
fn load_history() -> (HashMap<String, VecDeque<PingResult>>, Vec<String>, String, Vec<SiteMonitor>, VpnProtectionSettings, u32) {
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
                return (filtered_history, data.targets, data.primary_target, data.site_monitors, data.vpn_settings, data.ping_interval_secs);
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
                return (map, vec![target.clone()], target, Vec::new(), VpnProtectionSettings::default(), 10);
            }
        }
    }

    let mut history = HashMap::new();
    history.insert("1.1.1.1".to_string(), VecDeque::new());
    (history, vec!["1.1.1.1".to_string()], "1.1.1.1".to_string(), Vec::new(), VpnProtectionSettings::default(), 10)
}

/// Register for macOS sleep/wake notifications to pause background service during sleep
/// This is critical for battery optimization - without this, the app drains battery while laptop sleeps
#[cfg(target_os = "macos")]
fn register_sleep_wake_observer(state: Arc<AppState>) {
    use block2::StackBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_app_kit::NSWorkspace;

    // Spawn a thread to set up the observer and run a run loop
    std::thread::spawn(move || {
        unsafe {
            // Get NSWorkspace and its notification center
            let workspace = NSWorkspace::sharedWorkspace();
            let notification_center: *mut AnyObject = msg_send![&*workspace, notificationCenter];

            // Create notification name strings
            let sleep_name = objc2_foundation::ns_string!("NSWorkspaceWillSleepNotification");
            let wake_name = objc2_foundation::ns_string!("NSWorkspaceDidWakeNotification");

            // Clone state for sleep block
            let state_sleep = state.clone();
            let sleep_block = StackBlock::new(move |_notif: *mut AnyObject| {
                state_sleep
                    .is_system_sleeping
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            });

            // Clone state for wake block
            let state_wake = state.clone();
            let wake_block = StackBlock::new(move |_notif: *mut AnyObject| {
                state_wake
                    .is_system_sleeping
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            });

            // Register observers using addObserverForName:object:queue:usingBlock:
            // queue: nil means use the posting thread
            let _: *mut AnyObject = msg_send![
                notification_center,
                addObserverForName: &*sleep_name
                object: std::ptr::null::<AnyObject>()
                queue: std::ptr::null::<AnyObject>()
                usingBlock: &sleep_block
            ];

            let _: *mut AnyObject = msg_send![
                notification_center,
                addObserverForName: &*wake_name
                object: std::ptr::null::<AnyObject>()
                queue: std::ptr::null::<AnyObject>()
                usingBlock: &wake_block
            ];

            // Run the run loop to receive notifications
            // This is required for the observer blocks to be called
            let run_loop_class = AnyClass::get("NSRunLoop").unwrap();
            let current_run_loop: *mut AnyObject = msg_send![run_loop_class, currentRunLoop];
            let distant_future_class = AnyClass::get("NSDate").unwrap();
            let distant_future: *mut AnyObject = msg_send![distant_future_class, distantFuture];

            loop {
                let _: Bool = msg_send![current_run_loop, runMode: objc2_foundation::ns_string!("NSDefaultRunLoopMode") beforeDate: distant_future];
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn register_sleep_wake_observer(_state: Arc<AppState>) {
    // No-op on non-macOS platforms
}

/// Disable App Nap for consistent timer behavior
/// Without this, macOS throttles background apps, making timers unreliable
#[cfg(target_os = "macos")]
fn disable_app_nap() {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSActivityOptions;

    unsafe {
        // Get NSProcessInfo class
        let process_info_class = AnyClass::get("NSProcessInfo").unwrap();
        let process_info: *mut objc2::runtime::AnyObject = msg_send![process_info_class, processInfo];

        // Create the reason string
        let reason_str = objc2_foundation::ns_string!("Network monitoring service");

        // NSActivityUserInitiatedAllowingIdleSystemSleep = 0x00FFFFF
        // This tells macOS: "I'm doing important work, don't throttle me, but you can still sleep"
        let options = NSActivityOptions(0x00FFFFF);

        let _: *mut objc2::runtime::AnyObject = msg_send![process_info, beginActivityWithOptions:options reason:reason_str];
    }
}

#[cfg(not(target_os = "macos"))]
fn disable_app_nap() {
    // No-op on non-macOS platforms
}

/// Convert country code to flag emoji (e.g., "US" -> "🇺🇸")
fn country_to_flag(country_code: &str) -> String {
    if country_code.len() != 2 {
        return "🌐".to_string();
    }
    country_code
        .to_uppercase()
        .chars()
        .map(|c| char::from_u32(0x1F1E6 - 'A' as u32 + c as u32).unwrap_or(c))
        .collect()
}

/// Shorten URL for display in menu (e.g., "https://example.com/path" -> "example.com")
fn shorten_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// Build initial menu structure (before any ping data is available)
fn build_initial_menu(app: &AppHandle) -> Result<Menu<Wry>, tauri::Error> {
    let ping_item = MenuItem::with_id(app, "ping", "⚪ Ping: ---", true, None::<&str>)?;
    let target_item = MenuItem::with_id(app, "target", "   → loading...", true, None::<&str>)?;
    let stats_item = MenuItem::with_id(app, "stats", "   ↓-- · ~-- · ↑--", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let ip_item = MenuItem::with_id(app, "ip", "📍 IP: Loading...", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let dashboard = MenuItem::with_id(app, "dashboard", "📊 Open Dashboard...", true, None::<&str>)?;
    let separator3 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit PingZilla", true, None::<&str>)?;

    Menu::with_items(app, &[
        &ping_item, &target_item, &stats_item, &separator1,
        &ip_item, &separator2,
        &dashboard, &separator3,
        &quit,
    ])
}

/// Build dynamic menu with current ping data
/// Called after each ping to update the menu with latest info
async fn build_dynamic_menu(app: &AppHandle, state: &Arc<AppState>) -> Result<Menu<Wry>, tauri::Error> {
    // Get current data
    let primary_target = state.primary_target.lock().await.clone();
    let ip_info = state.ip_info.lock().await.clone();
    let site_statuses = state.site_statuses.lock().await.clone();

    // Get ping data while holding the lock, then release it
    let (current_ping, min_ms, avg_ms, max_ms) = {
        let history = state.ping_history.lock().await;
        let current_ping = history.get(&primary_target).and_then(|h| h.back()).cloned();

        // Calculate stats from recent history (last 5 minutes)
        let cutoff = Utc::now() - chrono::Duration::minutes(5);
        let pings: Vec<f64> = history
            .get(&primary_target)
            .map(|h| {
                h.iter()
                    .filter(|p| p.timestamp > cutoff)
                    .filter_map(|p| p.latency_ms)
                    .collect()
            })
            .unwrap_or_default();

        let stats = if pings.is_empty() {
            (None, None, None)
        } else {
            let min = pings.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = pings.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let avg = pings.iter().sum::<f64>() / pings.len() as f64;
            (Some(min), Some(avg), Some(max))
        };

        (current_ping, stats.0, stats.1, stats.2)
    };

    // Build menu items - info items are enabled (true) so they appear normal, not greyed out
    // Ping status with quality indicator
    let (ping_text, status_icon) = match &current_ping {
        Some(p) => match p.latency_ms {
            Some(ms) => {
                let icon = if ms < 50.0 { "🟢" } else if ms < 150.0 { "🟡" } else { "🔴" };
                (format!("{:.0}ms", ms), icon)
            },
            None => ("Timeout".to_string(), "⚫"),
        },
        None => ("---".to_string(), "⚪"),
    };
    // Add method indicator after ping value if using TCP fallback
    let method_suffix = match current_ping.as_ref().and_then(|p| p.method.as_ref()) {
        Some(PingMethod::TcpDns) | Some(PingMethod::TcpHttps) | Some(PingMethod::TcpHttp) => " TCP",
        _ => "", // ICMP - no indicator needed
    };
    let ping_item = MenuItem::with_id(app, "ping", &format!("{} Ping: {}{}", status_icon, ping_text, method_suffix), true, None::<&str>)?;

    // Target line
    let target_item = MenuItem::with_id(app, "target", &format!("   → {}", primary_target), true, None::<&str>)?;

    // Stats - more compact
    let stats_text = format!(
        "   ↓{} · ~{} · ↑{}",
        min_ms.map(|v| format!("{:.0}ms", v)).unwrap_or_else(|| "--".to_string()),
        avg_ms.map(|v| format!("{:.0}ms", v)).unwrap_or_else(|| "--".to_string()),
        max_ms.map(|v| format!("{:.0}ms", v)).unwrap_or_else(|| "--".to_string())
    );
    let stats_item = MenuItem::with_id(app, "stats", &stats_text, true, None::<&str>)?;

    let separator1 = PredefinedMenuItem::separator(app)?;

    // IP info
    let ip_text = match ip_info {
        Some(info) => {
            let flag = country_to_flag(&info.country_code);
            format!("📍 IP: {} ({} {})", info.ip, flag, info.country_code)
        }
        None => "📍 IP: Loading...".to_string(),
    };
    let ip_item = MenuItem::with_id(app, "ip", &ip_text, true, None::<&str>)?;

    // Site monitors section
    let mut menu_items: Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> = vec![
        Box::new(ping_item),
        Box::new(target_item),
        Box::new(stats_item),
        Box::new(separator1),
        Box::new(ip_item),
    ];

    // Add site monitors if any
    if !site_statuses.is_empty() {
        let sites_sep = PredefinedMenuItem::separator(app)?;
        menu_items.push(Box::new(sites_sep));

        let sites_header = MenuItem::with_id(app, "sites_header", "🌐 Sites:", true, None::<&str>)?;
        menu_items.push(Box::new(sites_header));

        for (url, status) in site_statuses.iter().take(5) {
            let icon = if status.is_up { "✅" } else { "❌" };
            let latency = status.latency_ms.map(|ms| format!("({}ms)", ms as i32)).unwrap_or_default();
            let site_name = shorten_url(url);
            let text = format!("  {} {} {}", icon, site_name, latency);
            let site_item = MenuItem::with_id(app, &format!("site_{}", url), &text, true, None::<&str>)?;
            menu_items.push(Box::new(site_item));
        }
    }

    // Action items
    let separator3 = PredefinedMenuItem::separator(app)?;
    let dashboard = MenuItem::with_id(app, "dashboard", "📊 Open Dashboard...", true, None::<&str>)?;
    let separator4 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit PingZilla", true, None::<&str>)?;

    menu_items.push(Box::new(separator3));
    menu_items.push(Box::new(dashboard));
    menu_items.push(Box::new(separator4));
    menu_items.push(Box::new(quit));

    // Build the menu
    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = menu_items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &item_refs)
}

/// Open dashboard window with full React UI (graph, stats, etc.)
/// Creates window on demand to save battery when not in use
fn open_dashboard_window(app: &AppHandle) {
    // If window exists, show it
    if let Some(window) = app.get_webview_window("dashboard") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    // Create window on demand (saves battery by not running webview until needed)
    if let Ok(window) = tauri::WebviewWindowBuilder::new(
        app,
        "dashboard",
        tauri::WebviewUrl::App("index.html".into())
    )
    .title("PingZilla")
    .inner_size(400.0, 600.0)
    .resizable(true)
    .visible(true)
    .decorations(true)
    .center()
    .build()
    {
        // Hide window on close instead of destroying (keeps app running)
        let win = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win.hide();
            }
        });
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (loaded_history, loaded_targets, loaded_primary, loaded_site_monitors, loaded_vpn_settings, loaded_ping_interval) = load_history();

    let app_state = Arc::new(AppState {
        ping_history: Mutex::new(loaded_history),
        targets: Mutex::new(loaded_targets),
        primary_target: Mutex::new(loaded_primary),
        site_monitors: Mutex::new(loaded_site_monitors),
        vpn_settings: Mutex::new(loaded_vpn_settings),
        ping_interval_secs: Mutex::new(loaded_ping_interval),
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
            get_my_ip_info,
            get_site_monitors,
            add_site_monitor,
            remove_site_monitor,
            get_site_statuses,
            get_vpn_settings,
            set_vpn_settings,
            get_network_stability,
            acknowledge_ip_change,
            set_window_visible,
            get_ping_interval,
            set_ping_interval,
        ])
        .setup(move |app| {
            // Show in Dock - required for ping to work in sandboxed App Store builds
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            // Build initial menu (will be updated dynamically on each ping)
            let initial_menu = build_initial_menu(app.handle())?;

            // Start with happy Godzilla icon (will update based on ping latency)
            let icon_bytes = include_bytes!("../icons/pingzilla_happy.png");
            let icon = Image::from_bytes(icon_bytes)?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .icon_as_template(true)
                .title("...")
                .tooltip("PingZilla - Network Monitor")
                .menu(&initial_menu)
                .show_menu_on_left_click(true) // Both left and right click show menu - works in fullscreen!
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "dashboard" => open_dashboard_window(app),
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .build(app)?;

            // Battery optimizations: disable App Nap and register for sleep/wake
            disable_app_nap();
            register_sleep_wake_observer(app_state.clone());

            // Single unified background service for battery efficiency
            // Consolidates ping, site monitoring, and VPN check into ONE timer
            start_unified_background_service(app.handle().clone(), app_state.clone());

            // No window at startup - webview is created on demand when user opens dashboard
            // This saves significant battery by not running Chromium until needed

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
