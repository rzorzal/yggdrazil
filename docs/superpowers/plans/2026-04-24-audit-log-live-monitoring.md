# Live Audit Log Monitoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ygg monit` receives audit log events in real-time from the daemon via IPC and updates the TUI without restarting.

**Architecture:** The daemon broadcasts `EventNotification` messages to all connected clients via a tokio broadcast channel. The server splits each accepted socket connection into read/write halves and spawns a write task that forwards broadcast messages to the client. The TUI spawns a background thread running a tokio runtime that connects to the daemon, subscribes, and pipes `AuditEvent`s via `std::sync::mpsc` into the synchronous TUI loop, which drains the channel every 500 ms.

**Tech Stack:** Rust, tokio (full features — already in Cargo.toml), ratatui/crossterm (existing TUI), interprocess v2 (existing IPC), std::sync::mpsc (no new deps)

---

## File Map

| File | Change |
|---|---|
| `src/daemon/mod.rs` | Broadcast `EventNotification` after each `FileModified` append |
| `src/ipc/server.rs` | Split accepted connection; spawn write task per client to forward broadcast |
| `src/ipc/client.rs` | Fix: hold persistent `BufReader` so rapid messages aren't dropped |
| `src/tui/mod.rs` | Add `spawn_ipc_listener`; drain mpsc channel in 500 ms loop |

---

### Task 1: Daemon broadcasts FileModified events

**Files:**
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: Write the failing test**

Add inside the `#[cfg(test)]` block in `src/daemon/mod.rs`:

```rust
#[tokio::test]
async fn hook_report_broadcasts_file_modified_event() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ygg/worlds")).unwrap();
    std::fs::write(dir.path().join(".ygg/shared_memory.json"), "").unwrap();

    let repo_root = dir.path().to_path_buf();
    let handle = tokio::spawn(Daemon::run(repo_root.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let sock = crate::ipc::socket_path(dir.path());
    let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
    // Subscribe so write task starts forwarding
    client.send(&crate::types::IpcMessage::Subscribe).await.unwrap();
    // Give daemon time to register the write task
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    client.send(&crate::types::IpcMessage::HookReport {
        world: "feat-auth".into(),
        files: vec!["src/auth.rs".into()],
    }).await.unwrap();

    let received = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        client.recv(),
    ).await.unwrap().unwrap();

    assert!(
        matches!(
            &received,
            crate::types::IpcMessage::EventNotification {
                event: crate::types::AuditEvent {
                    event: crate::types::EventKind::FileModified,
                    ..
                }
            }
        ),
        "expected FileModified EventNotification, got {:?}", received
    );

    handle.abort();
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test hook_report_broadcasts_file_modified_event -- --nocapture
```

Expected: FAIL — test times out waiting for the `EventNotification` because the daemon doesn't broadcast `FileModified` yet. (Tasks 2 and 3 must also be complete before this test can pass end-to-end, but this step documents intent.)

- [ ] **Step 3: Add broadcast after each FileModified append**

In `src/daemon/mod.rs`, inside the `HookReport` match arm, after the `log.append(...)` call, add the broadcast. The relevant block currently reads:

```rust
if let Ok(mut log) = bus::AuditLog::open(&log_path) {
    for file in &files {
        let _ = log.append(&crate::types::AuditEvent {
            ts: chrono::Utc::now(),
            event: crate::types::EventKind::FileModified,
            world: world.clone(),
            agent: None,
            pid: None,
            file: Some(file.clone()),
            files: None,
            worlds: None,
        });
    }
    // ... conflict detection follows
```

Replace the `for file in &files` loop with:

```rust
for file in &files {
    let event = crate::types::AuditEvent {
        ts: chrono::Utc::now(),
        event: crate::types::EventKind::FileModified,
        world: world.clone(),
        agent: None,
        pid: None,
        file: Some(file.clone()),
        files: None,
        worlds: None,
    };
    let _ = log.append(&event);
    let _ = tx.send(crate::types::IpcMessage::EventNotification {
        event,
    });
}
```

- [ ] **Step 4: Verify compilation**

```bash
cargo build 2>&1
```

Expected: compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs
git commit -m "feat: broadcast FileModified EventNotification from daemon"
```

---

### Task 2: Server forwards broadcast messages to connected clients

**Files:**
- Modify: `src/ipc/server.rs`

- [ ] **Step 1: Write the failing test**

Add inside the `#[cfg(test)]` block in `src/ipc/server.rs`:

```rust
#[tokio::test]
async fn server_pushes_broadcast_to_connected_client() {
    use crate::types::{AuditEvent, EventKind, IpcMessage};
    use tokio::io::AsyncWriteExt;

    let dir = tempdir().unwrap();
    let sock = crate::ipc::socket_path(dir.path());
    std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

    let mut server = IpcServer::new(&sock).await.unwrap();
    let tx = server.tx.clone();

    tokio::spawn(async move {
        server.accept_loop(|_msg| async move {}).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
    // Send Subscribe so connection is established
    client.send(&IpcMessage::Subscribe).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Server broadcasts an event
    let _ = tx.send(IpcMessage::EventNotification {
        event: AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: None,
            pid: None,
            file: Some("src/auth.rs".into()),
            files: None,
            worlds: None,
        },
    });

    let received = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        client.recv(),
    ).await.unwrap().unwrap();

    assert!(
        matches!(received, IpcMessage::EventNotification { .. }),
        "expected EventNotification, got {:?}", received
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test server_pushes_broadcast_to_connected_client -- --nocapture
```

Expected: FAIL — times out because the server currently doesn't write back to clients.

- [ ] **Step 3: Add imports to `src/ipc/server.rs`**

At the top of the file, add `AsyncWriteExt` to the tokio import:

```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
```

- [ ] **Step 4: Rewrite `accept_loop` to split connection and spawn write task**

Replace the entire `accept_loop` method body with:

```rust
pub async fn accept_loop<F, Fut>(&mut self, handler: F) -> Result<()>
where
    F: Fn(IpcMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send,
{
    let handler = std::sync::Arc::new(handler);
    loop {
        let conn = self.listener.accept().await?;
        let tx = self.tx.clone();
        let handler = handler.clone();
        let mut rx = self.tx.subscribe();

        let (read_half, mut write_half) = tokio::io::split(conn);

        // Write task: forward all broadcast messages to this client
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        match serde_json::to_string(&msg) {
                            Ok(mut line) => {
                                line.push('\n');
                                if write_half.write_all(line.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });

        // Read task: receive messages from client and call handler
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                if let Ok(msg) = serde_json::from_str::<IpcMessage>(line.trim()) {
                    handler(msg.clone()).await;
                    let _ = tx.send(msg);
                }
                line.clear();
            }
        });
    }
}
```

- [ ] **Step 5: Run all tests**

```bash
cargo test -- --nocapture 2>&1
```

Expected: `server_pushes_broadcast_to_connected_client` passes. Existing tests (`server_accepts_and_echoes_subscribe`, `client_sends_hook_report`) pass.

- [ ] **Step 6: Commit**

```bash
git add src/ipc/server.rs
git commit -m "feat: server forwards broadcast messages to connected clients"
```

---

### Task 3: Fix IpcClient to use persistent BufReader

**Context:** `IpcClient::recv()` currently creates a new `BufReader` on each call. If the server sends two messages back-to-back, the first `BufReader` may buffer both messages but only return one line — then the buffer is dropped and the second message is silently lost. This must be fixed before the TUI listener loop will work reliably.

**Files:**
- Modify: `src/ipc/client.rs`

- [ ] **Step 1: Write the failing test**

Add inside the `#[cfg(test)]` block in `src/ipc/client.rs`:

```rust
#[tokio::test]
async fn client_receives_multiple_rapid_messages() {
    use crate::types::{AuditEvent, EventKind, IpcMessage};

    let dir = tempdir().unwrap();
    let sock = crate::ipc::socket_path(dir.path());
    std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

    let mut server = crate::ipc::server::IpcServer::new(&sock).await.unwrap();
    let tx = server.tx.clone();

    tokio::spawn(async move {
        server.accept_loop(|_| async move {}).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&sock).await.unwrap();
    client.send(&IpcMessage::Subscribe).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Broadcast two messages in rapid succession
    let make_event = |f: &str| IpcMessage::EventNotification {
        event: AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "w".into(),
            agent: None, pid: None,
            file: Some(f.to_string()),
            files: None, worlds: None,
        },
    };
    let _ = tx.send(make_event("a.rs"));
    let _ = tx.send(make_event("b.rs"));

    let msg1 = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        client.recv(),
    ).await.unwrap().unwrap();

    let msg2 = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        client.recv(),
    ).await.unwrap().unwrap();

    assert!(matches!(msg1, IpcMessage::EventNotification { .. }));
    assert!(matches!(msg2, IpcMessage::EventNotification { .. }));
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test client_receives_multiple_rapid_messages -- --nocapture
```

Expected: FAIL — second `recv()` hangs or errors because the buffered second line was dropped.

- [ ] **Step 3: Rewrite `IpcClient` to hold a persistent `BufReader`**

Replace the entire contents of `src/ipc/client.rs` with:

```rust
use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{tokio::prelude::*, ConnectOptions, GenericFilePath};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct IpcClient {
    reader: BufReader<tokio::io::ReadHalf<interprocess::local_socket::tokio::Stream>>,
    writer: tokio::io::WriteHalf<interprocess::local_socket::tokio::Stream>,
}

impl IpcClient {
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let name = socket_path
            .to_str()
            .unwrap()
            .to_fs_name::<GenericFilePath>()?;
        let stream = ConnectOptions::new().name(name).connect_tokio().await?;
        let (read_half, write_half) = tokio::io::split(stream);
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
        })
    }

    pub async fn send(&mut self, msg: &IpcMessage) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<IpcMessage> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        let msg = serde_json::from_str(line.trim())?;
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IpcMessage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn client_sends_hook_report() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let mut server = crate::ipc::server::IpcServer::new(&sock).await.unwrap();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::channel(1);

        tokio::spawn(async move {
            server.accept_loop(move |msg| {
                let tx = result_tx.clone();
                async move { let _ = tx.send(msg).await; }
            }).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = IpcClient::connect(&sock).await.unwrap();
        client.send(&IpcMessage::HookReport {
            world: "feat-auth".into(),
            files: vec!["src/auth.rs".into()],
        }).await.unwrap();

        let received = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            result_rx.recv(),
        ).await.unwrap().unwrap();

        assert!(matches!(received, IpcMessage::HookReport { .. }));
    }

    #[tokio::test]
    async fn client_receives_multiple_rapid_messages() {
        use crate::types::{AuditEvent, EventKind};

        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let mut server = crate::ipc::server::IpcServer::new(&sock).await.unwrap();
        let tx = server.tx.clone();

        tokio::spawn(async move {
            server.accept_loop(|_| async move {}).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = IpcClient::connect(&sock).await.unwrap();
        client.send(&IpcMessage::Subscribe).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let make_event = |f: &str| IpcMessage::EventNotification {
            event: AuditEvent {
                ts: chrono::Utc::now(),
                event: EventKind::FileModified,
                world: "w".into(),
                agent: None, pid: None,
                file: Some(f.to_string()),
                files: None, worlds: None,
            },
        };
        let _ = tx.send(make_event("a.rs"));
        let _ = tx.send(make_event("b.rs"));

        let msg1 = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.recv(),
        ).await.unwrap().unwrap();

        let msg2 = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.recv(),
        ).await.unwrap().unwrap();

        assert!(matches!(msg1, IpcMessage::EventNotification { .. }));
        assert!(matches!(msg2, IpcMessage::EventNotification { .. }));
    }
}
```

- [ ] **Step 4: Run all tests**

```bash
cargo test -- --nocapture 2>&1
```

Expected: `client_receives_multiple_rapid_messages` passes. `client_sends_hook_report` passes. All other tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ipc/client.rs
git commit -m "fix: IpcClient holds persistent BufReader to prevent message loss"
```

---

### Task 4: TUI spawns IPC listener and drains live events

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Write the unit test for mpsc drain logic**

Add inside the `#[cfg(test)]` block in `src/tui/mod.rs`:

```rust
#[test]
fn ipc_events_appended_to_audit_log() {
    use crate::types::{AuditEvent, EventKind};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<AuditEvent>();

    let event1 = AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::FileModified,
        world: "feat-auth".into(),
        agent: None, pid: None,
        file: Some("src/auth.rs".into()),
        files: None, worlds: None,
    };
    let event2 = AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::FileModified,
        world: "feat-api".into(),
        agent: None, pid: None,
        file: Some("src/auth.rs".into()),
        files: None, worlds: None,
    };

    tx.send(event1).unwrap();
    tx.send(event2).unwrap();
    drop(tx);

    let mut state = AppState::default();
    let mut got_new = false;
    while let Ok(event) = rx.try_recv() {
        state.audit_log.push(event);
        got_new = true;
    }
    if got_new {
        state.conflicts = crate::daemon::bus::detect_conflicts(&state.audit_log);
    }

    assert_eq!(state.audit_log.len(), 2);
    // Same file touched by two different worlds → conflict
    assert_eq!(state.conflicts.len(), 1);
    assert_eq!(state.conflicts[0].file, "src/auth.rs");
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test ipc_events_appended_to_audit_log -- --nocapture
```

Expected: FAIL — compilation error because `spawn_ipc_listener` and `ipc_rx` don't exist yet.

- [ ] **Step 3: Add imports to `src/tui/mod.rs`**

Add these imports at the top of `src/tui/mod.rs` (after existing imports):

```rust
use crate::ipc::client::IpcClient;
use crate::types::IpcMessage;
use std::sync::mpsc;
```

- [ ] **Step 4: Add `spawn_ipc_listener` function**

Add this function before `pub fn run_tui` in `src/tui/mod.rs`:

```rust
fn spawn_ipc_listener(repo_root: std::path::PathBuf, tx: mpsc::Sender<crate::types::AuditEvent>) {
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Runtime::new() else { return };
        rt.block_on(async move {
            let sock = crate::ipc::socket_path(&repo_root);
            let Ok(mut client) = IpcClient::connect(&sock).await else { return };
            let _ = client.send(&IpcMessage::Subscribe).await;
            loop {
                match client.recv().await {
                    Ok(IpcMessage::EventNotification { event }) => {
                        if tx.send(event).is_err() {
                            break; // TUI exited, receiver dropped
                        }
                    }
                    Err(_) => break, // daemon disconnected
                    _ => {}
                }
            }
        });
    });
}
```

- [ ] **Step 5: Wire channel into `run_tui`**

In `pub fn run_tui(repo_root: &Path) -> Result<()>`, after `state.conflicts = ...` (the startup load block), add:

```rust
let (ipc_tx, ipc_rx) = mpsc::channel::<crate::types::AuditEvent>();
spawn_ipc_listener(repo_root.to_path_buf(), ipc_tx);
```

Then, inside the main loop, after the `if event::poll(...)` block, add the drain:

```rust
// Drain IPC events into state
let mut got_new = false;
while let Ok(event) = ipc_rx.try_recv() {
    state.audit_log.push(event);
    got_new = true;
}
if got_new {
    state.conflicts = crate::daemon::bus::detect_conflicts(&state.audit_log);
}
```

The loop body should now look like:

```rust
loop {
    terminal.draw(|f| match &state.view {
        View::Dashboard => dashboard::render(f, &state),
        View::WorldDetail(id) => world_detail::render(f, &state, id),
    })?;

    if event::poll(Duration::from_millis(500))? {
        if let Event::Key(key) = event::read()? {
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
                _ => {}
            }
        }
    }

    // Drain IPC events into state
    let mut got_new = false;
    while let Ok(event) = ipc_rx.try_recv() {
        state.audit_log.push(event);
        got_new = true;
    }
    if got_new {
        state.conflicts = crate::daemon::bus::detect_conflicts(&state.audit_log);
    }
}
```

- [ ] **Step 6: Run all tests**

```bash
cargo test -- --nocapture 2>&1
```

Expected: all tests pass, including `ipc_events_appended_to_audit_log`.

- [ ] **Step 7: Run full end-to-end smoke test**

```bash
cargo build 2>&1 && echo "BUILD OK"
```

Expected: `BUILD OK` with no warnings about unused imports.

- [ ] **Step 8: Run the integration test from Task 1**

```bash
cargo test hook_report_broadcasts_file_modified_event -- --nocapture
```

Expected: PASS — full chain works: HookReport → daemon broadcasts → server forwards to client → client receives EventNotification.

- [ ] **Step 9: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat: live audit log updates in ygg monit via IPC"
```

---

## Done

Run `ygg daemon` in one terminal, `ygg monit` in another, then trigger `ygg hook` from a world. The audit log panel in the dashboard should update within 500 ms without restarting.
