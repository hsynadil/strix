# Strix — Roadmap

**Strix** — a lightweight Windows process & performance monitor inspired by **AppControl**.
(The repo folder is still `perf-diag`; the product name is Strix.)
Built with **Tauri + Rust** (backend) and **vanilla TS + Vite** (frontend).

Goal: match AppControl's standout features (historical timeline, privacy alerts,
plain-language app info) while staying lighter on RAM.

---

## Phase 0 — Foundation ✅ (done)

- [x] Rust (MSVC) toolchain + Tauri scaffold, switched to **pnpm**
- [x] Pinned `time` 0.3.51 to fix the `cookie`/`time` build break
- [x] `sysinfo`-backed backend: `get_snapshot`, `kill_process`
- [x] Dark, compact UI: live table, sortable columns, filter, pause, End Task

## Phase 1 — Solid live view (MVP) 🔜

The dependable "better Task Manager" base.

- [ ] **Verify MVP runs** end to end (window opens, list updates, kill works)
- [ ] Extra columns: process owner/user, exe path, status, thread/handle count
- [ ] **Group by application** (collapse child processes under a parent, like Task Manager's Apps section)
- [ ] Smooth refresh: update rows in place instead of full re-render (no fl/ scroll jump)
- [ ] Configurable refresh interval (1s / 2s / 5s)
- [ ] Header sparkline for total CPU & RAM (last ~60s in-memory)

## Phase 2 — Historical recording + timeline ⭐ (AppControl's killer feature)

Record metrics over time and scrub back through them.

- [ ] Storage layer: ring buffer in memory + periodic flush to **SQLite** (`rusqlite`)
- [ ] Sample CPU / RAM / Disk (and later GPU/temp) every ~1–2s, retain up to **3 days**
- [ ] Timeline slider UI: scrub to any past moment, hover shows exact value + timestamp
- [ ] "What spiked?" — at a selected time, show the top processes responsible
- [ ] Per-metric charts (CPU / RAM / Disk / GPU) with the shared timeline cursor
- [ ] Background sampling continues while the window is hidden (tray)

## Phase 3 — App identification & control

- [ ] **App Details**: right-click → plain-language description of a process/exe
  - source: exe version info / publisher signature, plus a curated lookup table
- [ ] Show publisher (from Authenticode signature) per process
- [ ] **Publisher blocking**: block all apps from a publisher in one click
- [ ] **Permanent app disable** + custom rules (persisted)
- [ ] Notify on new / unusual app launch

## Phase 4 — Privacy & sensor alerts

Detect and alert on camera / microphone / location access.

- [ ] Read Windows `CapabilityAccessManager\ConsentStore` registry keys
      (webcam, microphone, location — `LastUsedTimeStart/Stop` per app)
- [ ] **Event tab**: timeline of app launches + sensor accesses
- [ ] Real-time notification when an app starts using camera/mic/location
- [ ] Tie sensor events into the Phase 2 timeline

## Phase 5 — Hardware temperature monitoring

- [ ] CPU / GPU temperature via LibreHardwareMonitor backend or WMI
      (note: usually needs admin; design a graceful fallback when unavailable)
- [ ] Temperature history on the shared timeline ("when did it start overheating?")

## Phase 6 — AI / MCP extension

- [ ] Optional MCP server exposing live + historical system data
- [ ] Natural-language queries from Claude / Cursor / Gemini
      ("what used my CPU at 3am?", "is anything using my mic?")

## Phase 7 — Polish & distribution

- [ ] System tray icon + run in background + autostart option
- [ ] Settings persistence (refresh rate, retention, rules)
- [ ] Installer (MSI / NSIS) via `tauri build`
- [ ] Performance pass — keep RAM well under AppControl's ~150MB
- [ ] App icon / branding

---

## Tech notes & risks

- **Timeline storage**: SQLite keeps memory flat over 3 days; in-memory-only would balloon RAM.
- **Temperatures & some sensor data** need elevated privileges or extra drivers — isolate behind a capability check so the app degrades gracefully.
- **CPU %**: `sysinfo` reports per-core; we normalize by core count for a 0–100 machine-wide figure.
- **GPU usage** on Windows: needs PDH counters / DXGI — slot into Phase 2 once the storage layer exists.

## Suggested order

`Phase 1 → 2 → 4 → 3 → 5 → 6 → 7`
(Privacy alerts (4) before full app-control (3): higher user value, lower complexity.)
