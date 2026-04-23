# Yggdrazil (`ygg`) — Design Spec
**Date:** 2026-04-23  
**Status:** Approved

---

## 1. Overview

Yggdrazil is a Rust CLI tool that governs parallel AI agent development via Git Worktrees. It creates isolated "Worlds" from a trunk repo, monitors AI agent processes, detects file conflicts, injects governance rules into each world, and provides a smart merge flow when work is complete.

**Target platforms:** macOS, Linux, Windows (cross-compiled via GitHub Actions).

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    ygg binary                           │
│                                                         │
│  CLI (clap)                                             │
│  ├── ygg init [--world <name>] [--rules <path>]         │
│  ├── ygg monit          → connect to daemon, render TUI │
│  ├── ygg sync           → smart merge + memory consolidate
│  ├── ygg hook           → agent self-report (cross-platform)│
│  ├── ygg daemon start   → spawn background daemon       │
│  └── ygg daemon stop                                    │
│  Note: ygg monit auto-starts daemon if not running      │
│                                                         │
│  Daemon (tokio async)                                   │
│  ├── Roots: sysinfo process scanner (30-60s poll)       │
│  ├── Trunk: git worktree manager                        │
│  ├── Laws: rules injector (init + dynamic updates)      │
│  ├── Resonance Bus: audit log writer + conflict detector│
│  └── IPC server: Unix socket / Windows named pipe       │
└─────────────────────────────────────────────────────────┘
         │ IPC (JSON-lines over socket)
         ▼
┌─────────────────┐    ┌──────────────────────────────┐
│  ygg monit TUI  │    │  Agent hooks (.agent_state)  │
│  (ratatui)      │    │  push events to daemon socket│
│  Panels:        │    │  on each iteration end       │
│  - Worlds       │    └──────────────────────────────┘
│  - Agents       │
│  - Conflicts    │
│  - Audit log    │
└─────────────────┘
```

**Filesystem layout:**
```
.ygg/                          # governance root (gitignored)
├── daemon.sock                # IPC socket (named pipe on Windows)
├── shared_memory.json         # append-only audit log
└── worlds/
    └── <name>/                # git worktree per world
        ├── .agent_state       # agent self-report file
        └── .env               # injected env vars (PORT, etc.)
```

---

## 3. Components

### 3.1 Roots — Process Sensor
- Uses `sysinfo` crate to scan all PIDs every 30s (configurable).
- Matches binaries: `claude`, `claude-code`, `codex`, `aider`, `cursor`.
- For each match: extracts PID, CWD, binary name.
- If CWD is under `.ygg/worlds/<name>/` → maps agent to that world.
- Emits `AgentSpawned` / `AgentExited` events to Resonance Bus.

### 3.2 Trunk — Worktree Manager
- Wraps `git worktree add .ygg/worlds/<name> -b <name>` via `git2` crate.
- `ygg init`: creates `.ygg/` structure, adds first worktree (or named world with `--world`).
- Injects env vars into `.ygg/worlds/<name>/.env`: base port + world index (world 0 → PORT=3000, world 1 → PORT=3001, etc.).
- Custom rules path via `--rules <path>` copies/symlinks file into world.

### 3.3 Laws — Rules Injector
- On `ygg init`: writes `CLAUDE.md`, `.cursorrules`, `.aider.conf.yml` into each world with YGGDRAZIL PROTOCOL preamble:
  ```
  YGGDRAZIL PROTOCOL ACTIVE
  1. You are in World: {{WORLD_ID}}
  2. Agent {{OTHER_AGENT}} is working on {{FILE}}
  3. Read .ygg/shared_memory.json before acting to save tokens
  4. On each iteration end, write your state to .agent_state
  ```
- **Dynamic updates:** Resonance Bus conflict events trigger re-injection — appends conflict warning block to the conflicting world's instruction files automatically.

### 3.4 Resonance Bus — Audit Log + Conflict Detector

**Audit log format** (`shared_memory.json`, append-only JSON-lines):
```json
{"ts":"2026-04-23T10:00:00Z","event":"agent_spawned","world":"feature-auth","agent":"claude-code","pid":1234}
{"ts":"2026-04-23T10:23:39Z","event":"file_modified","world":"feature-auth","agent":"claude-code","file":"src/auth.rs"}
{"ts":"2026-04-23T10:23:41Z","event":"file_modified","world":"feature-api","agent":"aider","file":"src/auth.rs"}
{"ts":"2026-04-23T10:23:41Z","event":"conflict_detected","worlds":["feature-auth","feature-api"],"file":"src/auth.rs"}
{"ts":"2026-04-23T10:45:00Z","event":"world_merged","world":"feature-auth"}
```

**Event types:** `agent_spawned`, `agent_exited`, `file_modified`, `iteration_end`, `conflict_detected`, `warning_injected`, `world_merged`.

**Conflict detection:**  
On each `file_modified` event, scan the last 500 events (or 2-hour window, whichever is smaller) for the same file path in a different world → emit `conflict_detected` → trigger Laws dynamic update + OS push notification via `notify-rust`.

**File change detection (hybrid):**
- Primary: agent hook pushes `{"event":"iteration_end","world":"x","files":["src/foo.rs"]}` to daemon socket at end of each iteration.
- Fallback: `git status` poll in each worktree every 30-60s to catch agents that don't self-report.

### 3.5 IPC
- JSON-lines protocol over `interprocess` crate (Unix socket on macOS/Linux, named pipe on Windows).
- Daemon pushes full state snapshots to all connected TUI clients on any event (pure push, no polling from TUI).
- Agent hooks are thin: one `ygg hook --world <name> --files <file,...>` call injected into `CLAUDE.md` hook instructions. The `ygg hook` subcommand handles cross-platform IPC (Unix socket on macOS/Linux, named pipe on Windows) — no `nc` dependency.

---

## 4. TUI Dashboard (`ygg monit`)

Built with `ratatui` + `crossterm`.

```
┌─ YGGDRAZIL ──────────────────────────────────────────────────────┐
│ Worlds          │ Active Agents                                   │
│ ─────────────── │ ──────────────────────────────────────────────  │
│ ● feature-auth  │ PID 1234  claude-code  feature-auth  auth.rs   │
│ ● feature-api   │ PID 5678  aider        feature-api   routes.rs │
│ ○ main          │                                                 │
├─────────────────┴──────────────────────────────────────────────  ┤
│ ⚠ CONFLICTS                                                       │
│ src/auth.rs — feature-auth (claude) + feature-api (aider)        │
│ Warning injected into feature-api CLAUDE.md at 10:23:41          │
├──────────────────────────────────────────────────────────────────┤
│ Audit Log                                              [↑↓ scroll]│
│ 10:23:41  file_modified   feature-api    src/auth.rs             │
│ 10:23:39  file_modified   feature-auth   src/auth.rs             │
│ 10:23:10  agent_spawned   feature-api    aider  PID 5678         │
└──────────────────────────────────────────────────────────────────┘
 [q]uit  [s]ync  [n]ew world  [d]elete world  [r]efresh
```

**Keyboard nav:**
- `↑↓` select world, `Enter` drill into world detail view
- `s` trigger `ygg sync` flow inline
- `n` prompt for new world name + optional rules path
- `d` delete selected world (with confirmation)
- `q` exit TUI (daemon keeps running)

**World detail view** (after `Enter`): full file list, agent state, last 50 audit events for that world, injected env vars.

---

## 5. `ygg sync` — Smart Merge Flow

```
1. For each world: git diff trunk...world --stat
   → build overlap map: file → [world_a, world_b, ...]

2. Show merge report:
   feature-auth  →  src/auth.rs (lines 10-40)  ✓ safe
   feature-api   →  src/auth.rs (lines 35-60)  ⚠ overlap
   feature-api   →  src/routes.rs              ✓ safe

3. Per-world confirmation:
   Merge feature-auth → trunk? [y/n]
   Merge feature-api  → trunk? [y/n/defer]

4. For confirmed worlds:
   git merge --no-ff <world-branch-name>   # branch, not worktree path
   on conflict → surface diff in TUI or open $EDITOR

5. Consolidate audit log:
   Append world_merged event to shared_memory.json

6. Optional --prune flag:
   Remove merged worktree + branch after success
```

**Overlap detection:** line-range diff via `git diff -U0`, parse hunk headers `@@ -start,count @@`, compare ranges across worlds. Flag any overlap ≥ 1 line.

---

## 6. Crate Structure

```
yggdrazil/
├── src/
│   ├── main.rs
│   ├── cli/
│   │   ├── init.rs
│   │   ├── sync.rs
│   │   └── daemon.rs
│   ├── daemon/
│   │   ├── mod.rs        # tokio runtime, supervisor
│   │   ├── roots.rs      # sysinfo process scanner
│   │   ├── trunk.rs      # git worktree manager
│   │   ├── laws.rs       # rules injector
│   │   └── bus.rs        # audit log + conflict detector
│   ├── ipc/
│   │   ├── mod.rs        # socket path resolution
│   │   ├── server.rs     # daemon IPC server
│   │   └── client.rs     # CLI/TUI IPC client
│   ├── tui/
│   │   ├── mod.rs        # ratatui app loop
│   │   ├── dashboard.rs  # main 4-panel layout
│   │   └── world_detail.rs
│   └── types.rs          # World, Agent, Event, Conflict structs
├── Cargo.toml
├── .github/workflows/
│   └── release.yml       # cross-compile matrix + GitHub release
└── scripts/
    └── install.sh
```

---

## 7. Key Dependencies

```toml
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sysinfo = "0.30"
ratatui = "0.26"
crossterm = "0.27"
git2 = "0.19"
notify-rust = "4"
interprocess = "2"
anyhow = "1"
tracing = "1"
tracing-subscriber = "1"
```

---

## 8. Release & Install

**GitHub Actions** (`release.yml`): matrix build on `ubuntu-latest`, `macos-latest`, `windows-latest` for targets `x86_64` + `aarch64`. Triggered on `v*` tag push. Uploads compiled binaries as release assets.

**install.sh:**
1. Detect arch (`x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, etc.)
2. Query GitHub API for latest release asset URL
3. Download, extract, move to `/usr/local/bin/ygg`

```bash
curl -sSL https://raw.githubusercontent.com/rzorzal/yggdrazil/main/scripts/install.sh | sh
```

---

## 9. Error Handling & Testing

- All errors via `anyhow` with context chains. No panics in daemon.
- Daemon restarts automatically if subprocess crashes (supervisor in `daemon/mod.rs`).
- Unit tests: each module tested in isolation (mock `sysinfo`, mock git ops via temp repos).
- Integration tests: spin up temp git repo, run `ygg init`, assert `.ygg/` structure.
- Cross-platform CI matrix validates all 3 platforms on every PR.
