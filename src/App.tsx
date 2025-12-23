// ABOUTME: PingZilla React frontend - displays ping graph and current latency
// ABOUTME: Listens to Tauri events for real-time updates

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

interface ChartData {
  time: string;
  latency: number | null;
}

function App() {
  const [currentPing, setCurrentPing] = useState<number | null>(null);
  const [history, setHistory] = useState<ChartData[]>([]);
  const [target, setTarget] = useState("8.8.8.8");
  const [threshold, setThreshold] = useState(400);
  const [showSettings, setShowSettings] = useState(false);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);

  // Load initial data and settings
  useEffect(() => {
    const loadData = async () => {
      try {
        const [loadedTarget, loadedThreshold] = await invoke<[string, number]>("get_settings");
        setTarget(loadedTarget);
        setThreshold(loadedThreshold);

        const autoStartEnabled = await isEnabled();
        setLaunchAtLogin(autoStartEnabled);

        const pingHistory = await invoke<PingResult[]>("get_ping_history");
        const chartData = pingHistory.slice(-60).map((p) => ({
          time: new Date(p.timestamp).toLocaleTimeString("en-US", {
            hour12: false,
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          }),
          latency: p.latency_ms,
        }));
        setHistory(chartData);

        if (pingHistory.length > 0) {
          const last = pingHistory[pingHistory.length - 1];
          setCurrentPing(last.latency_ms);
        }
      } catch (e) {
        console.error("Failed to load initial data:", e);
      }
    };

    loadData();
  }, []);

  // Listen for real-time ping updates
  useEffect(() => {
    const unlisten = listen<PingResult>("ping-update", (event) => {
      const result = event.payload;
      setCurrentPing(result.latency_ms);

      setHistory((prev) => {
        const newData = [
          ...prev,
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
        return newData;
      });
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const saveSettings = useCallback(async () => {
    try {
      await invoke("set_ping_target", { target });
      await invoke("set_notification_threshold", { thresholdMs: threshold });
      setShowSettings(false);
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }, [target, threshold]);

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

      {/* Settings Panel */}
      {showSettings && (
        <div className="settings-panel">
          <div className="setting-row">
            <label>Target:</label>
            <input
              type="text"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              placeholder="8.8.8.8"
            />
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
        <div className="ping-target">{target}</div>
      </div>

      {/* Ping Graph */}
      <div className="graph-container">
        <ResponsiveContainer width="100%" height={180}>
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
