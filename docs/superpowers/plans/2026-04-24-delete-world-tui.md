# Delete World from TUI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `[d]` delete-world action to the TUI dashboard (kills active agent + removes git worktree via IPC to daemon) and fix the missing agent/PID display by wiring TUI to daemon's live StateSnapshot broadcasts.

**Architecture:** New `DeleteWorld`/`WorldDeleted` IPC message variants flow TUI→daemon→TUI. The daemon's `scan_loop` broadcasts `StateSnapshot` each cycle so TUI always has live agent data. TUI spawns a background thread (own tokio runtime) with two `std::sync::mpsc` channels for bidirectional communication with the daemon, while the main TUI loop stays synchronous.

**Tech Stack:** Rust, ratatui 0.27, crossterm 0.27, tokio 1, interprocess 2 (IPC), libc 0.2 (SIGTERM on Unix), serde_json (IPC wire format).

---

## File Map

| File | Change |
|---|---|
| `src/types.rs` | Add `DeleteWorld`, `WorldDeleted` to `IpcMessage`; `WorldDeleted` to `EventKind` |
| `src/daemon/roots.rs` | `scan_loop` accepts `tx: broadcast::Sender<IpcMessage>`; broadcasts `StateSnapshot` each cycle |
| `src/daemon/mod.rs` | Pass `server.tx.clone()` to `scan_loop`; handle `DeleteWorld` in accept_loop |
| `src/tui/mod.rs` | `AppState` new fields; IPC background thread; drain `evt_rx` in main loop; key bindings for `d`/`y`/`n` |
| `src/tui/dashboard.rs` | Confirmation overlay; `status_msg` in status bar; updated help bar |

---

## Task 1: New IPC Types

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` block in `src/types.rs`:

```rust
#[test]
fn ipc_delete_world_roundtrips() {
    let msg = IpcMessage::DeleteWorld { world_id: "feat-auth".into() };
    let json = serde_json::to_string(&msg).unwrap();
    let decoded: IpcMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, IpcMessage::DeleteWorld { world_id } if world_id == "feat-auth"));
}

#[test]
fn ipc_world_deleted_roundtrips() {
    let msg = IpcMessage::WorldDeleted { world_id: "feat-auth".into() };
    let json = serde_json::to_string(&msg).unwrap();
    let decoded: IpcMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, IpcMessage::WorldDeleted { world_id } if world_id == "feat-auth"));
}

#[test]
fn event_kind_world_deleted_roundtrips() {
    let event = AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::WorldDeleted,
        world: "feat-auth".into(),
        agent: None, pid: None, file: None, files: None, worlds: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let decoded: AuditEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.event, EventKind::WorldDeleted);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/ricardo/Repos/cer/yggdrazil
cargo test ipc_delete_world_roundtrips ipc_world_deleted_roundtrips event_kind_world_deleted_roundtrips 2>&1 | tail -20
```

Expected: compile error — `IpcMessage::DeleteWorld` and `WorldDeleted` don't exist yet.

- [ ] **Step 3: Add new variants to `IpcMessage` and `EventKind`**

In `src/types.rs`, add to the `IpcMessage` enum after the existing `EventNotification` variant:

```rust
DeleteWorld { world_id: String },
WorldDeleted { world_id: String },
```

Add to the `EventKind` enum after `WorldMerged`:

```rust
WorldDeleted,
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test ipc_delete_world_roundtrips ipc_world_deleted_roundtrips event_kind_world_deleted_roundtrips 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 5: Confirm full test suite still passes**

```bash
cargo test 2>&1 | tail -15
```

Expected: all existing tests pass plus 3 new ones.

- [ ] **Step 6: Commit**

```bash
git add src/types.rs
git commit -m "feat(types): add DeleteWorld/WorldDeleted IPC messages and EventKind"
```

---

## Task 2: `scan_loop` Broadcasts `StateSnapshot`

**Files:**
- Modify: `src/daemon/roots.rs`
- Modify: `src/daemon/mod.rs` (pass `tx` to `scan_loop`)

- [ ] **Step 1: Write the failing test**

Add to `src/daemon/roots.rs` test module:

```rust
#[tokio::test]
async fn scan_loop_broadcasts_state_snapshot() {
    use tokio::sync::broadcast;
    use crate::types::IpcMessage;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ygg/worlds")).unwrap();
    std::fs::write(dir.path().join(".ygg/shared_memory.json"), "").unwrap();

    let (tx, mut rx) = broadcast::channel::<IpcMessage>(16);
    let root = dir.path().to_path_buf();
    let handle = tokio::spawn(async move {
        scan_loop(&root, tx).await;
    });

    // Wait up to 2s for at least one StateSnapshot
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        async move {
            loop {
                match rx.recv().await {
                    Ok(IpcMessage::StateSnapshot { .. }) => return true,
                    Ok(_) => continue,
                    Err(_) => return false,
                }
            }
        },
    ).await;

    handle.abort();
    // snapshot arrives within the first cycle
    assert!(result.is_ok(), "timeout waiting for StateSnapshot");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test scan_loop_broadcasts_state_snapshot 2>&1 | tail -15
```

Expected: compile error — `scan_loop` signature doesn't accept `tx` yet.

- [ ] **Step 3: Update `scan_loop` in `src/daemon/roots.rs`**

Change the function signature and add a broadcast at the end of each loop iteration. Replace the entire `scan_loop` function:

```rust
pub async fn scan_loop(
    repo_root: &std::path::Path,
    tx: tokio::sync::broadcast::Sender<crate::types::IpcMessage>,
) {
    let worlds_dir = repo_root.join(".ygg").join("worlds");
    let worlds_dir_str = worlds_dir.to_string_lossy().to_string();
    let repo_str = repo_root.to_string_lossy().to_string();
    let mut known_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    loop {
        let mut sys = System::new_all();
        sys.refresh_processes();
        let current_pids: std::collections::HashSet<u32> =
            sys.processes().keys().map(|p| p.as_u32()).collect();

        for agent in scan_once(&repo_str, &worlds_dir_str) {
            let pid = agent.pid;
            if known_pids.contains(&pid) {
                continue;
            }
            known_pids.insert(pid);

            if agent.world_id.is_empty() {
                let cwd = sys.processes().values()
                    .find(|p| p.pid().as_u32() == pid)
                    .and_then(|p| p.cwd())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| repo_root.to_path_buf());
                let world_id = world_id_for_unmanaged_cwd(repo_root, &cwd);
                tracing::warn!(
                    "unmanaged agent detected: {} PID {}, creating world {}",
                    agent.binary, pid, world_id
                );
                let branch = {
                    std::process::Command::new("git")
                        .args(["rev-parse", "--abbrev-ref", "HEAD"])
                        .current_dir(repo_root)
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "HEAD".to_string())
                };
                if let Ok(world) = trunk::create_world(repo_root, &world_id, &branch) {
                    let _ = laws::inject_rules(&world.path, &world_id, &branch, &[]);
                }
            } else {
                tracing::info!(
                    "managed agent: {} PID {} in world {}",
                    agent.binary, pid, agent.world_id
                );
            }
        }

        known_pids.retain(|pid| {
            if current_pids.contains(pid) {
                true
            } else {
                tracing::info!("agent exited: PID {}", pid);
                false
            }
        });

        // Broadcast current state to all TUI subscribers
        let worlds = trunk::list_worlds(repo_root).unwrap_or_default();
        let agents = scan_once(&repo_str, &worlds_dir_str);
        let conflicts = {
            let log_path = repo_root.join(".ygg").join("shared_memory.json");
            if log_path.exists() {
                super::bus::AuditLog::open(&log_path)
                    .and_then(|l| l.read_recent(500, 2))
                    .map(|events| super::bus::detect_conflicts(&events))
                    .unwrap_or_default()
            } else {
                vec![]
            }
        };
        let _ = tx.send(crate::types::IpcMessage::StateSnapshot { worlds, agents, conflicts });

        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}
```

- [ ] **Step 4: Update `Daemon::run` in `src/daemon/mod.rs` to pass `tx`**

Find this block in `Daemon::run`:

```rust
let roots_root = repo_root.clone();
tokio::spawn(async move {
    roots::scan_loop(&roots_root).await;
});
```

Replace with:

```rust
let roots_root = repo_root.clone();
let scan_tx = server.tx.clone();
tokio::spawn(async move {
    roots::scan_loop(&roots_root, scan_tx).await;
});
```

- [ ] **Step 5: Run test to verify it passes**

```bash
cargo test scan_loop_broadcasts_state_snapshot 2>&1 | tail -10
```

Expected: 1 test passes (may take up to 2s).

- [ ] **Step 6: Full test suite**

```bash
cargo test 2>&1 | tail -15
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/daemon/roots.rs src/daemon/mod.rs
git commit -m "feat(daemon): scan_loop broadcasts StateSnapshot to IPC subscribers each cycle"
```

---

## Task 3: Daemon Handles `DeleteWorld`

**Files:**
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: Write the failing integration test**

Add to `src/daemon/mod.rs` test module:

```rust
#[tokio::test]
async fn delete_world_nonexistent_does_not_broadcast() {
    use crate::types::IpcMessage;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ygg/worlds")).unwrap();
    std::fs::write(dir.path().join(".ygg/shared_memory.json"), "").unwrap();

    let repo_root = dir.path().to_path_buf();
    let handle = tokio::spawn(Daemon::run(repo_root.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let sock = crate::ipc::socket_path(dir.path());
    let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
    client.send(&IpcMessage::DeleteWorld { world_id: "nonexistent".into() }).await.unwrap();

    // Give daemon time to process
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    // No panic = daemon handled gracefully
    handle.abort();
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test delete_world_nonexistent_does_not_broadcast 2>&1 | tail -15
```

Expected: FAIL — `DeleteWorld` hits the `_ => {}` arm and does nothing (test passes but wrong for the wrong reason). Verify by adding a `WorldDeleted` check after — actually the test should PASS as-is since daemon doesn't crash. Proceed to implementation to get full behavior.

- [ ] **Step 3: Add `DeleteWorld` handler in `src/daemon/mod.rs`**

Inside the `accept_loop` closure in `Daemon::run`, add a new match arm before `_ => {}`:

```rust
crate::types::IpcMessage::DeleteWorld { world_id } => {
    tracing::info!("delete world request: {}", world_id);

    // Find and kill active agent for this world
    let worlds_dir = repo_root.join(".ygg").join("worlds");
    let agents = crate::daemon::roots::scan_once(
        repo_root.to_str().unwrap_or(""),
        worlds_dir.to_str().unwrap_or(""),
    );
    if let Some(agent) = agents.iter().find(|a| a.world_id == world_id) {
        tracing::info!("killing agent PID {} for world {}", agent.pid, world_id);
        #[cfg(unix)]
        unsafe {
            libc::kill(agent.pid as libc::pid_t, libc::SIGTERM);
        }
    }

    // Remove the worktree
    match crate::daemon::trunk::delete_world(&repo_root, &world_id) {
        Ok(()) => {
            tracing::info!("world {} deleted", world_id);
            if let Ok(mut log) = crate::daemon::bus::AuditLog::open(&log_path) {
                let _ = log.append(&crate::types::AuditEvent {
                    ts: chrono::Utc::now(),
                    event: crate::types::EventKind::WorldDeleted,
                    world: world_id.clone(),
                    agent: None, pid: None, file: None, files: None, worlds: None,
                });
            }
            let _ = tx.send(crate::types::IpcMessage::WorldDeleted { world_id });
        }
        Err(e) => {
            tracing::error!("delete_world failed for {}: {}", world_id, e);
            // Do not broadcast — TUI keeps world in list
        }
    }
}
```

Also add the `libc` import at the top of `daemon/mod.rs` (it's already a dependency):

```rust
#[cfg(unix)]
use libc;
```

- [ ] **Step 4: Run the test**

```bash
cargo test delete_world_nonexistent_does_not_broadcast 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 5: Full test suite**

```bash
cargo test 2>&1 | tail -15
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/daemon/mod.rs
git commit -m "feat(daemon): handle DeleteWorld — kill agent + remove worktree + broadcast WorldDeleted"
```

---

## Task 4: TUI AppState Fields + IPC Background Thread

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` block in `src/tui/mod.rs`:

```rust
#[test]
fn confirm_delete_starts_none() {
    let state = AppState::default();
    assert!(state.confirm_delete.is_none());
    assert!(state.ipc_tx.is_none());
    assert!(state.status_msg.is_none());
}

#[test]
fn ipc_rx_drains_state_snapshot_into_app_state() {
    use crate::types::{Agent, Conflict, IpcMessage, World};
    use std::path::PathBuf;

    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<IpcMessage>();
    let world = World {
        id: "feat-auth".into(),
        branch: "feat/auth".into(),
        path: PathBuf::from("/tmp"),
        managed: true,
        created_at: chrono::Utc::now(),
    };
    evt_tx.send(IpcMessage::StateSnapshot {
        worlds: vec![world.clone()],
        agents: vec![],
        conflicts: vec![],
    }).unwrap();

    let mut state = AppState::default();
    // Drain events (same logic as in the main loop)
    while let Ok(msg) = evt_rx.try_recv() {
        apply_ipc_msg(&mut state, msg);
    }
    assert_eq!(state.worlds.len(), 1);
    assert_eq!(state.worlds[0].id, "feat-auth");
}

#[test]
fn ipc_rx_world_deleted_removes_world() {
    use crate::types::{IpcMessage, World};
    use std::path::PathBuf;

    let mut state = AppState {
        worlds: vec![World {
            id: "feat-auth".into(),
            branch: "feat/auth".into(),
            path: PathBuf::from("/tmp"),
            managed: true,
            created_at: chrono::Utc::now(),
        }],
        confirm_delete: Some("feat-auth".into()),
        ..Default::default()
    };
    apply_ipc_msg(&mut state, IpcMessage::WorldDeleted { world_id: "feat-auth".into() });
    assert!(state.worlds.is_empty());
    assert!(state.confirm_delete.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test confirm_delete_starts_none ipc_rx_drains_state_snapshot ipc_rx_world_deleted 2>&1 | tail -15
```

Expected: compile error — `confirm_delete`, `ipc_tx`, `status_msg`, `apply_ipc_msg` don't exist yet.

- [ ] **Step 3: Add new fields to `AppState`**

Replace the `AppState` struct in `src/tui/mod.rs`:

```rust
#[derive(Default)]
pub struct AppState {
    pub worlds: Vec<World>,
    pub agents: Vec<Agent>,
    pub conflicts: Vec<Conflict>,
    pub audit_log: Vec<AuditEvent>,
    pub selected_world: usize,
    pub view: View,
    pub audit_scroll: usize,
    pub world_env: Option<String>,
    pub confirm_delete: Option<String>,
    pub ipc_tx: Option<std::sync::mpsc::Sender<crate::types::IpcMessage>>,
    pub status_msg: Option<String>,
}
```

- [ ] **Step 4: Add `apply_ipc_msg` function**

Add this public function before `run_tui` in `src/tui/mod.rs`:

```rust
pub fn apply_ipc_msg(state: &mut AppState, msg: crate::types::IpcMessage) {
    use crate::types::IpcMessage;
    match msg {
        IpcMessage::StateSnapshot { worlds, agents, conflicts } => {
            state.worlds = worlds;
            state.agents = agents;
            state.conflicts = conflicts;
        }
        IpcMessage::EventNotification { event } => {
            state.audit_log.insert(0, event);
            state.audit_log.truncate(200);
        }
        IpcMessage::WorldDeleted { world_id } => {
            state.worlds.retain(|w| w.id != world_id);
            if state.selected_world >= state.worlds.len() && !state.worlds.is_empty() {
                state.selected_world = state.worlds.len() - 1;
            }
            state.confirm_delete = None;
        }
        _ => {}
    }
}
```

- [ ] **Step 5: Add `spawn_ipc_thread` function**

Add this function before `run_tui` in `src/tui/mod.rs`:

```rust
fn spawn_ipc_thread(
    socket_path: std::path::PathBuf,
) -> (
    std::sync::mpsc::Sender<crate::types::IpcMessage>,
    std::sync::mpsc::Receiver<crate::types::IpcMessage>,
) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<crate::types::IpcMessage>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<crate::types::IpcMessage>();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime for IPC thread");
        rt.block_on(async move {
            let Ok(mut client) = crate::ipc::client::IpcClient::connect(&socket_path).await
            else {
                return;
            };
            let _ = client.send(&crate::types::IpcMessage::Subscribe).await;

            loop {
                // Drain outgoing commands (non-blocking)
                while let Ok(cmd) = cmd_rx.try_recv() {
                    if client.send(&cmd).await.is_err() {
                        return;
                    }
                }
                // Receive one incoming message (100ms timeout to stay responsive)
                match tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    client.recv(),
                )
                .await
                {
                    Ok(Ok(msg)) => {
                        let _ = evt_tx.send(msg);
                    }
                    Ok(Err(_)) => return, // daemon disconnected
                    Err(_) => {}          // timeout, loop again
                }
            }
        });
    });

    (cmd_tx, evt_rx)
}
```

- [ ] **Step 6: Wire IPC thread into `run_tui`**

In `run_tui`, add after `let mut state = AppState::default();`:

```rust
let socket_path = crate::ipc::socket_path(repo_root);
let evt_rx: Option<std::sync::mpsc::Receiver<crate::types::IpcMessage>> =
    if socket_path.exists() {
        let (cmd_tx, evt_rx) = spawn_ipc_thread(socket_path);
        state.ipc_tx = Some(cmd_tx);
        Some(evt_rx)
    } else {
        None
    };
```

And in the main loop, add event draining at the top (before `terminal.draw`):

```rust
// Drain IPC events
if let Some(ref rx) = evt_rx {
    while let Ok(msg) = rx.try_recv() {
        apply_ipc_msg(&mut state, msg);
    }
}
```

Also add the required import at the top of `src/tui/mod.rs`:

```rust
use crate::types::IpcMessage;
```

- [ ] **Step 7: Run tests**

```bash
cargo test confirm_delete_starts_none ipc_rx_drains_state_snapshot ipc_rx_world_deleted 2>&1 | tail -15
```

Expected: 3 tests pass.

- [ ] **Step 8: Full test suite**

```bash
cargo test 2>&1 | tail -15
```

Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(tui): add IPC background thread and AppState live-update from daemon StateSnapshot"
```

---

## Task 5: TUI Key Bindings for Delete Confirmation

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` block in `src/tui/mod.rs`:

```rust
#[test]
fn d_key_sets_confirm_delete_for_selected_world() {
    use crate::types::World;
    use std::path::PathBuf;

    let mut state = AppState {
        worlds: vec![World {
            id: "feat-auth".into(),
            branch: "feat/auth".into(),
            path: PathBuf::from("/tmp"),
            managed: true,
            created_at: chrono::Utc::now(),
        }],
        selected_world: 0,
        ..Default::default()
    };
    handle_d_key(&mut state);
    assert_eq!(state.confirm_delete, Some("feat-auth".into()));
}

#[test]
fn n_key_cancels_confirm_delete() {
    let mut state = AppState {
        confirm_delete: Some("feat-auth".into()),
        ..Default::default()
    };
    handle_cancel_confirm(&mut state);
    assert!(state.confirm_delete.is_none());
}

#[test]
fn y_key_without_daemon_sets_status_msg() {
    let mut state = AppState {
        confirm_delete: Some("feat-auth".into()),
        ipc_tx: None,
        ..Default::default()
    };
    handle_confirm_delete(&mut state);
    assert!(state.confirm_delete.is_none());
    assert_eq!(state.status_msg.as_deref(), Some("daemon not running"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test d_key_sets_confirm n_key_cancels y_key_without_daemon 2>&1 | tail -15
```

Expected: compile error — helper functions don't exist.

- [ ] **Step 3: Add key handler helpers**

Add these functions to `src/tui/mod.rs` (before `run_tui`):

```rust
pub fn handle_d_key(state: &mut AppState) {
    if let Some(w) = state.worlds.get(state.selected_world) {
        state.confirm_delete = Some(w.id.clone());
    }
}

pub fn handle_cancel_confirm(state: &mut AppState) {
    state.confirm_delete = None;
    state.status_msg = None;
}

pub fn handle_confirm_delete(state: &mut AppState) {
    let world_id = match state.confirm_delete.take() {
        Some(id) => id,
        None => return,
    };
    match &state.ipc_tx {
        Some(tx) => {
            let _ = tx.send(IpcMessage::DeleteWorld { world_id });
        }
        None => {
            state.status_msg = Some("daemon not running".into());
        }
    }
}
```

- [ ] **Step 4: Update the key handler in `run_tui`**

Replace the entire `if let Event::Key(key) = event::read()? { ... }` block with:

```rust
if let Event::Key(key) = event::read()? {
    // Clear transient status on any keypress
    state.status_msg = None;

    // Confirmation overlay takes priority
    if state.confirm_delete.is_some() {
        match key.code {
            KeyCode::Char('y') => handle_confirm_delete(&mut state),
            KeyCode::Char('n') | KeyCode::Esc => handle_cancel_confirm(&mut state),
            _ => {}
        }
        continue;
    }

    match (&state.view, key.code) {
        (View::Dashboard, KeyCode::Char('q')) => break,
        (View::Dashboard, KeyCode::Up) => {
            if state.selected_world > 0 { state.selected_world -= 1; }
        }
        (View::Dashboard, KeyCode::Down) => {
            if state.selected_world + 1 < state.worlds.len() {
                state.selected_world += 1;
            }
        }
        (View::Dashboard, KeyCode::Enter) => {
            if let Some(w) = state.worlds.get(state.selected_world) {
                let env_path = w.path.join(".env");
                state.world_env = std::fs::read_to_string(&env_path).ok();
                state.view = View::WorldDetail(w.id.clone());
            }
        }
        (View::WorldDetail(_), KeyCode::Esc) => {
            state.view = View::Dashboard;
        }
        (View::Dashboard, KeyCode::Char('j')) => {
            state.audit_scroll += 1;
        }
        (View::Dashboard, KeyCode::Char('k')) => {
            if state.audit_scroll > 0 { state.audit_scroll -= 1; }
        }
        (View::Dashboard, KeyCode::Char('d')) => handle_d_key(&mut state),
        _ => {}
    }
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test d_key_sets_confirm n_key_cancels y_key_without_daemon 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 6: Full test suite**

```bash
cargo test 2>&1 | tail -15
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(tui): add delete confirmation key bindings [d]/[y]/[n]"
```

---

## Task 6: Dashboard Confirmation Overlay + Status Bar

**Files:**
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block in `src/tui/dashboard.rs`:

```rust
#[test]
fn centered_rect_is_inside_parent() {
    use ratatui::layout::Rect;
    let area = Rect::new(0, 0, 80, 24);
    let popup = centered_rect(50, 5, area);
    assert!(popup.x >= area.x);
    assert!(popup.y >= area.y);
    assert!(popup.x + popup.width <= area.x + area.width);
    assert!(popup.y + popup.height <= area.y + area.height);
    assert_eq!(popup.height, 5);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test centered_rect_is_inside_parent 2>&1 | tail -10
```

Expected: compile error — `centered_rect` not defined.

- [ ] **Step 3: Add `centered_rect` helper and overlay to `src/tui/dashboard.rs`**

Add the following imports at the top of `src/tui/dashboard.rs` (merge with existing imports):

```rust
use ratatui::layout::Alignment;
use ratatui::widgets::Clear;
```

Add `centered_rect` function at the bottom of the file (before `#[cfg(test)]`):

```rust
pub fn centered_rect(percent_x: u16, height: u16, r: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(r.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
```

- [ ] **Step 4: Update `render` to show overlay and `status_msg`**

In the `render` function in `src/tui/dashboard.rs`, replace the status bar line:

```rust
// Status bar
let help = Paragraph::new("[q]uit  [s]ync  [r]un new agent  [d]elete world  [↑↓]select  [Enter]detail");
f.render_widget(help, chunks[3]);
```

With:

```rust
// Status bar — show transient message or default help
let help_text = if let Some(ref msg) = state.status_msg {
    format!("⚠  {}", msg)
} else {
    "[q]uit  [d]elete world  [↑↓]select  [Enter]detail  [j/k]scroll log".into()
};
let help = Paragraph::new(help_text);
f.render_widget(help, chunks[3]);

// Confirmation overlay (rendered last so it appears on top)
if let Some(ref world_id) = state.confirm_delete {
    let popup_area = centered_rect(52, 4, size);
    f.render_widget(Clear, popup_area);
    let text = format!("Delete \"{}\" + kill agent?\n\n[y] confirm        [n] cancel", world_id);
    let popup = Paragraph::new(text)
        .block(Block::default().title(" Confirm Delete ").borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(popup, popup_area);
}
```

- [ ] **Step 5: Run test**

```bash
cargo test centered_rect_is_inside_parent 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 6: Full test suite**

```bash
cargo test 2>&1 | tail -15
```

Expected: all tests pass.

- [ ] **Step 7: Build release to verify no warnings**

```bash
cargo build 2>&1 | grep -E "^error|^warning" | grep -v "unused import" | head -20
```

Expected: no errors; warnings about unused imports only (if any).

- [ ] **Step 8: Commit**

```bash
git add src/tui/dashboard.rs
git commit -m "feat(tui): add confirmation overlay and status bar message for world deletion"
```

---

## Self-Review

**Spec coverage:**
- ✅ `[d]` on selected world → confirmation overlay
- ✅ `[y]` → sends `DeleteWorld` IPC → daemon kills agent + removes worktree + broadcasts `WorldDeleted`
- ✅ `[n]`/`Esc` → cancel
- ✅ `WorldDeleted` received → TUI removes world from list
- ✅ Agents/PIDs visible via `StateSnapshot` from `scan_loop`
- ✅ Graceful degradation when daemon not running (`status_msg = "daemon not running"`)
- ✅ `nix` replaced by `libc` (already a dependency)

**Placeholder scan:** None found. All steps include full code.

**Type consistency:**
- `IpcMessage::DeleteWorld { world_id: String }` — defined Task 1, used Task 3, Task 5
- `IpcMessage::WorldDeleted { world_id: String }` — defined Task 1, sent Task 3, handled Task 4
- `AppState::confirm_delete: Option<String>` — defined Task 4, set Task 5, rendered Task 6
- `AppState::ipc_tx: Option<mpsc::Sender<IpcMessage>>` — defined Task 4, used Task 5
- `AppState::status_msg: Option<String>` — defined Task 4, set Task 5, rendered Task 6
- `apply_ipc_msg(state, msg)` — defined Task 4, used Task 4
- `handle_d_key`, `handle_cancel_confirm`, `handle_confirm_delete` — defined + used Task 5
- `centered_rect(percent_x, height, rect)` — defined + tested Task 6
