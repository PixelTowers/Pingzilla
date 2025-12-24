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

interface PingResult {
  timestamp: string;
  latency_ms: number | null;
  target: string;
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

type DisplayMode = "icon_only" | "icon_and_ping" | "ping_only";

function App() {
  const [targets, setTargets] = useState<string[]>(["8.8.8.8"]);
  const [activeTarget, setActiveTarget] = useState("8.8.8.8");
  const [currentPings, setCurrentPings] = useState<Record<string, number | null>>({});
  const [histories, setHistories] = useState<Record<string, ChartData[]>>({});
  const [statistics, setStatistics] = useState<PingStatistics | null>(null);
  const [statsPeriod, setStatsPeriod] = useState(5); // minutes
  const [threshold, setThreshold] = useState(400);
  const [displayMode, setDisplayMode] = useState<DisplayMode>("icon_and_ping");
  const [showSettings, setShowSettings] = useState(false);
  const [showAddTarget, setShowAddTarget] = useState(false);
  const [newTarget, setNewTarget] = useState("");
  const [launchAtLogin, setLaunchAtLogin] = useState(false);

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
      } catch (e) {
        console.error("Failed to load initial data:", e);
      }
    };

    loadData();
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

  const saveSettings = useCallback(async () => {
    try {
      await invoke("set_notification_threshold", { thresholdMs: threshold });
      await invoke("set_display_mode", { mode: displayMode });
      setShowSettings(false);
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }, [threshold, displayMode]);

  const addTarget = useCallback(async () => {
    if (!newTarget.trim()) return;
    try {
      await invoke("add_target", { target: newTarget.trim() });
      const updatedTargets = await invoke<string[]>("get_targets");
      setTargets(updatedTargets);
      setNewTarget("");
      setShowAddTarget(false);
    } catch (e) {
      console.error("Failed to add target:", e);
    }
  }, [newTarget]);

  const removeTarget = useCallback(async (target: string) => {
    try {
      await invoke("remove_target", { target });
      const updatedTargets = await invoke<string[]>("get_targets");
      setTargets(updatedTargets);
      if (activeTarget === target && updatedTargets.length > 0) {
        setActiveTarget(updatedTargets[0]);
      }
    } catch (e) {
      console.error("Failed to remove target:", e);
    }
  }, [activeTarget]);

  const switchTarget = useCallback(async (target: string) => {
    setActiveTarget(target);
    try {
      await invoke("set_primary_target", { target });
    } catch (e) {
      console.error("Failed to set primary target:", e);
    }
  }, []);

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
  const history = histories[activeTarget] || [];

  return (
    <div className="app">
      {/* Header */}
      <div className="header">
        <h1 className="title">PingZilla</h1>
        <button
          className="settings-btn"
          onClick={() => setShowSettings(!showSettings)}
        >
          {showSettings ? "X" : "Settings"}
        </button>
      </div>

      {/* Target Tabs */}
      <div className="tabs-container">
        <div className="tabs">
          {targets.map((target) => (
            <div
              key={target}
              className={`tab ${activeTarget === target ? "active" : ""}`}
              onClick={() => switchTarget(target)}
            >
              <span className="tab-name">{target}</span>
              <span
                className="tab-ping"
                style={{ color: getPingColor(currentPings[target] ?? null) }}
              >
                {currentPings[target] !== undefined && currentPings[target] !== null
                  ? `${Math.round(currentPings[target])}ms`
                  : "---"}
              </span>
              {targets.length > 1 && (
                <button
                  className="tab-close"
                  onClick={(e) => {
                    e.stopPropagation();
                    removeTarget(target);
                  }}
                >
                  ×
                </button>
              )}
            </div>
          ))}
          <button className="tab-add" onClick={() => setShowAddTarget(true)}>
            +
          </button>
        </div>
      </div>

      {/* Add Target Modal */}
      {showAddTarget && (
        <div className="add-target-panel">
          <input
            type="text"
            value={newTarget}
            onChange={(e) => setNewTarget(e.target.value)}
            placeholder="Enter hostname or IP"
            onKeyDown={(e) => e.key === "Enter" && addTarget()}
            autoFocus
          />
          <div className="add-target-buttons">
            <button className="cancel-btn" onClick={() => setShowAddTarget(false)}>
              Cancel
            </button>
            <button className="save-btn" onClick={addTarget}>
              Add
            </button>
          </div>
        </div>
      )}

      {/* Settings Panel */}
      {showSettings && (
        <div className="settings-panel">
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

      {/* Footer */}
      <div className="footer">
        <span className="footer-text">Last 2 minutes</span>
      </div>
    </div>
  );
}

export default App;
