import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, primaryMonitor, PhysicalPosition } from "@tauri-apps/api/window";

interface SystemSummary {
  cpu: number;
  mem_used: number;
  mem_total: number;
}

interface Snapshot {
  summary: SystemSummary;
}

interface TempSensor {
  value: number;
  kind: string; // "temp" or "fan"
}

interface Temperatures {
  available: boolean;
  sensors: TempSensor[];
}

interface SensorAccess {
  capability: string; // "webcam" | "microphone" | "location"
  in_use: boolean;
}

const el = (id: string) => document.getElementById(id) as HTMLElement;

function tempClass(v: number): string {
  return v >= 85 ? "hot" : v >= 70 ? "warm" : "";
}

// Pin the overlay top-center on the primary monitor, with a small margin.
// Only needs to run once — the window keeps its position across show()/hide()
// toggles for the rest of the session.
async function positionTopCenter() {
  const win = getCurrentWindow();
  const monitor = await primaryMonitor();
  if (!monitor) return;
  const size = await win.outerSize();
  const margin = 4;
  const x = monitor.position.x + Math.round((monitor.size.width - size.width) / 2);
  const y = monitor.position.y + margin;
  await win.setPosition(new PhysicalPosition(x, y));
}

async function tick() {
  try {
    const snap = await invoke<Snapshot>("get_snapshot");
    el("osd-cpu").textContent = `${Math.round(snap.summary.cpu)}%`;
    const pct =
      snap.summary.mem_total > 0
        ? Math.round((snap.summary.mem_used / snap.summary.mem_total) * 100)
        : 0;
    el("osd-ram").textContent = `${pct}%`;
  } catch {
    el("osd-cpu").textContent = "—";
    el("osd-ram").textContent = "—";
  }

  try {
    const t = await invoke<Temperatures>("get_temperatures");
    const temps = t.sensors.filter((s) => s.kind !== "fan");
    const hottest = temps.reduce((m, s) => (s.value > m ? s.value : m), 0);
    const tempEl = el("osd-temp");
    if (t.available && temps.length) {
      tempEl.textContent = `${Math.round(hottest)}°C`;
      tempEl.className = `osd-value ${tempClass(hottest)}`;
    } else {
      tempEl.textContent = "—";
      tempEl.className = "osd-value";
    }
  } catch {
    el("osd-temp").textContent = "—";
  }

  try {
    const sensors = await invoke<SensorAccess[]>("get_sensor_access");
    const micActive = sensors.some((s) => s.capability === "microphone" && s.in_use);
    const camActive = sensors.some((s) => s.capability === "webcam" && s.in_use);
    el("osd-mic").classList.toggle("active", micActive);
    el("osd-cam").classList.toggle("active", camActive);
  } catch {
    el("osd-mic").classList.remove("active");
    el("osd-cam").classList.remove("active");
  }
}

async function init() {
  const win = getCurrentWindow();
  await win.setIgnoreCursorEvents(true);
  await positionTopCenter();
  tick();
  setInterval(tick, 2000);
}

init();
