# Yggdrazil (`ygg`)

AI agent governance engine for parallel development. Uses Git Worktrees to isolate each agent session into its own **World**, detects file conflicts across worlds in real time, injects governance rules into agent context files, and provides a smart merge flow when work is done.

```
┌─ YGGDRAZIL ──────────────────────────────────────────────────────────┐
│ Worlds             Branch           │ Active Agents                   │
│ ─────────────────────────────────── │ ──────────────────────────────  │
│ ● feature-auth     feat/auth        │ PID 1234  claude  auth.rs       │
│ ● feature-api      feat/api         │ PID 5678  aider   routes.rs     │
│ ⚠ unmanaged-a3f9   main             │ PID 9012  codex   main.rs       │
├─────────────────────────────────────┴──────────────────────────────  ┤
│ ⚠ CONFLICTS                                                           │
│ src/auth.rs — feature-auth (claude) + feature-api (aider)            │
├──────────────────────────────────────────────────────────────────────┤
│ Audit Log                                                  [↑↓ scroll]│
│ 10:23:41  file_modified   feature-api   feat/api    src/auth.rs      │
└──────────────────────────────────────────────────────────────────────┘
 [q]uit  [s]ync  [↑↓]select  [Enter]detail  [Esc]back
```

---

## How It Works

1. **`ygg init`** — one-time setup. Creates `.ygg/` in the repo, starts the background daemon.
2. **`ygg run <agent>`** — prompts for a branch, creates a Git worktree at `.ygg/worlds/<id>/`, injects governance rules into `CLAUDE.md` / `.cursorrules` / `.aider.conf.yml`, then spawns the agent inside that world.
3. **Daemon** — polls processes every 30s. Detects agents launched outside `ygg run` (IDE-launched, scripts) and auto-creates an `unmanaged-<hash>` world for them.
4. **Conflict detection** — when any agent calls `ygg hook` (or the daemon polls), file modifications are written to `.ygg/shared_memory.json`. If the same file is modified in two worlds within a 2-hour / 500-event window, a conflict is detected, a warning is injected into the other world's `CLAUDE.md`, and an OS push notification fires.
5. **`ygg monit`** — live ratatui TUI showing all worlds, active agents, conflicts, and audit log.
6. **`ygg sync`** — computes `git diff -U0` line ranges across all worlds, shows an overlap report, then prompts per-world merge confirmation.

---

## Prerequisites

- Rust toolchain (`rustup` — stable)
- Git ≥ 2.5 (worktrees support)
- macOS or Linux (Windows: builds but IPC uses named pipes; `ygg monit` TUI requires a capable terminal)

---

## Install

### From source

```bash
git clone https://github.com/rzorzal/yggdrazil
cd yggdrazil
cargo build --release
# Move binary to PATH
sudo mv target/release/ygg /usr/local/bin/ygg
```

### Curl-pipe (after first GitHub release)

```bash
curl -sSL https://raw.githubusercontent.com/rzorzal/yggdrazil/main/scripts/install.sh | sh
```

Detects OS and arch, downloads the pre-built binary from the latest GitHub Release, installs to `/usr/local/bin/ygg`.

---

## Quick Start

```bash
# 1. One-time setup inside any git repo
cd /path/to/your/project
ygg init

# 2. Launch an agent in a managed world
ygg run claude
# → prompts: "Which branch for this session? [enter for HEAD]"
# → creates .ygg/worlds/claude-feat-auth-143022123/
# → injects YGGDRAZIL PROTOCOL into CLAUDE.md
# → spawns `claude` inside that dir

# Pass agent flags through verbatim
ygg run claude --resume 34343
ygg run aider --model gpt-4o --yes

# 3. Monitor all worlds
ygg monit

# 4. Merge when done
ygg sync
ygg sync --prune   # also deletes merged worlds
```

---

## Commands

### `ygg init [--rules <path>]`

One-time repo setup. Creates:

```
.ygg/
├── worlds/              # git worktrees land here
└── shared_memory.json   # append-only audit log (JSON-lines)
```

Adds `.ygg/` to `.gitignore`. Starts the background daemon.

`--rules <path>` — path to a markdown file whose content is appended to the governance rules injected into every world's `CLAUDE.md`.

---

### `ygg run <agent-binary> [agent-args...]`

Launches a managed agent session.

1. Shows a branch selection prompt (lists local branches + option to create new).
2. Warns if another world is already on that branch.
3. Creates a Git worktree on that branch at `.ygg/worlds/<id>/`.
4. Injects `CLAUDE.md`, `.cursorrules`, `.aider.conf.yml` with the governance preamble.
5. Writes `.ygg/worlds/<id>/.env` with `PORT=300N` (unique per world index).
6. Spawns the agent with all args forwarded, CWD set to the world directory.

Examples:

```bash
ygg run claude
ygg run claude --resume 34343
ygg run aider --model gpt-4o
ygg run codex
```

Supported agent binaries detected: `claude`, `claude-code`, `codex`, `aider`, `cursor`.

---

### `ygg hook --world <id> --files <file1,file2,...>`

Agent self-report. Call this at the end of each agent iteration.

Add to your agent's `CLAUDE.md` instructions:

```
After each iteration, run:
ygg hook --world <WORLD_ID> --files src/changed.rs,src/other.rs
```

What it does:
- Writes `.agent_state` JSON in the world dir (world id, files, timestamp).
- Connects to the daemon socket (best-effort) and sends a `HookReport`.
- Daemon appends `file_modified` events to `shared_memory.json` and runs conflict detection.

---

### `ygg monit`

Opens the ratatui TUI dashboard. Auto-starts daemon if not running.

**Keyboard:**

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate worlds list |
| `Enter` | Drill into world detail |
| `Esc` | Back to dashboard |
| `j` / `k` | Scroll audit log |
| `q` | Quit TUI (daemon keeps running) |

World detail shows: path, active agent + PID, `.env` contents, last 50 audit events for that world.

---

### `ygg sync [--prune]`

Smart merge flow:

1. Runs `git diff -U0 HEAD...<branch>` for each world.
2. Parses hunk headers to build a line-range map per file.
3. Detects overlapping ranges across worlds.
4. Prints a SYNC REPORT:

```
SYNC REPORT
────────────────────────────────────────────────────────────
  feature-auth  →  3 files changed  [✓ safe]
  feature-api   →  2 files changed  [⚠ overlap]

Overlap details:
  ⚠ src/auth.rs — feature-auth (lines 10-40) overlaps feature-api (lines 35-60)
```

5. Prompts per-world: `yes / no / defer`.
6. Runs `git merge --no-ff <branch>` for confirmed worlds.
7. Appends `world_merged` event to the audit log.
8. `--prune` — removes the worktree + branch after a successful merge.

**Note:** `ygg sync` merges into whatever branch is currently checked out in the main repo. Ensure you're on your integration branch before running.

---

### `ygg daemon start / stop`

```bash
ygg daemon start   # starts daemon in background, writes PID to .ygg/daemon.pid
ygg daemon stop    # sends SIGTERM, removes PID file
```

`ygg init`, `ygg run`, and `ygg monit` auto-start the daemon if it isn't running.

---

## Filesystem Layout

```
.ygg/                          # governance root — gitignored
├── daemon.sock                # Unix socket (daemon.pipe on Windows)
├── daemon.pid                 # daemon PID
├── shared_memory.json         # append-only audit log (JSON-lines)
└── worlds/
    └── <world-id>/            # git worktree
        ├── CLAUDE.md          # governance rules injected here
        ├── .cursorrules       # same rules for Cursor
        ├── .aider.conf.yml    # same rules for Aider
        ├── .agent_state       # agent self-report (written by ygg hook)
        └── .env               # PORT=300N, injected env vars
```

`.ygg/` is gitignored automatically by `ygg init`.

---

## Audit Log Format

`shared_memory.json` is append-only JSON-lines. Agents are instructed to read it before starting to avoid redundant work.

```json
{"ts":"2026-04-23T10:00:00Z","event":"agent_spawned","world":"feature-auth","agent":"claude-code","pid":1234}
{"ts":"2026-04-23T10:23:39Z","event":"file_modified","world":"feature-auth","agent":"claude-code","file":"src/auth.rs"}
{"ts":"2026-04-23T10:23:41Z","event":"conflict_detected","world":"feature-auth","file":"src/auth.rs","worlds":["feature-auth","feature-api"]}
{"ts":"2026-04-23T10:45:00Z","event":"world_merged","world":"feature-auth"}
```

Event types: `agent_spawned`, `agent_exited`, `file_modified`, `iteration_end`, `conflict_detected`, `warning_injected`, `world_created`, `world_merged`.

---

## Governance Rules Injected into Agents

Every world gets this preamble prepended to `CLAUDE.md` (and equivalents):

```markdown
<!-- YGGDRAZIL PROTOCOL ACTIVE -->
# Yggdrazil Governance Protocol

**You are operating in World: `<id>` on branch `<branch>`.**

Before starting any task:
1. Read `.ygg/shared_memory.json` to understand what other agents are doing.
2. After each iteration, run: `ygg hook --world <id> --files <comma-separated-files-you-touched>`

This saves tokens for all agents by avoiding redundant rediscovery.
```

If a conflict is detected, a `CONFLICT WARNING` block is appended to the conflicting world's `CLAUDE.md` automatically.

---

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug ygg monit

# Run a specific integration test
cargo test --test init_integration
```

### Project Structure

```
src/
  main.rs               CLI entry, clap dispatch
  types.rs              World, Agent, AuditEvent, Conflict, IpcMessage
  cli/
    init.rs             ygg init
    run.rs              ygg run — branch prompt, worktree creation, agent spawn
    hook.rs             ygg hook — .agent_state + IPC notify
    sync.rs             ygg sync — diff, overlap detection, merge flow
    daemon_cmd.rs       ygg daemon start/stop
    monit.rs            ygg monit — TUI entry
  daemon/
    mod.rs              tokio supervisor, HookReport event loop
    roots.rs            sysinfo process scanner (30s poll)
    trunk.rs            git worktree CRUD via git CLI
    laws.rs             CLAUDE.md / rules injector
    bus.rs              append-only audit log + conflict detector
  ipc/
    mod.rs              socket path resolution (unix/windows)
    server.rs           daemon-side IPC listener (broadcast channel)
    client.rs           CLI/TUI IPC connector
  tui/
    mod.rs              ratatui app loop, AppState, key navigation
    dashboard.rs        4-panel layout
    world_detail.rs     drill-down per world
tests/
  init_integration.rs   ygg init creates .ygg/ structure
  trunk_unit.rs         worktree CRUD against temp git repo
  bus_unit.rs           audit log + conflict detection logic
```

### Key Dependencies

| Crate | Use |
|-------|-----|
| `clap 4` (derive) | CLI |
| `tokio 1` (full) | async daemon runtime |
| `interprocess 2` (tokio) | Unix socket / Windows named pipe IPC |
| `sysinfo 0.30` | Process scanning |
| `ratatui 0.27` + `crossterm 0.27` | TUI |
| `notify-rust 4` | OS push notifications on conflict |
| `dialoguer 0.11` | Interactive branch selection |
| `anyhow 1` | Error handling |
| `chrono 0.4` | Timestamps |
| `tracing` + `tracing-subscriber` | Structured logging |

### Environment Variables

| Variable | Effect |
|----------|--------|
| `RUST_LOG` | Log level filter (e.g. `RUST_LOG=yggdrazil=debug`) |

---

## Release

GitHub Actions (`.github/workflows/release.yml`) cross-compiles on every `v*` tag push:

| Target | OS |
|--------|----|
| `x86_64-unknown-linux-gnu` | Linux x86 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows x86 |

To cut a release:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Binaries appear as release assets within ~10 minutes.

---

## Known Limitations

- **Audit log has no file locking** — concurrent daemon restarts can produce interleaved writes. Acceptable for single-machine use; a lockfile-based solution is tracked as a TODO in `bus.rs`.
- **Port assignment is not atomic** — two simultaneous `ygg run` invocations within the same millisecond can get the same PORT. Rare in practice.
- **`ygg monit` state is loaded at startup** — the TUI does not yet receive live push events from the daemon. Restart `ygg monit` to refresh state.
- **Windows** — builds and IPC work, but `ygg monit` TUI rendering depends on terminal capabilities. VS Code terminal is recommended.
