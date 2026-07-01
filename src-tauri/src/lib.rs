// Strix — Tauri backend
//
// A background thread samples the system every SAMPLE_SECS, caches the latest
// snapshot (so `get_snapshot` is cheap) and persists metrics + the heaviest
// processes to SQLite with a rolling retention window, powering the timeline.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::Serialize;
use sysinfo::{Pid, ProcessesToUpdate, System, Users};
use tauri::{Manager, State};

const SAMPLE_SECS: u64 = 2;
const RETENTION_SECS: i64 = 3 * 24 * 60 * 60; // 3 days
const TOP_N: usize = 8; // heaviest processes stored per sample
const PRUNE_EVERY: u32 = 60; // prune old rows every N samples

// --- shared state -----------------------------------------------------------

struct Shared {
    sys: Mutex<System>,
    users: Users,
    db: Mutex<Connection>,
    latest: RwLock<Snapshot>,
    blocks: RwLock<BlockSet>,
    /// Latest CPU/board temps from WMI (sampler-populated; empty unless elevated).
    cpu_temp: RwLock<Vec<TempSensor>>,
    /// Latest temps from the LibreHardwareMonitor helper (real per-core CPU,
    /// GPU, board), sampler-populated. Primary source when non-empty.
    lhm_temp: RwLock<Vec<TempSensor>>,
    /// Path to the bundled strix-sensors.exe helper, if found.
    helper_path: Option<PathBuf>,
}

/// In-memory blocklist (lowercased) for fast enforcement in the sampler.
#[derive(Default)]
struct BlockSet {
    exes: HashSet<String>,
    publishers: HashSet<String>,
}

impl BlockSet {
    fn is_empty(&self) -> bool {
        self.exes.is_empty() && self.publishers.is_empty()
    }
}

// --- serializable payloads --------------------------------------------------

#[derive(Serialize, Clone)]
struct ProcInfo {
    pid: u32,
    name: String,
    cpu: f32,
    memory: u64,
    disk_read: u64,
    disk_write: u64,
    user: String,
    exe: String,
    status: String,
    run_time: u64,
}

#[derive(Serialize, Clone)]
struct SystemSummary {
    cpu: f32,
    mem_used: u64,
    mem_total: u64,
    process_count: usize,
    cpu_count: usize,
}

#[derive(Serialize, Clone)]
struct Snapshot {
    summary: SystemSummary,
    processes: Vec<ProcInfo>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            summary: SystemSummary {
                cpu: 0.0,
                mem_used: 0,
                mem_total: 0,
                process_count: 0,
                cpu_count: 1,
            },
            processes: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct MetricPoint {
    ts: i64,
    cpu: f32,
    mem_used: u64,
    mem_total: u64,
    disk_read: u64,
    disk_write: u64,
    temp: f32,
}

#[derive(Serialize)]
struct HistoryWindow {
    min_ts: Option<i64>,
    max_ts: Option<i64>,
    retention_secs: i64,
    sample_secs: u64,
}

#[derive(Serialize)]
struct TopProc {
    name: String,
    cpu: f32,
    memory: u64,
}

// --- helpers ----------------------------------------------------------------

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Refresh `sys` and build a full snapshot (summary + sorted process list).
fn build_snapshot(sys: &mut System, users: &Users) -> Snapshot {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let cpu_count = sys.cpus().len().max(1);

    let mut processes: Vec<ProcInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| {
            let disk = p.disk_usage();
            let user = p
                .user_id()
                .and_then(|uid| users.get_user_by_id(uid))
                .map(|u| u.name().to_string())
                .unwrap_or_default();
            ProcInfo {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().to_string(),
                cpu: p.cpu_usage() / cpu_count as f32,
                memory: p.memory(),
                disk_read: disk.read_bytes,
                disk_write: disk.written_bytes,
                user,
                exe: p
                    .exe()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default(),
                status: p.status().to_string(),
                run_time: p.run_time(),
            }
        })
        .collect();

    processes.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));

    let summary = SystemSummary {
        cpu: sys.global_cpu_usage(),
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        process_count: processes.len(),
        cpu_count,
    };

    Snapshot { summary, processes }
}

fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metrics (
            ts          INTEGER PRIMARY KEY,
            cpu         REAL    NOT NULL,
            mem_used    INTEGER NOT NULL,
            mem_total   INTEGER NOT NULL,
            disk_read   INTEGER NOT NULL,
            disk_write  INTEGER NOT NULL,
            temp        REAL    NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS top_procs (
            ts      INTEGER NOT NULL,
            name    TEXT    NOT NULL,
            cpu     REAL    NOT NULL,
            memory  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_top_ts ON top_procs(ts);
        CREATE TABLE IF NOT EXISTS blocklist (
            kind    TEXT NOT NULL,
            value   TEXT NOT NULL,
            PRIMARY KEY (kind, value)
        );",
    )?;
    // Migrate DBs created before the temp column existed (errors if present).
    let _ = conn.execute("ALTER TABLE metrics ADD COLUMN temp REAL NOT NULL DEFAULT 0", []);
    Ok(())
}

fn load_blocks(conn: &Connection) -> rusqlite::Result<BlockSet> {
    let mut bs = BlockSet::default();
    let mut stmt = conn.prepare("SELECT kind, value FROM blocklist")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, value) = row?;
        match kind.as_str() {
            "exe" => {
                bs.exes.insert(value.to_lowercase());
            }
            "publisher" => {
                bs.publishers.insert(value.to_lowercase());
            }
            _ => {}
        }
    }
    Ok(bs)
}

/// Kill any running process whose exe name or publisher is on the blocklist.
/// Publisher lookups are cached by exe path to avoid re-reading version info.
fn enforce_blocks(sys: &System, blocks: &BlockSet, pub_cache: &mut HashMap<String, String>) {
    if blocks.is_empty() {
        return;
    }
    for (_pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_lowercase();
        let mut blocked = blocks.exes.contains(&name);

        if !blocked && !blocks.publishers.is_empty() {
            if let Some(exe) = p.exe() {
                let key = exe.to_string_lossy().to_string();
                let company = pub_cache
                    .entry(key)
                    .or_insert_with(|| read_version_info(&exe.to_string_lossy()).company)
                    .clone();
                if !company.is_empty() && blocks.publishers.contains(&company.to_lowercase()) {
                    blocked = true;
                }
            }
        }

        if blocked {
            let _ = p.kill();
        }
    }
}

/// Persist one sample: the system metrics row plus the heaviest processes.
fn persist_sample(conn: &Connection, ts: i64, snap: &Snapshot, cpu_temp: f64) -> rusqlite::Result<()> {
    let s = &snap.summary;
    let (disk_read, disk_write) = snap
        .processes
        .iter()
        .fold((0u64, 0u64), |(r, w), p| (r + p.disk_read, w + p.disk_write));

    conn.execute(
        "INSERT OR REPLACE INTO metrics (ts, cpu, mem_used, mem_total, disk_read, disk_write, temp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![ts, s.cpu, s.mem_used, s.mem_total, disk_read, disk_write, cpu_temp],
    )?;

    for p in snap.processes.iter().take(TOP_N) {
        conn.execute(
            "INSERT INTO top_procs (ts, name, cpu, memory) VALUES (?1, ?2, ?3, ?4)",
            params![ts, p.name, p.cpu, p.memory],
        )?;
    }
    Ok(())
}

/// Pick a representative CPU temperature for the timeline: prefer a "package"
/// sensor, else any CPU-labelled sensor.
fn pick_cpu_temp(sensors: &[TempSensor]) -> Option<f64> {
    let is = |s: &TempSensor, kw: &str| s.kind == "temp" && s.label.to_lowercase().contains(kw);
    sensors
        .iter()
        .find(|s| is(s, "package"))
        .or_else(|| sensors.iter().find(|s| is(s, "cpu")))
        .map(|s| s.value)
}

fn prune(conn: &Connection, cutoff: i64) {
    let _ = conn.execute("DELETE FROM metrics WHERE ts < ?1", params![cutoff]);
    let _ = conn.execute("DELETE FROM top_procs WHERE ts < ?1", params![cutoff]);
}

/// Background sampling loop. Runs for the lifetime of the app.
fn sampler_loop(shared: Arc<Shared>) {
    let mut tick: u32 = 0;
    let mut lhm_tick: u32 = 0;
    let mut pub_cache: HashMap<String, String> = HashMap::new();

    // WMI connection for ACPI temperatures, set up once on this thread. Works
    // only when the process is elevated; the per-cycle query fails otherwise.
    #[cfg(windows)]
    let wmi_thermal: Option<wmi::WMIConnection> = {
        use wmi::{COMLibrary, WMIConnection};
        COMLibrary::new()
            .ok()
            .and_then(|com| WMIConnection::with_namespace_path("ROOT\\WMI", com).ok())
    };

    loop {
        std::thread::sleep(Duration::from_secs(SAMPLE_SECS));

        let snap = {
            let mut sys = shared.sys.lock().unwrap();
            let s = build_snapshot(&mut sys, &shared.users);
            if let Ok(blocks) = shared.blocks.read() {
                enforce_blocks(&sys, &blocks, &mut pub_cache);
            }
            s
        };

        #[cfg(windows)]
        {
            let acpi = read_acpi_temps(wmi_thermal.as_ref());
            if !acpi.is_empty() {
                if let Ok(mut slot) = shared.cpu_temp.write() {
                    *slot = acpi;
                }
            }

            // The LHM helper is heavier (~1s: driver + enumeration), so poll it
            // less often than the base sample interval.
            if let Some(helper) = &shared.helper_path {
                if lhm_tick % 3 == 0 {
                    let lhm = read_lhm_temps(helper);
                    // Keep the last non-empty reading so a transient empty result
                    // (e.g. an Optimus dGPU briefly powering down) doesn't blank
                    // the Temps view.
                    if !lhm.is_empty() {
                        if let Ok(mut slot) = shared.lhm_temp.write() {
                            *slot = lhm;
                        }
                    }
                }
                lhm_tick = lhm_tick.wrapping_add(1);
            }
        }

        if let Ok(mut latest) = shared.latest.write() {
            *latest = snap.clone();
        }

        // Representative CPU temp for the timeline (LHM preferred, else ACPI).
        let cpu_temp_val = {
            let from_lhm = shared.lhm_temp.read().ok().and_then(|v| pick_cpu_temp(&v));
            from_lhm
                .or_else(|| shared.cpu_temp.read().ok().and_then(|v| pick_cpu_temp(&v)))
                .unwrap_or(0.0)
        };

        let ts = now_ts();
        if let Ok(conn) = shared.db.lock() {
            let _ = persist_sample(&conn, ts, &snap, cpu_temp_val);
            tick = tick.wrapping_add(1);
            if tick % PRUNE_EVERY == 0 {
                prune(&conn, ts - RETENTION_SECS);
            }
        }
    }
}

// --- commands ---------------------------------------------------------------

/// Return the most recent cached snapshot (cheap; no system refresh here).
#[tauri::command]
fn get_snapshot(state: State<'_, Arc<Shared>>) -> Snapshot {
    state.latest.read().map(|s| s.clone()).unwrap_or_default()
}

/// Terminate a process by PID.
#[tauri::command]
fn kill_process(pid: u32, state: State<'_, Arc<Shared>>) -> Result<bool, String> {
    let sys = state.sys.lock().unwrap();
    match sys.process(Pid::from_u32(pid)) {
        Some(p) => Ok(p.kill()),
        None => Err(format!("Process {pid} not found")),
    }
}

/// The available history range and sampling parameters.
#[tauri::command]
fn get_history_window(state: State<'_, Arc<Shared>>) -> Result<HistoryWindow, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let (min_ts, max_ts): (Option<i64>, Option<i64>) = conn
        .query_row("SELECT MIN(ts), MAX(ts) FROM metrics", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    Ok(HistoryWindow {
        min_ts,
        max_ts,
        retention_secs: RETENTION_SECS,
        sample_secs: SAMPLE_SECS,
    })
}

/// Metric points between `from`..`to`, optionally bucketed (averaged) into
/// `bucket_secs`-wide buckets to keep the payload small for wide ranges.
#[tauri::command]
fn get_history(
    from: i64,
    to: i64,
    bucket_secs: i64,
    state: State<'_, Arc<Shared>>,
) -> Result<Vec<MetricPoint>, String> {
    let bucket = bucket_secs.max(1);
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT (ts / ?1) * ?1 AS bucket,
                    AVG(cpu), AVG(mem_used), MAX(mem_total),
                    AVG(disk_read), AVG(disk_write), AVG(temp)
             FROM metrics
             WHERE ts BETWEEN ?2 AND ?3
             GROUP BY bucket
             ORDER BY bucket",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![bucket, from, to], |r| {
            Ok(MetricPoint {
                ts: r.get(0)?,
                cpu: r.get::<_, f64>(1)? as f32,
                mem_used: r.get::<_, f64>(2)? as u64,
                mem_total: r.get::<_, i64>(3)? as u64,
                disk_read: r.get::<_, f64>(4)? as u64,
                disk_write: r.get::<_, f64>(5)? as u64,
                temp: r.get::<_, f64>(6)? as f32,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

/// The heaviest processes recorded at the sample nearest to (and at or before) `ts`.
#[tauri::command]
fn get_top_at(ts: i64, state: State<'_, Arc<Shared>>) -> Result<Vec<TopProc>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let sample_ts: Option<i64> = conn
        .query_row(
            "SELECT ts FROM metrics WHERE ts <= ?1 ORDER BY ts DESC LIMIT 1",
            params![ts],
            |r| r.get(0),
        )
        .ok();

    let Some(sample_ts) = sample_ts else {
        return Ok(Vec::new());
    };

    let mut stmt = conn
        .prepare("SELECT name, cpu, memory FROM top_procs WHERE ts = ?1 ORDER BY cpu DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![sample_ts], |r| {
            Ok(TopProc {
                name: r.get(0)?,
                cpu: r.get::<_, f64>(1)? as f32,
                memory: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

// --- app details ------------------------------------------------------------

#[derive(Serialize, Default)]
struct VersionInfo {
    company: String,
    product: String,
    description: String,
    version: String,
}

/// Read CompanyName / ProductName / FileDescription / FileVersion from an exe's
/// version resource via the Windows version.dll APIs.
#[cfg(windows)]
fn read_version_info(path: &str) -> VersionInfo {
    use std::ffi::{c_void, OsStr};
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let mut out = VersionInfo::default();
    if path.is_empty() {
        return out;
    }
    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(once(0)).collect();

    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return out;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr() as *mut c_void) == 0 {
            return out;
        }

        // Determine the language/codepage of the string table.
        let (mut lang, mut cp) = (0x0409u16, 0x04b0u16);
        let tr_key: Vec<u16> = OsStr::new("\\VarFileInfo\\Translation")
            .encode_wide()
            .chain(once(0))
            .collect();
        let mut tr_ptr: *mut c_void = std::ptr::null_mut();
        let mut tr_len = 0u32;
        if VerQueryValueW(
            buf.as_ptr() as *const c_void,
            tr_key.as_ptr(),
            &mut tr_ptr,
            &mut tr_len,
        ) != 0
            && tr_len >= 4
            && !tr_ptr.is_null()
        {
            let arr = std::slice::from_raw_parts(tr_ptr as *const u16, 2);
            lang = arr[0];
            cp = arr[1];
        }

        let query = |field: &str| -> String {
            let sub = format!("\\StringFileInfo\\{:04x}{:04x}\\{}", lang, cp, field);
            let subw: Vec<u16> = OsStr::new(&sub).encode_wide().chain(once(0)).collect();
            let mut p: *mut c_void = std::ptr::null_mut();
            let mut l = 0u32;
            if VerQueryValueW(buf.as_ptr() as *const c_void, subw.as_ptr(), &mut p, &mut l) != 0
                && l > 0
                && !p.is_null()
            {
                let s = std::slice::from_raw_parts(p as *const u16, l as usize);
                let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                String::from_utf16_lossy(&s[..end])
            } else {
                String::new()
            }
        };

        out.company = query("CompanyName");
        out.product = query("ProductName");
        out.description = query("FileDescription");
        out.version = query("FileVersion");
    }
    out
}

#[cfg(not(windows))]
fn read_version_info(_path: &str) -> VersionInfo {
    VersionInfo::default()
}

#[derive(Serialize)]
struct AppDetails {
    pid: u32,
    name: String,
    exe: String,
    cmd: String,
    cwd: String,
    parent_pid: Option<u32>,
    parent_name: String,
    user: String,
    status: String,
    start_time: i64,
    run_time: u64,
    memory: u64,
    cpu: f32,
    disk_read: u64,
    disk_write: u64,
    version_info: VersionInfo,
}

/// Rich, on-demand detail for a single process (read from the cached System).
#[tauri::command]
fn get_app_details(pid: u32, state: State<'_, Arc<Shared>>) -> Result<AppDetails, String> {
    let sys = state.sys.lock().unwrap();
    let p = sys
        .process(Pid::from_u32(pid))
        .ok_or_else(|| format!("Process {pid} not found"))?;

    let cpu_count = sys.cpus().len().max(1) as f32;
    let parent = p.parent();
    let parent_name = parent
        .and_then(|pp| sys.process(pp))
        .map(|pp| pp.name().to_string_lossy().to_string())
        .unwrap_or_default();
    let user = p
        .user_id()
        .and_then(|uid| state.users.get_user_by_id(uid))
        .map(|u| u.name().to_string())
        .unwrap_or_default();
    let disk = p.disk_usage();
    let exe = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let version_info = read_version_info(&exe);

    Ok(AppDetails {
        pid,
        name: p.name().to_string_lossy().to_string(),
        exe,
        cmd: p
            .cmd()
            .iter()
            .map(|c| c.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        cwd: p.cwd().map(|e| e.to_string_lossy().to_string()).unwrap_or_default(),
        parent_pid: parent.map(|x| x.as_u32()),
        parent_name,
        user,
        status: p.status().to_string(),
        start_time: p.start_time() as i64,
        run_time: p.run_time(),
        memory: p.memory(),
        cpu: p.cpu_usage() / cpu_count,
        disk_read: disk.read_bytes,
        disk_write: disk.written_bytes,
        version_info,
    })
}

// --- blocklist (block / permanently disable apps) ---------------------------

#[derive(Serialize)]
struct Block {
    kind: String,
    value: String,
}

#[tauri::command]
fn get_blocks(state: State<'_, Arc<Shared>>) -> Result<Vec<Block>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT kind, value FROM blocklist ORDER BY kind, value")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Block {
                kind: r.get(0)?,
                value: r.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn add_block(kind: String, value: String, state: State<'_, Arc<Shared>>) -> Result<(), String> {
    if kind != "exe" && kind != "publisher" {
        return Err("kind must be 'exe' or 'publisher'".into());
    }
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err("value is empty".into());
    }
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO blocklist (kind, value) VALUES (?1, ?2)",
            params![kind, value],
        )
        .map_err(|e| e.to_string())?;
    }
    let mut bs = state.blocks.write().map_err(|e| e.to_string())?;
    if kind == "exe" {
        bs.exes.insert(value.to_lowercase());
    } else {
        bs.publishers.insert(value.to_lowercase());
    }
    Ok(())
}

#[tauri::command]
fn remove_block(kind: String, value: String, state: State<'_, Arc<Shared>>) -> Result<(), String> {
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM blocklist WHERE kind = ?1 AND value = ?2",
            params![kind, value],
        )
        .map_err(|e| e.to_string())?;
    }
    let mut bs = state.blocks.write().map_err(|e| e.to_string())?;
    let lower = value.to_lowercase();
    if kind == "exe" {
        bs.exes.remove(&lower);
    } else {
        bs.publishers.remove(&lower);
    }
    Ok(())
}

// --- temperatures (HWiNFO shared memory) ------------------------------------
// HWiNFO publishes all sensor readings to the named mapping
// "Global\HWiNFO_SENS_SM2" when "Shared Memory Support" is enabled. We read
// the temperature readings (type == 1) without any driver of our own.

#[derive(Serialize, Clone)]
struct TempSensor {
    label: String,
    value: f64,
    min: f64,
    max: f64,
    /// "temp" (°C) or "fan" (RPM).
    kind: String,
}

#[derive(Serialize)]
struct Temperatures {
    /// True if HWiNFO's shared memory is present (HWiNFO running w/ SHM).
    available: bool,
    sensors: Vec<TempSensor>,
}

#[cfg(windows)]
fn read_hwinfo_temps() -> Temperatures {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Memory::{
        MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
    };

    let mut out = Temperatures {
        available: false,
        sensors: Vec::new(),
    };
    let name: Vec<u16> = OsStr::new("Global\\HWiNFO_SENS_SM2")
        .encode_wide()
        .chain(once(0))
        .collect();

    unsafe {
        let handle = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
        if handle.is_null() {
            return out; // HWiNFO not running with shared memory
        }
        let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
        if view.Value.is_null() {
            CloseHandle(handle);
            return out;
        }
        // The mapping exists, so HWiNFO is publishing.
        out.available = true;
        let base = view.Value as *const u8;

        let rd_u32 = |off: usize| -> u32 {
            let mut b = [0u8; 4];
            std::ptr::copy_nonoverlapping(base.add(off), b.as_mut_ptr(), 4);
            u32::from_le_bytes(b)
        };
        let rd_f64 = |off: usize| -> f64 {
            let mut b = [0u8; 8];
            std::ptr::copy_nonoverlapping(base.add(off), b.as_mut_ptr(), 8);
            f64::from_le_bytes(b)
        };
        let rd_cstr = |off: usize, max: usize| -> String {
            let mut v = Vec::new();
            for i in 0..max {
                let c = *base.add(off + i);
                if c == 0 {
                    break;
                }
                v.push(c);
            }
            String::from_utf8_lossy(&v).into_owned()
        };

        // Header (see HWiNFO SDK): reading-section layout at fixed offsets.
        let offset_reading = rd_u32(36) as usize;
        let size_reading = rd_u32(40) as usize;
        let num_reading = rd_u32(44) as usize;

        // Sanity-gate the geometry before walking the section.
        if (200..2000).contains(&size_reading) && num_reading < 100_000 {
            for i in 0..num_reading {
                let b = offset_reading + i * size_reading;
                if rd_u32(b) != 1 {
                    continue; // 1 == SENSOR_TYPE_TEMP
                }
                let user = rd_cstr(b + 140, 128);
                let orig = rd_cstr(b + 12, 128);
                let label = if user.is_empty() { orig } else { user };
                let value = rd_f64(b + 288);
                if !value.is_finite() || !(-50.0..=200.0).contains(&value) {
                    continue;
                }
                out.sensors.push(TempSensor {
                    label,
                    value,
                    min: rd_f64(b + 296),
                    max: rd_f64(b + 304),
                    kind: "temp".to_string(),
                });
            }
        }

        UnmapViewOfFile(view);
        CloseHandle(handle);
    }
    out
}

#[cfg(not(windows))]
fn read_hwinfo_temps() -> Temperatures {
    Temperatures {
        available: false,
        sensors: Vec::new(),
    }
}

/// Session min/max accumulator for point-in-time temps (nvidia-smi / ACPI give
/// only the current value). Keyed by sensor label.
fn temp_minmax(label: &str, value: f64) -> (f64, f64) {
    static CACHE: OnceLock<Mutex<HashMap<String, (f64, f64)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = cache.lock().unwrap();
    let e = m.entry(label.to_string()).or_insert((value, value));
    e.0 = e.0.min(value);
    e.1 = e.1.max(value);
    *e
}

/// GPU temperature via `nvidia-smi` (ships with the NVIDIA driver, no admin,
/// no persistent process — invoked once per poll and exits immediately).
#[cfg(windows)]
fn read_nvidia_temps() -> Vec<TempSensor> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut sensors = Vec::new();
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,temperature.gpu", "--format=csv,noheader,nounits"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(o) = output {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                if parts.len() < 2 {
                    continue;
                }
                let Ok(temp) = parts[1].parse::<f64>() else {
                    continue;
                };
                let label = format!("GPU: {}", parts[0]);
                let (min, max) = temp_minmax(&label, temp);
                sensors.push(TempSensor {
                    label,
                    value: temp,
                    min,
                    max,
                    kind: "temp".to_string(),
                });
            }
        }
    }
    sensors
}

#[cfg(not(windows))]
fn read_nvidia_temps() -> Vec<TempSensor> {
    Vec::new()
}

/// CPU/board temperature via WMI `MSAcpi_ThermalZoneTemperature`. This works
/// only when Strix runs elevated (admin); otherwise the query is denied
/// and we return nothing. Coarse ACPI thermal zones, not per-core sensors.
#[cfg(windows)]
fn read_acpi_temps(conn: Option<&wmi::WMIConnection>) -> Vec<TempSensor> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename = "MSAcpi_ThermalZoneTemperature")]
    #[serde(rename_all = "PascalCase")]
    struct Zone {
        instance_name: String,
        current_temperature: u32,
    }

    let Some(conn) = conn else {
        return Vec::new();
    };
    let rows: Vec<Zone> = match conn.query() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    rows.into_iter()
        .filter_map(|z| {
            // CurrentTemperature is in tenths of a Kelvin.
            let c = z.current_temperature as f64 / 10.0 - 273.15;
            if !(-50.0..=200.0).contains(&c) {
                return None;
            }
            let c = (c * 10.0).round() / 10.0;
            let zone = z
                .instance_name
                .rsplit('\\')
                .next()
                .unwrap_or(&z.instance_name)
                .to_string();
            let label = format!("CPU/ACPI: {zone}");
            let (min, max) = temp_minmax(&label, c);
            Some(TempSensor {
                label,
                value: c,
                min,
                max,
                kind: "temp".to_string(),
            })
        })
        .collect()
}

/// Run the bundled LibreHardwareMonitor helper and parse its JSON output.
/// Returns real per-core CPU / GPU / board temperatures — the CPU data needs
/// Strix (and thus this child) to be elevated; otherwise only GPU is reported.
fn read_lhm_temps(helper: &Path) -> Vec<TempSensor> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Raw {
        label: String,
        value: f64,
        #[serde(default = "default_kind")]
        kind: String,
    }
    fn default_kind() -> String {
        "temp".to_string()
    }

    let mut cmd = std::process::Command::new(helper);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let raws: Vec<Raw> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    raws.into_iter()
        .filter(|r| {
            r.value.is_finite()
                && if r.kind == "fan" {
                    (0.0..=100_000.0).contains(&r.value)
                } else {
                    (-50.0..=200.0).contains(&r.value)
                }
        })
        .map(|r| {
            let (min, max) = temp_minmax(&r.label, r.value);
            TempSensor {
                label: r.label,
                value: r.value,
                min,
                max,
                kind: r.kind,
            }
        })
        .collect()
}

/// Combined temperatures. Prefers the LibreHardwareMonitor helper (real
/// per-core CPU + GPU + board); otherwise falls back to HWiNFO shared memory,
/// the cached ACPI reading (elevated) and nvidia-smi.
#[tauri::command]
fn get_temperatures(state: State<'_, Arc<Shared>>) -> Temperatures {
    let lhm = state.lhm_temp.read().map(|v| v.clone()).unwrap_or_default();
    if !lhm.is_empty() {
        return Temperatures {
            available: true,
            sensors: lhm,
        };
    }

    let mut out = read_hwinfo_temps();
    let acpi = state.cpu_temp.read().map(|v| v.clone()).unwrap_or_default();
    let gpu = read_nvidia_temps();
    if !acpi.is_empty() || !gpu.is_empty() {
        out.available = true;
    }
    out.sensors.extend(acpi);
    out.sensors.extend(gpu);
    out
}

// --- privacy / sensor access (Windows ConsentStore) -------------------------

#[derive(Serialize, Clone)]
struct SensorAccess {
    /// "webcam" | "microphone" | "location"
    capability: String,
    /// Readable app name (exe filename or package family name).
    app: String,
    /// Full exe path or package id.
    path: String,
    /// Unix seconds of the last time the app started using the sensor.
    last_used: i64,
    /// True if the app is using the sensor right now (stop time not yet set).
    in_use: bool,
}

/// Windows FILETIME (100ns ticks since 1601) -> unix seconds.
fn filetime_to_unix(ft: u64) -> i64 {
    if ft == 0 {
        return 0;
    }
    (ft / 10_000_000) as i64 - 11_644_473_600
}

/// NonPackaged keys encode the exe path with '#' instead of '\\'.
fn nonpackaged_name(raw: &str) -> (String, String) {
    let path = raw.replace('#', "\\");
    let app = path.rsplit('\\').next().unwrap_or(&path).to_string();
    (app, path)
}

fn push_access(
    key: &winreg::RegKey,
    cap: &str,
    raw: &str,
    nonpackaged: bool,
    out: &mut Vec<SensorAccess>,
) {
    let start: u64 = key.get_value("LastUsedTimeStart").unwrap_or(0);
    let stop: u64 = key.get_value("LastUsedTimeStop").unwrap_or(0);
    if start == 0 && stop == 0 {
        return; // no usage recorded
    }
    let (app, path) = if nonpackaged {
        nonpackaged_name(raw)
    } else {
        (raw.split('_').next().unwrap_or(raw).to_string(), raw.to_string())
    };
    out.push(SensorAccess {
        capability: cap.to_string(),
        app,
        path,
        last_used: filetime_to_unix(start),
        in_use: stop == 0 && start != 0,
    });
}

fn read_consent_for(hive: winreg::HKEY, cap: &str, out: &mut Vec<SensorAccess>) {
    let root = winreg::RegKey::predef(hive);
    let path = format!(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{cap}"
    );
    let Ok(cap_key) = root.open_subkey(&path) else {
        return;
    };
    for sub in cap_key.enum_keys().flatten() {
        if sub.eq_ignore_ascii_case("NonPackaged") {
            if let Ok(np) = cap_key.open_subkey("NonPackaged") {
                for app_raw in np.enum_keys().flatten() {
                    if let Ok(k) = np.open_subkey(&app_raw) {
                        push_access(&k, cap, &app_raw, true, out);
                    }
                }
            }
        } else if let Ok(k) = cap_key.open_subkey(&sub) {
            push_access(&k, cap, &sub, false, out);
        }
    }
}

/// Which apps have used the camera / microphone / location, and which are
/// using them right now. Read from the Windows ConsentStore registry.
#[tauri::command]
fn get_sensor_access() -> Vec<SensorAccess> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut out = Vec::new();
    for cap in ["webcam", "microphone", "location"] {
        read_consent_for(HKEY_CURRENT_USER, cap, &mut out);
        read_consent_for(HKEY_LOCAL_MACHINE, cap, &mut out);
    }
    // Active sensors first, then most recently used.
    out.sort_by(|a, b| b.in_use.cmp(&a.in_use).then(b.last_used.cmp(&a.last_used)));
    out
}

// --- window / tray helpers --------------------------------------------------

/// Show, unminimize and focus the main window (from tray actions).
fn reveal_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// --- elevation --------------------------------------------------------------

/// Relaunch Strix elevated (UAC) so the WMI ACPI temperature query works,
/// then exit this non-elevated instance.
#[tauri::command]
fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::iter::once;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;

        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(once(0)).collect();
        let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(once(0)).collect();

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                exe_w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };
        // ShellExecute returns a value > 32 on success.
        if (result as isize) <= 32 {
            return Err("Elevation was cancelled or failed".into());
        }
        app.exit(0);
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(())
}

/// Whether Strix is currently running with administrator rights.
#[tauri::command]
fn is_elevated() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
    #[cfg(not(windows))]
    false
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            kill_process,
            get_history_window,
            get_history,
            get_top_at,
            get_sensor_access,
            get_app_details,
            get_blocks,
            add_block,
            remove_block,
            get_temperatures,
            restart_as_admin,
            is_elevated
        ])
        .setup(|app| {
            // Database lives in the app's local data directory.
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = Connection::open(dir.join("strix.db"))?;
            init_db(&conn)?;
            let block_set = load_blocks(&conn).unwrap_or_default();

            // Locate the bundled LibreHardwareMonitor helper (best-effort).
            let helper_path = app
                .path()
                .resource_dir()
                .ok()
                .and_then(|base| {
                    [
                        base.join("resources").join("sensors").join("strix-sensors.exe"),
                        base.join("sensors").join("strix-sensors.exe"),
                        base.join("strix-sensors.exe"),
                    ]
                    .into_iter()
                    .find(|p| p.exists())
                })
                .or_else(|| {
                    // Dev fallback: the source-tree resources folder.
                    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("resources")
                        .join("sensors")
                        .join("strix-sensors.exe");
                    dev.exists().then_some(dev)
                });

            let mut sys = System::new_all();
            sys.refresh_cpu_usage();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let users = Users::new_with_refreshed_list();
            let initial = build_snapshot(&mut sys, &users);

            let shared = Arc::new(Shared {
                sys: Mutex::new(sys),
                users,
                db: Mutex::new(conn),
                latest: RwLock::new(initial),
                blocks: RwLock::new(block_set),
                cpu_temp: RwLock::new(Vec::new()),
                lhm_temp: RwLock::new(Vec::new()),
                helper_path,
            });

            let worker = shared.clone();
            std::thread::spawn(move || sampler_loop(worker));

            app.manage(shared);

            // System tray with Show / Quit, left-click to restore.
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show = MenuItem::with_id(app, "show", "Show Strix", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show, &quit])?;

                let _tray = TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Strix")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => reveal_main(app),
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            reveal_main(tray.app_handle());
                        }
                    })
                    .build(app)?;
            }

            // Closing the window hides it to the tray instead of quitting.
            if let Some(win) = app.get_webview_window("main") {
                let w = win.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
