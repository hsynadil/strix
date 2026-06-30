// perf-diag — Tauri backend
//
// A background thread samples the system every SAMPLE_SECS, caches the latest
// snapshot (so `get_snapshot` is cheap) and persists metrics + the heaviest
// processes to SQLite with a rolling retention window, powering the timeline.

use std::sync::{Arc, Mutex, RwLock};
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
            disk_write  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS top_procs (
            ts      INTEGER NOT NULL,
            name    TEXT    NOT NULL,
            cpu     REAL    NOT NULL,
            memory  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_top_ts ON top_procs(ts);",
    )
}

/// Persist one sample: the system metrics row plus the heaviest processes.
fn persist_sample(conn: &Connection, ts: i64, snap: &Snapshot) -> rusqlite::Result<()> {
    let s = &snap.summary;
    let (disk_read, disk_write) = snap
        .processes
        .iter()
        .fold((0u64, 0u64), |(r, w), p| (r + p.disk_read, w + p.disk_write));

    conn.execute(
        "INSERT OR REPLACE INTO metrics (ts, cpu, mem_used, mem_total, disk_read, disk_write)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ts, s.cpu, s.mem_used, s.mem_total, disk_read, disk_write],
    )?;

    for p in snap.processes.iter().take(TOP_N) {
        conn.execute(
            "INSERT INTO top_procs (ts, name, cpu, memory) VALUES (?1, ?2, ?3, ?4)",
            params![ts, p.name, p.cpu, p.memory],
        )?;
    }
    Ok(())
}

fn prune(conn: &Connection, cutoff: i64) {
    let _ = conn.execute("DELETE FROM metrics WHERE ts < ?1", params![cutoff]);
    let _ = conn.execute("DELETE FROM top_procs WHERE ts < ?1", params![cutoff]);
}

/// Background sampling loop. Runs for the lifetime of the app.
fn sampler_loop(shared: Arc<Shared>) {
    let mut tick: u32 = 0;
    loop {
        std::thread::sleep(Duration::from_secs(SAMPLE_SECS));

        let snap = {
            let mut sys = shared.sys.lock().unwrap();
            build_snapshot(&mut sys, &shared.users)
        };

        if let Ok(mut latest) = shared.latest.write() {
            *latest = snap.clone();
        }

        let ts = now_ts();
        if let Ok(conn) = shared.db.lock() {
            let _ = persist_sample(&conn, ts, &snap);
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
                    AVG(disk_read), AVG(disk_write)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            kill_process,
            get_history_window,
            get_history,
            get_top_at,
            get_sensor_access
        ])
        .setup(|app| {
            // Database lives in the app's local data directory.
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = Connection::open(dir.join("perf-diag.db"))?;
            init_db(&conn)?;

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
            });

            let worker = shared.clone();
            std::thread::spawn(move || sampler_loop(worker));

            app.manage(shared);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
