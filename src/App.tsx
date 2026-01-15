// ABOUTME: PingZilla React frontend - displays ping graph and current latency
// ABOUTME: Supports multiple targets with tabs and statistics display

import { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  ResponsiveContainer,
  ReferenceLine,
} from "recharts";
import "./App.css";

// Animated number component for smooth transitions
function AnimatedNumber({ value, duration = 300 }: { value: number | null; duration?: number }) {
  const [displayValue, setDisplayValue] = useState(value);
  const animationRef = useRef<number | undefined>(undefined);
  const startTimeRef = useRef<number | undefined>(undefined);
  const startValueRef = useRef<number | null>(null);

  useEffect(() => {
    if (value === null) {
      setDisplayValue(null);
      return;
    }

    const startValue = displayValue ?? value;
    startValueRef.current = startValue;
    startTimeRef.current = performance.now();

    const animate = (currentTime: number) => {
      const elapsed = currentTime - (startTimeRef.current ?? currentTime);
      const progress = Math.min(elapsed / duration, 1);

      // Ease out cubic for smooth deceleration
      const easeOut = 1 - Math.pow(1 - progress, 3);

      const start = startValueRef.current ?? value;
      const current = start + (value - start) * easeOut;

      setDisplayValue(Math.round(current));

      if (progress < 1) {
        animationRef.current = requestAnimationFrame(animate);
      }
    };

    animationRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
      }
    };
  }, [value, duration]);

  return <>{displayValue !== null ? displayValue : "---"}</>;
}

type PingMethod = "Icmp" | "TcpDns" | "TcpHttps" | "TcpHttp";

interface PingResult {
  timestamp: string;
  latency_ms: number | null;
  target: string;
  method: PingMethod | null;
}

interface PingStatistics {
  min_ms: number | null;
  max_ms: number | null;
  avg_ms: number | null;
  packet_loss_pct: number;
  total_pings: number;
  failed_pings: number;
}

interface ChartData {
  time: string;
  latency: number | null;
}

interface IpInfo {
  ip: string;
  country: string;
  country_code: string;
  city: string | null;
  isp: string | null;
}

interface SiteMonitor {
  url: string;
  name: string | null;
  enabled: boolean;
}

interface SiteStatus {
  url: string;
  is_up: boolean;
  latency_ms: number | null;
  last_check: string;
  last_down: string | null;
}

// VPN drop detection types
type NetworkChangeType = "IpChanged" | "CountryChanged" | "IspChanged" | "Initial";

interface NetworkChangeEvent {
  change_type: NetworkChangeType;
  previous: IpInfo | null;
  current: IpInfo;
  timestamp: string;
  is_expected: boolean;
}

interface VpnProtectionSettings {
  enabled: boolean;
  check_interval_secs: number;
  alert_on_country_change: boolean;
  alert_on_ip_change: boolean;
  expected_country: string | null;
}

type DisplayMode = "icon_only" | "icon_and_ping" | "ping_only";

// View mode for window type detection (dashboard vs settings)
type ViewMode = "dashboard" | "settings" | "full";

// Get view mode from URL params (e.g., index.html?view=dashboard)
const getViewMode = (): ViewMode => {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");
  if (view === "dashboard") return "dashboard";
  if (view === "settings") return "settings";
  return "full"; // Default: show everything (for compatibility)
};

// Convert country code to flag emoji
const countryCodeToFlag = (code: string): string => {
  if (!code || code.length !== 2) return "";
  return code
    .toUpperCase()
    .split("")
    .map((char) => String.fromCodePoint(127397 + char.charCodeAt(0)))
    .join("");
};

function App() {
  // Detect view mode from URL params
  const viewMode = getViewMode();

  const [targets, setTargets] = useState<string[]>(["1.1.1.1"]);
  const [activeTarget, setActiveTarget] = useState("1.1.1.1");
  const [currentPings, setCurrentPings] = useState<Record<string, number | null>>({});
  const [currentMethods, setCurrentMethods] = useState<Record<string, PingMethod | null>>({});
  const [histories, setHistories] = useState<Record<string, ChartData[]>>({});
  const [statistics, setStatistics] = useState<PingStatistics | null>(null);
  const [statsPeriod, setStatsPeriod] = useState(5); // minutes
  const [threshold, setThreshold] = useState(400);
  const [displayMode, setDisplayMode] = useState<DisplayMode>("icon_and_ping");
  const [showSettings, setShowSettings] = useState(false);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [ipInfo, setIpInfo] = useState<IpInfo | null>(null);
  const [ipLoading, setIpLoading] = useState(false);
  const [siteMonitors, setSiteMonitors] = useState<SiteMonitor[]>([]);
  const [siteStatuses, setSiteStatuses] = useState<Record<string, SiteStatus>>({});
  const [showAddSite, setShowAddSite] = useState(false);
  const [newSiteUrl, setNewSiteUrl] = useState("");
  const [newSiteName, setNewSiteName] = useState("");
  // VPN drop detection state
  const [vpnSettings, setVpnSettings] = useState<VpnProtectionSettings>({
    enabled: true,
    check_interval_secs: 30,
    alert_on_country_change: true,
    alert_on_ip_change: true,
    expected_country: null,
  });
  const [networkAlert, setNetworkAlert] = useState<NetworkChangeEvent | null>(null);
  const [showVpnSettings, setShowVpnSettings] = useState(false);
  // Ping interval setting (in seconds)
  const [pingInterval, setPingInterval] = useState(10);

  // Load initial data and settings
  useEffect(() => {
    const loadData = async () => {
      try {
        const loadedTargets = await invoke<string[]>("get_targets");
        setTargets(loadedTargets);
        if (loadedTargets.length > 0) {
          setActiveTarget(loadedTargets[0]);
        }

        const [primaryTarget, loadedThreshold, loadedDisplayMode] = await invoke<[string, number, string]>("get_settings");
        setActiveTarget(primaryTarget);
        setThreshold(loadedThreshold);
        setDisplayMode(loadedDisplayMode as DisplayMode);

        const autoStartEnabled = await isEnabled();
        setLaunchAtLogin(autoStartEnabled);

        // Load history for each target
        const newHistories: Record<string, ChartData[]> = {};
        const newCurrentPings: Record<string, number | null> = {};

        for (const target of loadedTargets) {
          const pingHistory = await invoke<PingResult[]>("get_ping_history", { target });
          const chartData = pingHistory.slice(-60).map((p) => ({
            time: new Date(p.timestamp).toLocaleTimeString("en-US", {
              hour12: false,
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
            }),
            latency: p.latency_ms,
          }));
          newHistories[target] = chartData;

          if (pingHistory.length > 0) {
            const last = pingHistory[pingHistory.length - 1];
            newCurrentPings[target] = last.latency_ms;
          }
        }

        setHistories(newHistories);
        setCurrentPings(newCurrentPings);

        // Load IP info
        try {
          const loadedIpInfo = await invoke<IpInfo>("get_my_ip_info", {});
          setIpInfo(loadedIpInfo);
        } catch (e) {
          console.error("Failed to load IP info:", e);
        }

        // Load site monitors
        try {
          const loadedMonitors = await invoke<SiteMonitor[]>("get_site_monitors");
          setSiteMonitors(loadedMonitors);
          const loadedStatuses = await invoke<Record<string, SiteStatus>>("get_site_statuses");
          setSiteStatuses(loadedStatuses);
        } catch (e) {
          console.error("Failed to load site monitors:", e);
        }

        // Load VPN protection settings
        try {
          const loadedVpnSettings = await invoke<VpnProtectionSettings>("get_vpn_settings");
          setVpnSettings(loadedVpnSettings);
        } catch (e) {
          console.error("Failed to load VPN settings:", e);
        }

        // Load ping interval setting
        try {
          const loadedPingInterval = await invoke<number>("get_ping_interval");
          setPingInterval(loadedPingInterval);
        } catch (e) {
          console.error("Failed to load ping interval:", e);
        }
      } catch (e) {
        console.error("Failed to load initial data:", e);
      }
    };

    loadData();
  }, []);

  // Track window visibility for adaptive ping interval (battery optimization)
  useEffect(() => {
    const handleFocus = () => {
      invoke('set_window_visible', { visible: true }).catch(console.error);
    };
    const handleBlur = () => {
      invoke('set_window_visible', { visible: false }).catch(console.error);
    };

    window.addEventListener('focus', handleFocus);
    window.addEventListener('blur', handleBlur);

    // Set initial state based on document focus
    if (document.hasFocus()) {
      handleFocus();
    }

    return () => {
      window.removeEventListener('focus', handleFocus);
      window.removeEventListener('blur', handleBlur);
    };
  }, []);

  // Load statistics when active target or period changes
  useEffect(() => {
    const loadStats = async () => {
      try {
        const stats = await invoke<PingStatistics>("get_statistics", {
          target: activeTarget,
          minutes: statsPeriod,
        });
        setStatistics(stats);
      } catch (e) {
        console.error("Failed to load statistics:", e);
      }
    };

    loadStats();
    const interval = setInterval(loadStats, 5000); // Refresh stats every 5 seconds
    return () => clearInterval(interval);
  }, [activeTarget, statsPeriod]);

  // Listen for real-time ping updates
  useEffect(() => {
    const unlisten = listen<PingResult>("ping-update", (event) => {
      const result = event.payload;

      setCurrentPings((prev) => ({
        ...prev,
        [result.target]: result.latency_ms,
      }));

      setCurrentMethods((prev) => ({
        ...prev,
        [result.target]: result.method,
      }));

      setHistories((prev) => {
        const targetHistory = prev[result.target] || [];
        const newData = [
          ...targetHistory,
          {
            time: new Date(result.timestamp).toLocaleTimeString("en-US", {
              hour12: false,
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
            }),
            latency: result.latency_ms,
          },
        ].slice(-60);
        return { ...prev, [result.target]: newData };
      });
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Listen for site status updates
  useEffect(() => {
    const unlisten = listen<Record<string, SiteStatus>>("site-status-update", (event) => {
      setSiteStatuses(event.payload);
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Listen for network change events (VPN drop detection)
  useEffect(() => {
    const unlisten = listen<NetworkChangeEvent>("network-change", (event) => {
      const change = event.payload;
      // Show alert for unexpected country changes (VPN dropped!)
      if (!change.is_expected && change.change_type === "CountryChanged") {
        setNetworkAlert(change);
      }
      // Update IP info with current values
      setIpInfo(change.current);
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const refreshIpInfo = useCallback(async () => {
    setIpLoading(true);
    try {
      const info = await invoke<IpInfo>("get_my_ip_info", { forceRefresh: true });
      setIpInfo(info);
    } catch (e) {
      console.error("Failed to refresh IP info:", e);
    } finally {
      setIpLoading(false);
    }
  }, []);

  const addSiteMonitor = useCallback(async () => {
    if (!newSiteUrl.trim()) return;
    try {
      await invoke("add_site_monitor", {
        url: newSiteUrl.trim(),
        name: newSiteName.trim() || null,
      });
      const updatedMonitors = await invoke<SiteMonitor[]>("get_site_monitors");
      setSiteMonitors(updatedMonitors);
      setNewSiteUrl("");
      setNewSiteName("");
      setShowAddSite(false);
    } catch (e) {
      console.error("Failed to add site monitor:", e);
    }
  }, [newSiteUrl, newSiteName]);

  const removeSiteMonitor = useCallback(async (url: string) => {
    try {
      await invoke("remove_site_monitor", { url });
      const updatedMonitors = await invoke<SiteMonitor[]>("get_site_monitors");
      setSiteMonitors(updatedMonitors);
      setSiteStatuses((prev) => {
        const next = { ...prev };
        delete next[url];
        return next;
      });
    } catch (e) {
      console.error("Failed to remove site monitor:", e);
    }
  }, []);

  const acknowledgeNetworkAlert = useCallback(async () => {
    try {
      await invoke("acknowledge_ip_change");
      setNetworkAlert(null);
    } catch (e) {
      console.error("Failed to acknowledge network alert:", e);
    }
  }, []);

  const updateVpnSettings = useCallback(async (newSettings: VpnProtectionSettings) => {
    try {
      await invoke("set_vpn_settings", { settings: newSettings });
      setVpnSettings(newSettings);
    } catch (e) {
      console.error("Failed to update VPN settings:", e);
    }
  }, []);

  const saveSettings = useCallback(async () => {
    try {
      await invoke("set_notification_threshold", { thresholdMs: threshold });
      await invoke("set_display_mode", { mode: displayMode });
      await invoke("set_ping_interval", { interval_secs: pingInterval });
      setShowSettings(false);
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }, [threshold, displayMode, pingInterval]);

  const toggleLaunchAtLogin = useCallback(async () => {
    try {
      if (launchAtLogin) {
        await disable();
        setLaunchAtLogin(false);
      } else {
        await enable();
        setLaunchAtLogin(true);
      }
    } catch (e) {
      console.error("Failed to toggle launch at login:", e);
    }
  }, [launchAtLogin]);

  // Determine ping color based on latency
  const getPingColor = (ms: number | null): string => {
    if (ms === null) return "#888";
    if (ms < 50) return "#22c55e"; // green
    if (ms < 150) return "#eab308"; // yellow
    return "#ef4444"; // red
  };

  const getPingStatus = (ms: number | null): string => {
    if (ms === null) return "Timeout";
    if (ms < 50) return "Excellent";
    if (ms < 150) return "Good";
    return "Poor";
  };

  const currentPing = currentPings[activeTarget] ?? null;
  const currentMethod = currentMethods[activeTarget] ?? null;
  const history = histories[activeTarget] || [];

  // Check if using TCP fallback (not real ICMP)
  const isTcpFallback = currentMethod && currentMethod !== "Icmp";

  return (
    <div className="app">
      {/* Header */}
      <div className="header">
        <h1 className="title">
          {viewMode === "settings" ? "PingZilla Settings" : "PingZilla"}
        </h1>
        {/* Hide settings button in dedicated settings window or dashboard */}
        {viewMode === "full" && (
          <button
            className="settings-btn"
            onClick={() => setShowSettings(!showSettings)}
          >
            {showSettings ? "X" : "Settings"}
          </button>
        )}
      </div>

      {/* IP Info Bar */}
      {ipInfo && (
        <div className={`ip-info-bar ${networkAlert ? "alert" : ""}`}>
          {networkAlert && <span className="vpn-alert-icon" title="Network change detected!">⚠️</span>}
          <span className="ip-label">Your IP</span>
          <span className="ip-flag">{countryCodeToFlag(ipInfo.country_code)}</span>
          <span className="ip-address">{ipInfo.ip}</span>
          <span className="ip-country">{ipInfo.country}</span>
          <button
            className={`vpn-settings-btn ${vpnSettings.enabled ? "active" : ""}`}
            onClick={() => setShowVpnSettings(!showVpnSettings)}
            title="VPN Protection Settings"
          >
            🛡️
          </button>
          <button
            className="ip-refresh-btn"
            onClick={refreshIpInfo}
            disabled={ipLoading}
            title="Refresh IP info"
          >
            {ipLoading ? "..." : "↻"}
          </button>
        </div>
      )}

      {/* VPN Alert Banner */}
      {networkAlert && (
        <div className="vpn-alert-banner">
          <div className="alert-icon">🚨</div>
          <div className="alert-content">
            <div className="alert-title">VPN Connection May Have Dropped!</div>
            <div className="alert-details">
              {networkAlert.previous && (
                <>
                  {countryCodeToFlag(networkAlert.previous.country_code)} {networkAlert.previous.country}
                  {" → "}
                  {countryCodeToFlag(networkAlert.current.country_code)} {networkAlert.current.country}
                </>
              )}
            </div>
          </div>
          <button className="alert-dismiss" onClick={acknowledgeNetworkAlert}>
            Dismiss
          </button>
        </div>
      )}

      {/* VPN Settings Panel */}
      {showVpnSettings && (
        <div className="vpn-settings-panel">
          <div className="setting-row">
            <label>VPN Protection:</label>
            <button
              className={`toggle-btn ${vpnSettings.enabled ? "active" : ""}`}
              onClick={() => updateVpnSettings({ ...vpnSettings, enabled: !vpnSettings.enabled })}
            >
              {vpnSettings.enabled ? "On" : "Off"}
            </button>
          </div>
          <div className="setting-row">
            <label>Check interval:</label>
            <select
              className="display-mode-select"
              value={vpnSettings.check_interval_secs}
              onChange={(e) =>
                updateVpnSettings({ ...vpnSettings, check_interval_secs: parseInt(e.target.value) })
              }
            >
              <option value={15}>15 sec</option>
              <option value={30}>30 sec</option>
              <option value={60}>1 min</option>
              <option value={120}>2 min</option>
            </select>
          </div>
          <div className="setting-row">
            <label>Alert on country change:</label>
            <button
              className={`toggle-btn ${vpnSettings.alert_on_country_change ? "active" : ""}`}
              onClick={() =>
                updateVpnSettings({
                  ...vpnSettings,
                  alert_on_country_change: !vpnSettings.alert_on_country_change,
                })
              }
            >
              {vpnSettings.alert_on_country_change ? "On" : "Off"}
            </button>
          </div>
          <div className="setting-row">
            <label>Alert on IP change:</label>
            <button
              className={`toggle-btn ${vpnSettings.alert_on_ip_change ? "active" : ""}`}
              onClick={() =>
                updateVpnSettings({ ...vpnSettings, alert_on_ip_change: !vpnSettings.alert_on_ip_change })
              }
            >
              {vpnSettings.alert_on_ip_change ? "On" : "Off"}
            </button>
          </div>
        </div>
      )}

      {/* Settings Panel - always visible in settings mode, toggleable in full mode */}
      {(viewMode === "settings" || (viewMode === "full" && showSettings)) && (
        <div className="settings-panel">
          <div className="setting-row">
            <label>Ping target:</label>
            <input
              type="text"
              value={activeTarget}
              onChange={(e) => {
                const newTarget = e.target.value;
                setActiveTarget(newTarget);
              }}
              onBlur={async () => {
                if (activeTarget.trim()) {
                  try {
                    if (!targets.includes(activeTarget)) {
                      await invoke("add_target", { target: activeTarget.trim() });
                      const updatedTargets = await invoke<string[]>("get_targets");
                      setTargets(updatedTargets);
                    }
                    await invoke("set_primary_target", { target: activeTarget.trim() });
                  } catch (e) {
                    console.error("Failed to update target:", e);
                  }
                }
              }}
              placeholder="IP or hostname"
            />
          </div>
          <div className="setting-row">
            <label>Menu bar:</label>
            <select
              className="display-mode-select"
              value={displayMode}
              onChange={(e) => setDisplayMode(e.target.value as DisplayMode)}
            >
              <option value="icon_only">Icon only</option>
              <option value="icon_and_ping">Icon + Ping</option>
              <option value="ping_only">Ping only</option>
            </select>
          </div>
          <div className="setting-row">
            <label>Ping interval:</label>
            <select
              className="display-mode-select"
              value={pingInterval}
              onChange={(e) => setPingInterval(parseInt(e.target.value))}
            >
              <option value={5}>5 sec</option>
              <option value={10}>10 sec</option>
              <option value={15}>15 sec</option>
              <option value={30}>30 sec</option>
              <option value={60}>1 min</option>
              <option value={120}>2 min</option>
            </select>
          </div>
          <div className="setting-row">
            <label>Alert threshold:</label>
            <input
              type="number"
              value={threshold}
              onChange={(e) => setThreshold(parseInt(e.target.value) || 400)}
              min={50}
              max={1000}
            />
            <span>ms</span>
          </div>
          <div className="setting-row">
            <label>Launch at login:</label>
            <button
              className={`toggle-btn ${launchAtLogin ? "active" : ""}`}
              onClick={toggleLaunchAtLogin}
            >
              {launchAtLogin ? "On" : "Off"}
            </button>
          </div>
          <button className="save-btn" onClick={saveSettings}>
            Save
          </button>
        </div>
      )}

      {/* Main content - hidden in settings mode */}
      {viewMode !== "settings" && (
        <>
      {/* Current Ping Display */}
      <div className="current-ping">
        <div
          className="ping-value"
          style={{ color: getPingColor(currentPing) }}
        >
          <AnimatedNumber value={currentPing !== null ? Math.round(currentPing) : null} duration={400} />
          <span className="ping-unit">ms</span>
        </div>
        <div
          className="ping-status"
          style={{ color: getPingColor(currentPing) }}
        >
          {getPingStatus(currentPing)}
          {isTcpFallback && (
            <span className="tcp-badge" title="Using TCP connect instead of ICMP ping (sandbox mode)">
              TCP
            </span>
          )}
        </div>
      </div>

      {/* Statistics Row */}
      {statistics && (
        <div className="stats-row">
          <div className="stat">
            <span className="stat-label">Min</span>
            <span className="stat-value">
              {statistics.min_ms !== null ? `${Math.round(statistics.min_ms)}ms` : "---"}
            </span>
          </div>
          <div className="stat">
            <span className="stat-label">Avg</span>
            <span className="stat-value">
              {statistics.avg_ms !== null ? `${Math.round(statistics.avg_ms)}ms` : "---"}
            </span>
          </div>
          <div className="stat">
            <span className="stat-label">Max</span>
            <span className="stat-value">
              {statistics.max_ms !== null ? `${Math.round(statistics.max_ms)}ms` : "---"}
            </span>
          </div>
          <div className="stat">
            <span className="stat-label">Loss</span>
            <span
              className="stat-value"
              style={{ color: statistics.packet_loss_pct > 0 ? "#ef4444" : "#22c55e" }}
            >
              {statistics.packet_loss_pct.toFixed(1)}%
            </span>
          </div>
          <select
            className="stats-period"
            value={statsPeriod}
            onChange={(e) => setStatsPeriod(parseInt(e.target.value))}
          >
            <option value={5}>5m</option>
            <option value={30}>30m</option>
            <option value={60}>1h</option>
            <option value={1440}>24h</option>
          </select>
        </div>
      )}

      {/* Ping Graph */}
      <div className="graph-container">
        <ResponsiveContainer width="100%" height={140}>
          <LineChart data={history} margin={{ top: 5, right: 10, left: -20, bottom: 5 }}>
            <XAxis
              dataKey="time"
              tick={{ fontSize: 10, fill: "#888" }}
              interval="preserveStartEnd"
              tickLine={false}
              axisLine={{ stroke: "#333" }}
            />
            <YAxis
              tick={{ fontSize: 10, fill: "#888" }}
              tickLine={false}
              axisLine={{ stroke: "#333" }}
              domain={[0, "auto"]}
            />
            <ReferenceLine
              y={threshold}
              stroke="#ef4444"
              strokeDasharray="3 3"
              strokeOpacity={0.5}
            />
            <Line
              type="monotone"
              dataKey="latency"
              stroke="#3b82f6"
              strokeWidth={2}
              dot={false}
              connectNulls={false}
              isAnimationActive={false}
            />
          </LineChart>
        </ResponsiveContainer>
      </div>

      {/* Site Monitors Section */}
      <div className="site-monitors-section">
        <div className="section-header">
          <span className="section-title">Site Monitors</span>
          <button
            className="site-add-btn"
            onClick={() => setShowAddSite(!showAddSite)}
            disabled={siteMonitors.length >= 10}
            title={siteMonitors.length >= 10 ? "Max 10 sites" : "Add site"}
          >
            {showAddSite ? "×" : "+"}
          </button>
        </div>

        {/* Add Site Panel */}
        {showAddSite && (
          <div className="add-site-panel">
            <input
              type="text"
              value={newSiteUrl}
              onChange={(e) => setNewSiteUrl(e.target.value)}
              placeholder="URL (e.g., https://example.com)"
              onKeyDown={(e) => e.key === "Enter" && addSiteMonitor()}
              autoFocus
            />
            <input
              type="text"
              value={newSiteName}
              onChange={(e) => setNewSiteName(e.target.value)}
              placeholder="Name (optional)"
              onKeyDown={(e) => e.key === "Enter" && addSiteMonitor()}
            />
            <div className="add-site-buttons">
              <button className="cancel-btn" onClick={() => setShowAddSite(false)}>
                Cancel
              </button>
              <button className="save-btn" onClick={addSiteMonitor}>
                Add
              </button>
            </div>
          </div>
        )}

        {/* Site List */}
        {siteMonitors.length > 0 ? (
          <div className="site-list">
            {siteMonitors.map((site) => {
              const status = siteStatuses[site.url];
              const isUp = status?.is_up ?? true;
              return (
                <div key={site.url} className={`site-item ${isUp ? "up" : "down"}`}>
                  <span className={`status-dot ${isUp ? "up" : "down"}`} />
                  <span className="site-name" title={site.url}>
                    {site.name || site.url}
                  </span>
                  <span className="site-latency">
                    {status?.latency_ms != null ? `${Math.round(status.latency_ms)}ms` : "---"}
                  </span>
                  <button
                    className="site-remove-btn"
                    onClick={() => removeSiteMonitor(site.url)}
                    title="Remove"
                  >
                    ×
                  </button>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="site-empty">No sites monitored</div>
        )}
      </div>
        </>
      )}

      {/* Footer */}
      <div className="footer">
        <span className="footer-text">
          {viewMode === "settings" ? "PingZilla v1.3.4" : "Last 2 minutes"}
        </span>
      </div>
    </div>
  );
}

export default App;
