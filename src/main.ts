import { invoke } from "@tauri-apps/api/core";

interface ProcInfo {
  pid: number;
  name: string;
  cpu: number;
  memory: number;
  disk_read: number;
  disk_write: number;
  user: string;
  exe: string;
  status: string;
  run_time: number;
}

interface SystemSummary {
  cpu: number;
  mem_used: number;
  mem_total: number;
  process_count: number;
  cpu_count: number;
}

interface Snapshot {
  summary: SystemSummary;
  processes: ProcInfo[];
}

type SortKey = "name" | "user" | "pid" | "cpu" | "memory" | "disk";

interface GroupAgg {
  cpu: number;
  memory: number;
  disk: number;
  count: number;
  user: string;
  expanded: boolean;
}

type DisplayRow =
  | { key: string; kind: "proc"; proc: ProcInfo; indent: boolean }
  | { key: string; kind: "group"; name: string; agg: GroupAgg };

// --- state ------------------------------------------------------------------

let pollMs = 1500;
let pollTimer: number | undefined;

let sortKey: SortKey = "cpu";
let sortDir: "asc" | "desc" = "desc";
let filter = "";
let paused = false;
let groupBy = false;
const expanded = new Set<string>();
let latest: ProcInfo[] = [];

const cpuHist: number[] = [];
const memHist: number[] = [];
const HIST_LEN = 90;

// --- formatting helpers -----------------------------------------------------

function fmtBytes(n: number): string {
  if (n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  const v = n / Math.pow(1024, i);
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

function fmtRate(n: number): string {
  return n <= 0 ? "—" : fmtBytes(n) + "/s";
}

function fmtDuration(sec: number): string {
  if (sec <= 0) return "—";
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m ${sec % 60}s`;
  return `${sec}s`;
}

const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;

// --- view pipeline: filter -> (group) -> sort -------------------------------
// Everything that changes the view (poll, filter, sort, group, expand) funnels
// through buildRows(), so the active filter is always honored.

function dir(n: number): number {
  return sortDir === "asc" ? n : -n;
}

function cmpProc(a: ProcInfo, b: ProcInfo): number {
  switch (sortKey) {
    case "name": return dir(a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
    case "user": return dir((a.user || "").toLowerCase().localeCompare((b.user || "").toLowerCase()));
    case "pid": return dir(a.pid - b.pid);
    case "cpu": return dir(a.cpu - b.cpu);
    case "memory": return dir(a.memory - b.memory);
    case "disk": return dir((a.disk_read + a.disk_write) - (b.disk_read + b.disk_write));
  }
}

function matches(p: ProcInfo, f: string): boolean {
  return !f || p.name.toLowerCase().includes(f) || p.user.toLowerCase().includes(f);
}

function buildRows(): DisplayRow[] {
  const f = filter.trim().toLowerCase();
  const visible = latest.filter((p) => matches(p, f));

  if (!groupBy) {
    return visible
      .sort(cmpProc)
      .map((p) => ({ key: `p:${p.pid}`, kind: "proc", proc: p, indent: false }));
  }

  // Group visible processes by name.
  const groups = new Map<string, ProcInfo[]>();
  for (const p of visible) {
    const arr = groups.get(p.name);
    if (arr) arr.push(p);
    else groups.set(p.name, [p]);
  }

  const aggregated = [...groups.entries()].map(([name, procs]) => {
    const users = new Set(procs.map((p) => p.user || "—"));
    return {
      name,
      procs,
      cpu: procs.reduce((s, p) => s + p.cpu, 0),
      memory: procs.reduce((s, p) => s + p.memory, 0),
      disk: procs.reduce((s, p) => s + p.disk_read + p.disk_write, 0),
      minPid: Math.min(...procs.map((p) => p.pid)),
      count: procs.length,
      user: users.size === 1 ? [...users][0] : "multiple",
    };
  });

  aggregated.sort((a, b) => {
    switch (sortKey) {
      case "name": return dir(a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
      case "user": return dir(a.user.toLowerCase().localeCompare(b.user.toLowerCase()));
      case "pid": return dir(a.minPid - b.minPid);
      case "cpu": return dir(a.cpu - b.cpu);
      case "memory": return dir(a.memory - b.memory);
      case "disk": return dir(a.disk - b.disk);
    }
  });

  const out: DisplayRow[] = [];
  for (const g of aggregated) {
    const isOpen = expanded.has(g.name);
    out.push({
      key: `g:${g.name}`,
      kind: "group",
      name: g.name,
      agg: { cpu: g.cpu, memory: g.memory, disk: g.disk, count: g.count, user: g.user, expanded: isOpen },
    });
    if (isOpen) {
      for (const p of g.procs.slice().sort(cmpProc)) {
        out.push({ key: `p:${p.pid}`, kind: "proc", proc: p, indent: true });
      }
    }
  }
  return out;
}

// --- keyed row reconciliation (no flicker, preserves scroll) ----------------

interface RowRec {
  tr: HTMLTableRowElement;
  kind: "proc" | "group";
  name: HTMLTableCellElement;
  user: HTMLTableCellElement;
  pid: HTMLTableCellElement;
  cpu: HTMLTableCellElement;
  mem: HTMLTableCellElement;
  disk: HTMLTableCellElement;
  expander?: HTMLSpanElement;
}

const rows = new Map<string, RowRec>();

function mkCell(tr: HTMLTableRowElement, cls: string): HTMLTableCellElement {
  const td = document.createElement("td");
  if (cls) td.className = cls;
  tr.appendChild(td);
  return td;
}

function createProcRow(): RowRec {
  const tr = document.createElement("tr");
  const name = mkCell(tr, "name");
  const user = mkCell(tr, "muted");
  const pid = mkCell(tr, "num muted");
  const cpu = mkCell(tr, "num");
  const mem = mkCell(tr, "num");
  const disk = mkCell(tr, "num muted");
  const killTd = mkCell(tr, "num");
  const btn = document.createElement("button");
  btn.className = "kill";
  btn.textContent = "✕";
  btn.title = "End task";
  killTd.appendChild(btn);
  return { tr, kind: "proc", name, user, pid, cpu, mem, disk };
}

function createGroupRow(name: string): RowRec {
  const tr = document.createElement("tr");
  tr.className = "group";
  tr.dataset.name = name;
  const nameCell = mkCell(tr, "name");
  const expander = document.createElement("span");
  expander.className = "expander";
  const label = document.createElement("span");
  label.className = "glabel";
  const count = document.createElement("span");
  count.className = "count";
  nameCell.append(expander, label, count);
  const user = mkCell(tr, "muted");
  const pid = mkCell(tr, "num muted");
  const cpu = mkCell(tr, "num");
  const mem = mkCell(tr, "num");
  const disk = mkCell(tr, "num muted");
  mkCell(tr, "num");
  return { tr, kind: "group", name: label as unknown as HTMLTableCellElement, user, pid, cpu, mem, disk, expander };
}

function setText(el: { textContent: string | null }, t: string) {
  if (el.textContent !== t) el.textContent = t;
}

function cpuClass(v: number): string {
  return `num ${v >= 25 ? "hot" : v >= 5 ? "warm" : ""}`.trim();
}

function updateProcRow(r: RowRec, p: ProcInfo, indent: boolean) {
  setText(r.name, p.name);
  r.name.title = `${p.exe || p.name}\nStatus: ${p.status} · Uptime: ${fmtDuration(p.run_time)}`;
  r.name.classList.toggle("child", indent);
  const btn = r.tr.querySelector("button.kill") as HTMLElement | null;
  if (btn) btn.dataset.pid = String(p.pid);
  setText(r.user, p.user || "—");
  setText(r.pid, String(p.pid));
  setText(r.cpu, p.cpu.toFixed(1));
  if (r.cpu.className !== cpuClass(p.cpu)) r.cpu.className = cpuClass(p.cpu);
  setText(r.mem, fmtBytes(p.memory));
  setText(r.disk, fmtRate(p.disk_read + p.disk_write));
}

function updateGroupRow(r: RowRec, name: string, agg: GroupAgg) {
  if (r.expander) r.expander.textContent = agg.expanded ? "▾" : "▸";
  setText(r.name, name); // r.name is the label span here
  const count = r.tr.querySelector(".count");
  if (count) setText(count, ` ${agg.count}`);
  setText(r.user, agg.user);
  setText(r.pid, "");
  setText(r.cpu, agg.cpu.toFixed(1));
  if (r.cpu.className !== cpuClass(agg.cpu)) r.cpu.className = cpuClass(agg.cpu);
  setText(r.mem, fmtBytes(agg.memory));
  setText(r.disk, fmtRate(agg.disk));
}

function reconcile(list: DisplayRow[]) {
  const rowsEl = $("#rows");
  const desired = new Set(list.map((r) => r.key));

  for (const [key, rec] of rows) {
    if (!desired.has(key)) {
      rec.tr.remove();
      rows.delete(key);
    }
  }

  let prev: ChildNode | null = null;
  for (const row of list) {
    let rec = rows.get(row.key);
    if (!rec) {
      rec = row.kind === "group" ? createGroupRow(row.name) : createProcRow();
      rows.set(row.key, rec);
    }
    if (row.kind === "group") updateGroupRow(rec, row.name, row.agg);
    else updateProcRow(rec, row.proc, row.indent);

    const ref: ChildNode | null = prev ? prev.nextSibling : rowsEl.firstChild;
    if (rec.tr !== ref) rowsEl.insertBefore(rec.tr, ref);
    prev = rec.tr;
  }
}

function render() {
  reconcile(buildRows());
}

// --- sparklines -------------------------------------------------------------

function drawSpark(id: string, data: number[], max: number, color: string) {
  const canvas = document.getElementById(id) as HTMLCanvasElement | null;
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const w = canvas.width;
  const h = canvas.height;
  ctx.clearRect(0, 0, w, h);
  if (data.length < 2) return;

  const step = w / (HIST_LEN - 1);
  const y = (v: number) => h - 1 - (Math.min(v, max) / max) * (h - 2);
  const x = (i: number) => i * step;
  const start = HIST_LEN - data.length;

  ctx.beginPath();
  data.forEach((v, i) => {
    const px = x(start + i);
    const py = y(v);
    i === 0 ? ctx.moveTo(px, py) : ctx.lineTo(px, py);
  });
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = color;
  ctx.stroke();

  ctx.lineTo(x(HIST_LEN - 1), h);
  ctx.lineTo(x(start), h);
  ctx.closePath();
  ctx.fillStyle = color + "22";
  ctx.fill();
}

function pushHist(arr: number[], v: number) {
  arr.push(v);
  if (arr.length > HIST_LEN) arr.shift();
}

// --- summary ----------------------------------------------------------------

function renderSummary(s: SystemSummary) {
  const cpu = Math.min(100, Math.round(s.cpu));
  $("#cpu-value").textContent = `${cpu}%`;
  $<HTMLElement>("#cpu-bar").style.width = `${cpu}%`;

  const memPct = s.mem_total > 0 ? (s.mem_used / s.mem_total) * 100 : 0;
  $("#mem-value").textContent = `${fmtBytes(s.mem_used)} / ${fmtBytes(s.mem_total)}`;
  $<HTMLElement>("#mem-bar").style.width = `${memPct}%`;

  $("#proc-count").textContent = String(s.process_count);

  pushHist(cpuHist, s.cpu);
  pushHist(memHist, memPct);
  drawSpark("cpu-spark", cpuHist, 100, "#4493f8");
  drawSpark("mem-spark", memHist, 100, "#3fb950");
}

function updateSortHeaders() {
  document.querySelectorAll("th.sortable").forEach((th) => {
    const key = (th as HTMLElement).dataset.key;
    th.classList.toggle("active", key === sortKey);
    th.classList.toggle("asc", key === sortKey && sortDir === "asc");
    th.classList.toggle("desc", key === sortKey && sortDir === "desc");
  });
}

// --- polling ----------------------------------------------------------------

async function poll() {
  if (paused) return;
  try {
    const snap = await invoke<Snapshot>("get_snapshot");
    latest = snap.processes;
    renderSummary(snap.summary);
    render();
    $("#status").textContent = `Updated ${new Date().toLocaleTimeString()} · ${snap.summary.cpu_count} cores`;
  } catch (e) {
    $("#status").textContent = `Error: ${e}`;
  }
}

function restartPolling() {
  if (pollTimer !== undefined) clearInterval(pollTimer);
  pollTimer = window.setInterval(poll, pollMs);
}

async function killProcess(pid: number) {
  try {
    await invoke("kill_process", { pid });
    latest = latest.filter((p) => p.pid !== pid);
    render();
  } catch (e) {
    $("#status").textContent = `Kill failed: ${e}`;
  }
}

// --- history / timeline -----------------------------------------------------

interface MetricPoint {
  ts: number;
  cpu: number;
  mem_used: number;
  mem_total: number;
  disk_read: number;
  disk_write: number;
}

interface HistoryWindow {
  min_ts: number | null;
  max_ts: number | null;
  retention_secs: number;
  sample_secs: number;
}

interface TopProc {
  name: string;
  cpu: number;
  memory: number;
}

let currentView: "live" | "history" = "live";
let rangeSecs = 1800;
let histData: MetricPoint[] = [];
let histTimer: number | undefined;
let cursorIdx: number | null = null;
let hovering = false;
let lastTopTs = -1;

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

async function loadHistory() {
  try {
    const win = await invoke<HistoryWindow>("get_history_window");
    if (win.max_ts == null || win.min_ts == null) {
      byId("hist-status").textContent = "No history yet — collecting…";
      return;
    }
    const to = win.max_ts;
    const from = Math.max(win.min_ts, to - rangeSecs);
    const target = 500;
    const bucket = Math.max(win.sample_secs, Math.ceil((to - from) / target));
    histData = await invoke<MetricPoint[]>("get_history", { from, to, bucketSecs: bucket });
    drawCharts(cursorIdx);
    const mins = Math.round((to - from) / 60);
    byId("hist-status").textContent =
      `${histData.length} points · ~${mins}m · ${bucket}s/point`;
  } catch (e) {
    byId("hist-status").textContent = `Error: ${e}`;
  }
}

function drawSeriesChart(
  id: string,
  pts: MetricPoint[],
  getVal: (p: MetricPoint) => number,
  maxVal: number,
  color: string,
  cursor: number | null,
) {
  const c = document.getElementById(id) as HTMLCanvasElement | null;
  if (!c) return;
  const dpr = window.devicePixelRatio || 1;
  const w = c.clientWidth;
  const h = c.clientHeight;
  if (w === 0 || h === 0) return;
  c.width = Math.round(w * dpr);
  c.height = Math.round(h * dpr);
  const ctx = c.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  if (pts.length < 2) return;

  const n = pts.length;
  const max = maxVal || 1;
  const x = (i: number) => (i / (n - 1)) * (w - 2) + 1;
  const y = (v: number) => h - 1 - (Math.min(v, max) / max) * (h - 2);

  ctx.strokeStyle = "rgba(255,255,255,0.05)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, h * 0.5);
  ctx.lineTo(w, h * 0.5);
  ctx.stroke();

  ctx.beginPath();
  pts.forEach((p, i) => {
    const px = x(i);
    const py = y(getVal(p));
    i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
  });
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = color;
  ctx.stroke();
  ctx.lineTo(x(n - 1), h);
  ctx.lineTo(x(0), h);
  ctx.closePath();
  ctx.fillStyle = color + "1f";
  ctx.fill();

  if (cursor != null && cursor >= 0 && cursor < n) {
    const px = x(cursor);
    const py = y(getVal(pts[cursor]));
    ctx.strokeStyle = "rgba(255,255,255,0.28)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(px, 0);
    ctx.lineTo(px, h);
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(px, py, 2.5, 0, Math.PI * 2);
    ctx.fill();
  }
}

function drawCharts(cursor: number | null) {
  if (histData.length === 0) return;
  const memMax = Math.max(...histData.map((p) => p.mem_total), 1);
  const diskMax = Math.max(...histData.map((p) => p.disk_read + p.disk_write), 1);
  drawSeriesChart("chart-cpu", histData, (p) => p.cpu, 100, "#4493f8", cursor);
  drawSeriesChart("chart-mem", histData, (p) => p.mem_used, memMax, "#3fb950", cursor);
  drawSeriesChart("chart-disk", histData, (p) => p.disk_read + p.disk_write, diskMax, "#d9a441", cursor);
}

async function showTopAt(idx: number) {
  const p = histData[idx];
  if (!p) return;
  byId("top-title").textContent = new Date(p.ts * 1000).toLocaleString();
  byId("top-sub").textContent =
    `CPU ${p.cpu.toFixed(1)}% · RAM ${fmtBytes(p.mem_used)} · Disk ${fmtRate(p.disk_read + p.disk_write)}`;
  if (p.ts === lastTopTs) return;
  lastTopTs = p.ts;
  try {
    const top = await invoke<TopProc[]>("get_top_at", { ts: p.ts });
    byId("top-list").innerHTML = top
      .map(
        (t) =>
          `<li><span class="tp-name">${t.name}</span>` +
          `<span class="tp-val">${t.cpu.toFixed(1)}% · ${fmtBytes(t.memory)}</span></li>`,
      )
      .join("");
  } catch {
    /* ignore */
  }
}

function onChartHover(clientX: number) {
  const ref = document.getElementById("chart-cpu") as HTMLCanvasElement | null;
  if (!ref || histData.length < 2) return;
  const rect = ref.getBoundingClientRect();
  const frac = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  const idx = Math.round(frac * (histData.length - 1));
  if (idx === cursorIdx) return;
  cursorIdx = idx;
  drawCharts(idx);
  showTopAt(idx);
}

function startHistTimer() {
  stopHistTimer();
  histTimer = window.setInterval(() => {
    if (!hovering) loadHistory();
  }, 5000);
}

function stopHistTimer() {
  if (histTimer !== undefined) clearInterval(histTimer);
  histTimer = undefined;
}

function switchView(view: "live" | "history") {
  currentView = view;
  byId("live-view").hidden = view !== "live";
  byId("history-view").hidden = view !== "history";
  document.querySelectorAll(".tab").forEach((t) => {
    t.classList.toggle("active", (t as HTMLElement).dataset.view === view);
  });
  if (view === "history") {
    loadHistory();
    startHistTimer();
  } else {
    stopHistTimer();
  }
}

// --- wiring -----------------------------------------------------------------

window.addEventListener("DOMContentLoaded", () => {
  document.querySelectorAll("th.sortable").forEach((th) => {
    th.addEventListener("click", () => {
      const key = (th as HTMLElement).dataset.key as SortKey;
      if (key === sortKey) {
        sortDir = sortDir === "asc" ? "desc" : "asc";
      } else {
        sortKey = key;
        sortDir = key === "name" || key === "user" ? "asc" : "desc";
      }
      updateSortHeaders();
      render();
    });
  });

  $<HTMLInputElement>("#filter").addEventListener("input", (e) => {
    filter = (e.target as HTMLInputElement).value;
    render();
  });

  $<HTMLSelectElement>("#rate").addEventListener("change", (e) => {
    pollMs = Number((e.target as HTMLSelectElement).value);
    restartPolling();
  });

  $("#group-btn").addEventListener("click", () => {
    groupBy = !groupBy;
    $("#group-btn").classList.toggle("active", groupBy);
    render();
  });

  $("#pause-btn").addEventListener("click", () => {
    paused = !paused;
    $("#pause-btn").textContent = paused ? "▶ Resume" : "⏸ Pause";
    if (!paused) poll();
  });

  // Delegated clicks: kill buttons and group expand/collapse.
  $("#rows").addEventListener("click", (e) => {
    const target = e.target as HTMLElement;
    const btn = target.closest("button.kill") as HTMLElement | null;
    if (btn) {
      killProcess(Number(btn.dataset.pid));
      return;
    }
    const groupTr = target.closest("tr.group") as HTMLElement | null;
    if (groupTr) {
      const name = groupTr.dataset.name!;
      expanded.has(name) ? expanded.delete(name) : expanded.add(name);
      render();
    }
  });

  // Tab switching.
  document.querySelectorAll(".tab").forEach((t) => {
    t.addEventListener("click", () => switchView((t as HTMLElement).dataset.view as "live" | "history"));
  });

  // History range buttons.
  byId("ranges").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button") as HTMLElement | null;
    if (!btn || !btn.dataset.secs) return;
    rangeSecs = Number(btn.dataset.secs);
    byId("ranges").querySelectorAll("button").forEach((b) => b.classList.toggle("active", b === btn));
    cursorIdx = null;
    loadHistory();
  });

  // Chart scrubbing.
  const charts = byId("charts");
  charts.addEventListener("mousemove", (e) => {
    hovering = true;
    onChartHover((e as MouseEvent).clientX);
  });
  charts.addEventListener("mouseleave", () => {
    hovering = false;
    cursorIdx = null;
    drawCharts(null);
  });

  window.addEventListener("resize", () => {
    if (currentView === "history") drawCharts(cursorIdx);
  });

  updateSortHeaders();
  poll();
  restartPolling();
});
