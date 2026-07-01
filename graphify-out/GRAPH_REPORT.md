# Graph Report - .  (2026-07-01)

## Corpus Check
- Corpus is ~37,627 words - fits in a single context window. You may not need a graph.

## Summary
- 415 nodes · 639 edges · 23 communities (20 shown, 3 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 18 edges (avg confidence: 0.88)
- Token cost: 30,000 input · 4,238 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Rust Backend Core (lib.rs)|Rust Backend Core (lib.rs)]]
- [[_COMMUNITY_Capabilities ACL Schema|Capabilities ACL Schema]]
- [[_COMMUNITY_Capabilities Manifest Schema|Capabilities Manifest Schema]]
- [[_COMMUNITY_Frontend State & Data Model|Frontend State & Data Model]]
- [[_COMMUNITY_UI Views & Product Concepts|UI Views & Product Concepts]]
- [[_COMMUNITY_Desktop Permission Schema|Desktop Permission Schema]]
- [[_COMMUNITY_Windows Permission Schema|Windows Permission Schema]]
- [[_COMMUNITY_NPM Package & Dependencies|NPM Package & Dependencies]]
- [[_COMMUNITY_Tauri App Configuration|Tauri App Configuration]]
- [[_COMMUNITY_Sensor Helper (C)|Sensor Helper (C#)]]
- [[_COMMUNITY_TypeScript Compiler Config|TypeScript Compiler Config]]
- [[_COMMUNITY_Live View Rendering|Live View Rendering]]
- [[_COMMUNITY_Blocklist & Privacy Actions|Blocklist & Privacy Actions]]
- [[_COMMUNITY_Formatting & Detail Helpers|Formatting & Detail Helpers]]
- [[_COMMUNITY_View Timers & Switching|View Timers & Switching]]
- [[_COMMUNITY_Default Capability Permissions|Default Capability Permissions]]
- [[_COMMUNITY_History Charts|History Charts]]
- [[_COMMUNITY_Settings Modal|Settings Modal]]
- [[_COMMUNITY_CPU Normalization Concept|CPU Normalization Concept]]
- [[_COMMUNITY_HidSharp License|HidSharp License]]

## God Nodes (most connected - your core abstractions)
1. `Shared` - 23 edges
2. `compilerOptions` - 15 edges
3. `byId()` - 13 edges
4. `sampler_loop()` - 12 edges
5. `switchView()` - 11 edges
6. `Strix` - 10 edges
7. `definitions` - 9 edges
8. `definitions` - 9 edges
9. `Snapshot` - 9 edges
10. `Strix Roadmap` - 9 edges

## Surprising Connections (you probably didn't know these)
- `Phase 6 — AI / MCP Extension` --conceptually_related_to--> `Strix`  [INFERRED]
  ROADMAP.md → README.md
- `Phase 2 — Historical Recording + Timeline` --references--> `SQLite History Storage`  [INFERRED]
  ROADMAP.md → README.md
- `Live View Section` --implements--> `Live Process View`  [INFERRED]
  index.html → README.md
- `Phase 1 — Solid Live View` --conceptually_related_to--> `Live Process View`  [INFERRED]
  ROADMAP.md → README.md
- `History View Section` --implements--> `History / Timeline Recording`  [INFERRED]
  index.html → README.md

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Strix Four-Tab UI (Live/History/Privacy/Temps)** — index_live_view, index_history_view, index_privacy_view, index_temps_view [EXTRACTED 0.90]
- **Temperature Monitoring Stack** — readme_temperature_monitoring, readme_strix_sensors_exe, readme_librehardwaremonitor, third_party_licenses_pawnio [INFERRED 0.85]
- **Roadmap Phase Progression** — roadmap_phase_1, roadmap_phase_2, roadmap_phase_4, roadmap_phase_3, roadmap_phase_5 [EXTRACTED 0.85]

## Communities (23 total, 3 thin omitted)

### Community 0 - "Rust Backend Core (lib.rs)"
Cohesion: 0.08
Nodes (70): AppHandle, Arc, Connection, Default, HashMap, HashSet, HKEY, Mutex (+62 more)

### Community 1 - "Capabilities ACL Schema"
Cohesion: 0.05
Nodes (41): description, properties, required, type, Capability, Identifier, default, description (+33 more)

### Community 2 - "Capabilities Manifest Schema"
Cohesion: 0.05
Nodes (41): description, properties, required, type, Capability, Identifier, default, description (+33 more)

### Community 3 - "Frontend State & Data Model"
Cohesion: 0.06
Nodes (25): AppDetails, Block, blockCache, cpuHist, DESCRIPTIONS, DisplayRow, expanded, GroupAgg (+17 more)

### Community 4 - "UI Views & Product Concepts"
Cohesion: 0.09
Nodes (33): App Details Modal, Blocklist Modal, History View Section, Live View Section, main.ts entry module, Privacy View Section, Temps View Section, Topbar Header & Stats (+25 more)

### Community 5 - "Desktop Permission Schema"
Cohesion: 0.07
Nodes (28): anyOf, anyOf, description, description, properties, required, type, definitions (+20 more)

### Community 6 - "Windows Permission Schema"
Cohesion: 0.07
Nodes (28): anyOf, anyOf, description, description, properties, required, type, definitions (+20 more)

### Community 7 - "NPM Package & Dependencies"
Cohesion: 0.10
Nodes (20): dependencies, @tauri-apps/api, @tauri-apps/plugin-autostart, @tauri-apps/plugin-opener, devDependencies, @tauri-apps/cli, typescript, vite (+12 more)

### Community 8 - "Tauri App Configuration"
Cohesion: 0.10
Nodes (19): app, security, windows, withGlobalTauri, build, beforeBuildCommand, beforeDevCommand, devUrl (+11 more)

### Community 9 - "Sensor Helper (C#)"
Cohesion: 0.14
Nodes (9): StrixSensors, IComputer, IHardware, IParameter, ISensor, IVisitor, List, Program (+1 more)

### Community 10 - "TypeScript Compiler Config"
Cohesion: 0.12
Nodes (16): compilerOptions, allowImportingTsExtensions, isolatedModules, lib, module, moduleResolution, noEmit, noFallthroughCasesInSwitch (+8 more)

### Community 11 - "Live View Rendering"
Cohesion: 0.23
Nodes (12): $(), buildRows(), createGroupRow(), createProcRow(), drawSpark(), killProcess(), mkCell(), poll() (+4 more)

### Community 12 - "Blocklist & Privacy Actions"
Cohesion: 0.24
Nodes (12): byId(), closeDetails(), isBlocked(), loadBlocks(), loadPrivacy(), loadTemps(), openBlocklist(), refreshBlockButtons() (+4 more)

### Community 13 - "Formatting & Detail Helpers"
Cohesion: 0.36
Nodes (10): cpuClass(), describe(), fmtBytes(), fmtDuration(), fmtRate(), openDetails(), setText(), showTopAt() (+2 more)

### Community 14 - "View Timers & Switching"
Cohesion: 0.43
Nodes (7): startHistTimer(), startPrivacyTimer(), startTempsTimer(), stopHistTimer(), stopPrivacyTimer(), stopTempsTimer(), switchView()

### Community 15 - "Default Capability Permissions"
Cohesion: 0.33
Nodes (5): description, identifier, permissions, $schema, windows

### Community 16 - "History Charts"
Cohesion: 0.50
Nodes (4): drawCharts(), drawSeriesChart(), loadHistory(), onChartHover()

## Knowledge Gaps
- **170 isolated node(s):** `name`, `private`, `version`, `type`, `packageManager` (+165 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `definitions` connect `Desktop Permission Schema` to `Capabilities ACL Schema`?**
  _High betweenness centrality (0.017) - this node is a cross-community bridge._
- **What connects `name`, `private`, `version` to the rest of the system?**
  _171 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Rust Backend Core (lib.rs)` be split into smaller, more focused modules?**
  _Cohesion score 0.0806697108066971 - nodes in this community are weakly interconnected._
- **Should `Capabilities ACL Schema` be split into smaller, more focused modules?**
  _Cohesion score 0.05121951219512195 - nodes in this community are weakly interconnected._
- **Should `Capabilities Manifest Schema` be split into smaller, more focused modules?**
  _Cohesion score 0.05121951219512195 - nodes in this community are weakly interconnected._
- **Should `Frontend State & Data Model` be split into smaller, more focused modules?**
  _Cohesion score 0.05714285714285714 - nodes in this community are weakly interconnected._
- **Should `UI Views & Product Concepts` be split into smaller, more focused modules?**
  _Cohesion score 0.09090909090909091 - nodes in this community are weakly interconnected._