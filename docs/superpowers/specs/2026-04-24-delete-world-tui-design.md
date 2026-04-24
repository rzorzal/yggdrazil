# Delete World from TUI — Design Spec

**Date:** 2026-04-24  
**Status:** Approved

## Summary

Add the ability to delete a world (git worktree) and kill its active agent directly from the monitoring TUI. Also fixes the missing agent/PID display by wiring the TUI to receive live state from the daemon via IPC.

## Goals

- `[d]` on a selected world in the dashboard triggers a confirmation overlay, then deletes the world and kills its agent.
- Agents (with PIDs) appear correctly in the TUI agents table.
- TUI gracefully degrades when daemon is not running (no crash, `[d]` shows error).

## Non-Goals

- Separate "kill agent only" action.
- Delete from the WorldDetail view.
- Any changes to the `ygg run` or `ygg sync` commands.

---

## Architecture

### 1. New IPC Message Types (`src/types.rs`)

Add two variants to `IpcMessage`:

```rust
DeleteWorld { world_id: String },   // TUI → daemon: request deletion
WorldDeleted { world_id: String },  // daemon → TUI: deletion confirmed
```

Add one variant to `EventKind`:

```rust
WorldDeleted,
```

### 2. Daemon: Handle `DeleteWorld` (`src/daemon/mod.rs`)

New arm in `accept_loop` handler:

1. Call `roots::scan_once(repo_root, worlds_dir)` to find the agent with matching `world_id`.
2. If agent found: send `SIGTERM` to its PID (via `nix::sys::signal::kill` on Unix).
3. Call `trunk::delete_world(repo_root, world_id)`.
4. Append `AuditEvent { event: EventKind::WorldDeleted, world: world_id }` to `shared_memory.json`.
5. Broadcast `IpcMessage::WorldDeleted { world_id }` via `tx`.

If `delete_world` fails, do not broadcast — let the error propagate to the log.

### 3. Daemon: Broadcast StateSnapshot (`src/daemon/roots.rs`)

`scan_loop` signature changes to accept `tx: broadcast::Sender<IpcMessage>`.

After each scan cycle, broadcast:

```rust
IpcMessage::StateSnapshot { worlds, agents, conflicts }
```

`worlds` and `conflicts` are read from disk at that point; `agents` comes from `scan_once`. This gives subscribers (TUI) live agent data every 30 seconds.

`Daemon::run` passes `server.tx.clone()` to `scan_loop`.

### 4. TUI: IPC Background Thread (`src/tui/mod.rs`)

`run_tui()` attempts `IpcClient::connect(socket_path)` at startup.

**If connection succeeds:**

- Spawns a background thread with its own `tokio::runtime::Runtime`.
- That thread:
  1. Sends `IpcMessage::Subscribe`.
  2. Loop: reads messages from daemon → pushes to `evt_tx: mpsc::Sender<IpcMessage>`.
  3. Also drains `cmd_rx: mpsc::Receiver<IpcMessage>` → sends to daemon.
- `AppState` stores `ipc_tx: Option<mpsc::Sender<IpcMessage>>` and `ipc_rx` is held in the loop.

**If connection fails:** `ipc_tx` is `None`. TUI works as before (read-only, no agents).

**Message handling in TUI loop:**

Each iteration drains `ipc_rx` (non-blocking `try_recv` loop):

| Message | Action |
|---|---|
| `StateSnapshot { worlds, agents, conflicts }` | Replace `state.worlds`, `state.agents`, `state.conflicts` |
| `EventNotification { event }` | Prepend to `state.audit_log` |
| `WorldDeleted { world_id }` | Remove from `state.worlds`, clear `confirm_delete` |

### 5. TUI: Confirmation Overlay (`src/tui/mod.rs`, `src/tui/dashboard.rs`)

`AppState` gains:

```rust
pub confirm_delete: Option<String>,       // world_id pending confirmation
pub ipc_tx: Option<mpsc::Sender<IpcMessage>>,
pub status_msg: Option<String>,           // transient status bar message
```

**Key bindings (Dashboard view):**

| Key | Condition | Action |
|---|---|---|
| `d` | `confirm_delete.is_none()` | `confirm_delete = Some(selected_world_id)` |
| `y` | `confirm_delete.is_some()` | Send `DeleteWorld` via `ipc_tx`; if `ipc_tx` is None set `status_msg = "daemon not running"` |
| `n` / `Esc` | `confirm_delete.is_some()` | `confirm_delete = None` |

**Overlay rendering** (in `dashboard.rs`):

When `state.confirm_delete.is_some()`, render a centered popup over the dashboard:

```
╔══════════════════════════════════════╗
║  Delete "feat-auth" + kill agent?    ║
║  [y] confirm       [n] cancel        ║
╚══════════════════════════════════════╝
```

Uses `ratatui::widgets::Clear` to blank the background area before rendering.

Status bar updates to show `status_msg` when set (cleared on next keypress).

---

## Error Handling

- Daemon not running at TUI start → `ipc_tx = None` → `[d]` sets `status_msg = "daemon not running"`, no crash.
- Agent not found during delete (already exited) → daemon skips kill, proceeds with worktree removal.
- `delete_world` fails → daemon logs error; `WorldDeleted` is NOT broadcast; TUI retains world in list.
- IPC thread panics → TUI continues without live updates (graceful degradation).

## Dependencies

- `nix` crate (Unix signals) must be added to `Cargo.toml` under `[target.'cfg(unix)'.dependencies]` for `SIGTERM` delivery. On Windows, use `taskkill` via `std::process::Command`.

## Files Changed

| File | Change |
|---|---|
| `src/types.rs` | Add `DeleteWorld`, `WorldDeleted` to `IpcMessage`; `WorldDeleted` to `EventKind` |
| `src/daemon/mod.rs` | Handle `DeleteWorld`; pass `tx` to `scan_loop` |
| `src/daemon/roots.rs` | `scan_loop` accepts `tx`; broadcasts `StateSnapshot` each cycle |
| `src/tui/mod.rs` | IPC background thread; `AppState` new fields; key bindings |
| `src/tui/dashboard.rs` | Confirmation overlay; status message in status bar |

## Tests

- `types.rs`: roundtrip serialize/deserialize `DeleteWorld` and `WorldDeleted`.
- `daemon/mod.rs`: integration test — send `DeleteWorld` for a world with no agent → world removed, `WorldDeleted` broadcast.
- `tui/mod.rs`: unit test `confirm_delete` state transitions (no IPC needed).
- `tui/dashboard.rs`: snapshot test for overlay rendering.
