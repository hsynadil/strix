<div align="center">

# 🦉 Strix

**A lightweight Windows process & performance monitor — a smarter Task Manager.**

Live process insight, days of scrollable history, privacy alerts, temperatures,
and per-app control. Built with Tauri + Rust, so the native core sits around
**~48 MB** of RAM.

</div>

---

Strix is inspired by [AppControl](https://www.appcontrol.com/): it doesn't just
show you what's running *now*, it records what your PC has been doing and tells
you, in plain language, what each process is.

## Features

### 🔴 Live
- Process table with **CPU / RAM / Disk**, user, executable path, status and uptime
- Smooth, flicker-free updates (rows are diffed by PID — scroll position is kept)
- Sort any column, filter by name/user, adjustable refresh rate
- **Group by application** (collapse a program's child processes)
- Per-process **CPU & RAM sparklines** in the header
- **End Task** on any process

### 🕒 History (the headline feature)
- A background sampler records **CPU / RAM / Disk every ~2 s for up to 3 days** to SQLite
- A **timeline you can scrub**: hover any past moment to see exact values…
- …and a **"what spiked?"** panel showing the heaviest processes at that time

### 🔒 Privacy
- See which apps used the **camera, microphone or location** (Windows ConsentStore)
- A live banner pulses red when a sensor is **in use right now**, plus an access log

### 🌡️ Temperatures
- **GPU** temperature via `nvidia-smi` (no admin, no background app)
- **Real per-core CPU + board** temperatures via a bundled **LibreHardwareMonitor**
  engine — one click to *Restart as administrator* unlocks it
- Falls back to WMI ACPI thermal zones or HWiNFO shared memory when the engine
  isn't available

### 🧩 App details & control
- Right-click any process → **App Details**: plain-language description, **publisher**
  (from the exe's signature/version info), command line, parent process and more
- **Block** an app or an entire **publisher** — matching processes are terminated
  automatically while running (a persistent "disable")

### ⚙️ Quality of life
- **System tray** — closing the window keeps Strix recording in the background
- **Launch at startup** toggle, and persisted UI settings

> **Note on temperatures:** reading the real CPU sensor needs ring-0 (a kernel
> driver), so it requires running Strix **as administrator**. GPU temperature
> works without elevation on NVIDIA GPUs.

## Install

Grab the latest `Strix_x64-setup.exe` from the
[Releases](../../releases) page and run it. Requires the
[WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
(already present on Windows 10/11).

## Build from source

**Prerequisites**
- [Rust](https://rustup.rs/) (MSVC toolchain) + Visual Studio C++ Build Tools
- [Node.js](https://nodejs.org/) and [pnpm](https://pnpm.io/)

```bash
pnpm install
pnpm tauri dev      # run in development
pnpm tauri build --bundles nsis   # produce the release exe + installer
```

The standalone exe lands in `src-tauri/target/release/strix.exe`; the installer
in `src-tauri/target/release/bundle/nsis/`.

## Tech stack

| Layer    | Tech |
|----------|------|
| Shell    | [Tauri 2](https://tauri.app) (Rust) |
| Backend  | `sysinfo`, `rusqlite` (bundled SQLite), `winreg`, `wmi`, `windows-sys` |
| Frontend | Vanilla **TypeScript** + [Vite](https://vitejs.dev) (no UI framework) |

## How it works (the tricky bits)

- **History** is a background thread that samples the system on a fixed interval
  and writes metrics + the top processes to SQLite, with a rolling 3-day window.
- **Privacy** reads `HKCU/HKLM …\CapabilityAccessManager\ConsentStore` and decodes
  the `LastUsedTimeStart/Stop` FILETIME values to detect in-use sensors.
- **CPU temperature** is read by a small bundled helper (`strix-sensors.exe`,
  source in [`sensor-helper/`](sensor-helper/)) that uses **LibreHardwareMonitor**.
  Reading the CPU's sensor needs a kernel driver, so it only works when Strix runs
  elevated — hence the *Restart as administrator* button. A coarse WMI ACPI reading
  is used as a fallback.

## Status

Phases 0–5 and 7 of the [roadmap](ROADMAP.md) are complete. Phase 6 (an optional
MCP server so AI tools can query Strix's data in natural language) is future work.

## Third-party

Strix bundles **LibreHardwareMonitor** (MPL-2.0) and its dependencies to read
temperatures — see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## License

To be decided by the project owner.
