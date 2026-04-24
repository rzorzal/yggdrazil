# Live Audit Log in `ygg monit` via IPC

**Date:** 2026-04-24  
**Status:** Approved

## Problem

`ygg monit` loads `shared_memory.json` once at startup. The audit log panel shows stale data — no live updates while the TUI is open.

## Goal

Audit log in the monitoring dashboard updates in real-time as agents emit events, sourced from the daemon via IPC.

## Architecture

4 files change, no new files.

| File | Change |
|---|---|
| `src/ipc/server.rs` | Split connection into read+write halves; spawn write task per client that forwards from broadcast channel |
| `src/daemon/mod.rs` | After appending `FileModified` event, broadcast `EventNotification` via `tx` |
| `src/tui/mod.rs` | Spawn IPC listener thread with tokio runtime; `std::sync::mpsc` channel bridges async IPC to sync TUI loop; drain in 500ms loop |

### Flow

```
HookReport → daemon appends FileModified → tx.broadcast(EventNotification)
                                                      ↓
                                    write task per connection → socket
                                                      ↓
                                       TUI IPC thread → mpsc::Sender
                                                      ↓
                              500ms loop drains channel → AppState.audit_log
```

## Components

### `src/daemon/mod.rs`

After each `FileModified` append, broadcast the event:

```rust
let _ = tx.send(IpcMessage::EventNotification {
    event: AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::FileModified,
        world: world.clone(),
        agent: None,
        pid: None,
        file: Some(file.clone()),
        files: None,
        worlds: None,
    },
});
```

### `src/ipc/server.rs`

Split accepted connection; spawn write task per client that forwards all broadcast messages:

```rust
let (read_half, mut write_half) = tokio::io::split(conn);
let mut rx = self.tx.subscribe();

tokio::spawn(async move {
    while let Ok(msg) = rx.recv().await {
        let mut line = serde_json::to_string(&msg).unwrap();
        line.push('\n');
        if write_half.write_all(line.as_bytes()).await.is_err() { break; }
    }
});

// existing read task uses read_half instead of conn
```

### `src/tui/mod.rs`

Before the TUI loop, spawn IPC listener thread:

```rust
fn spawn_ipc_listener(repo_root: PathBuf, tx: std::sync::mpsc::Sender<AuditEvent>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let sock = crate::ipc::socket_path(&repo_root);
            let Ok(mut client) = IpcClient::connect(&sock).await else { return };
            let _ = client.send(&IpcMessage::Subscribe).await;
            loop {
                match client.recv().await {
                    Ok(IpcMessage::EventNotification { event }) => {
                        if tx.send(event).is_err() { break; }
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
        });
    });
}
```

In the 500ms loop, drain the channel and recompute conflicts:

```rust
let mut got_new = false;
while let Ok(event) = rx.try_recv() {
    state.audit_log.push(event);
    got_new = true;
}
if got_new {
    state.conflicts = bus::detect_conflicts(&state.audit_log);
}
```

## Error Handling

| Scenario | Behavior |
|---|---|
| Daemon not running | `connect` fails → thread exits silently → TUI uses static data |
| Daemon stops mid-session | `recv` returns `Err` → thread exits silently |
| TUI closes | `tx.send` returns `Err` (rx dropped) → thread exits |
| Broadcast lag (channel full) | `RecvError::Lagged` → skip lagged events, continue |

## Tests

- `daemon/mod.rs`: `hook_report_broadcasts_file_modified_event` — assert `HookReport` produces `EventNotification { FileModified }` on broadcast channel
- `ipc/server.rs`: `server_pushes_broadcast_to_connected_client` — client connects, server broadcasts, client receives the message
- `tui/mod.rs`: `ipc_events_appended_to_audit_log` — drive mpsc sender, verify `state.audit_log` grows and conflicts recomputed

## Out of Scope

- Filtering audit log by world in the dashboard (existing j/k scroll is sufficient)
- Persisting events received via IPC back to file (daemon already owns the file)
- Backfill of events before TUI connected (startup load from file covers this)
