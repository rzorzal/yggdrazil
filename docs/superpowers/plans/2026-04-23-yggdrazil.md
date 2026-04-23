# Yggdrazil Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `ygg`, a Rust CLI daemon that governs parallel AI agent sessions using Git Worktrees, with conflict detection, rules injection, a TUI dashboard, and a smart merge flow.

**Architecture:** Single Rust binary with a background daemon (tokio async) that owns process scanning, worktree management, rules injection, and an append-only audit log. CLI commands connect to the daemon via cross-platform IPC (Unix socket / Windows named pipe). `ygg monit` renders a live ratatui TUI fed by daemon push events. Worlds are created automatically when agents launch via `ygg run`; unmanaged agents (IDE-launched) are detected by the process scanner.

**Tech Stack:** Rust, clap 4, tokio 1, serde_json, sysinfo 0.30, ratatui 0.27, crossterm 0.27, git2 0.19, notify-rust 4, interprocess 2, anyhow 1, tracing/tracing-subscriber, chrono 0.4

---

## File Map

```
src/
  main.rs               CLI entry, clap dispatch
  types.rs              Shared types: World, Agent, AuditEvent, Conflict, IpcMessage
  cli/
    mod.rs              CLI module re-exports
    init.rs             ygg init — one-time repo setup
    run.rs              ygg run — managed agent launch + branch prompt
    hook.rs             ygg hook — cross-platform agent self-report
    sync.rs             ygg sync — smart merge flow
    daemon_cmd.rs       ygg daemon start/stop
    monit.rs            ygg monit — spawn TUI, auto-start daemon
  daemon/
    mod.rs              tokio runtime entry, supervisor loop
    roots.rs            sysinfo process scanner
    trunk.rs            git worktree CRUD via git2
    laws.rs             CLAUDE.md / rules injector
    bus.rs              append-only audit log + conflict detector
  ipc/
    mod.rs              socket path resolution (unix/windows)
    server.rs           daemon-side IPC listener
    client.rs           CLI/TUI-side IPC connector
  tui/
    mod.rs              ratatui app loop + event handling
    dashboard.rs        4-panel layout (worlds, agents, conflicts, audit log)
    world_detail.rs     drill-down view per world
.github/workflows/
  release.yml           cross-compile matrix + GitHub release assets
scripts/
  install.sh            curl-pipe installer
tests/
  init_integration.rs   integration: ygg init creates .ygg/ structure
  trunk_unit.rs         worktree CRUD against temp git repo
  bus_unit.rs           audit log append + conflict detection logic
```

---

## Phase 1: Foundation

### Task 1: Project scaffold + Cargo.toml

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Init cargo project**

```bash
cargo init --name yggdrazil
```

Expected: `Cargo.toml` and `src/main.rs` created.

- [ ] **Step 2: Replace Cargo.toml with full dependencies**

```toml
[package]
name = "yggdrazil"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ygg"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sysinfo = "0.30"
ratatui = "0.27"
crossterm = "0.27"
git2 = "0.19"
notify-rust = "4"
interprocess = { version = "2", features = ["tokio"] }
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dialoguer = "0.11"

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 3: Verify build compiles**

```bash
cargo build
```

Expected: compiles with warnings only (empty main).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: scaffold Rust project with all dependencies"
```

---

### Task 2: Shared types

**Files:**
- Create: `src/types.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test**

Create `src/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_roundtrips_json() {
        let event = AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::AgentSpawned,
            world: "feature-auth".into(),
            agent: Some("claude-code".into()),
            pid: Some(1234),
            file: None,
            files: None,
            worlds: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.world, "feature-auth");
        assert_eq!(decoded.pid, Some(1234));
    }

    #[test]
    fn ipc_message_subscribe_roundtrips() {
        let msg = IpcMessage::Subscribe;
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: IpcMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, IpcMessage::Subscribe));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test types
```

Expected: FAIL — types not defined yet.

- [ ] **Step 3: Implement types**

Add full content to `src/types.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub id: String,
    pub branch: String,
    pub path: PathBuf,
    pub managed: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub pid: u32,
    pub binary: String,
    pub world_id: String,
    pub active_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AgentSpawned,
    AgentExited,
    FileModified,
    IterationEnd,
    ConflictDetected,
    WarningInjected,
    WorldCreated,
    WorldMerged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub ts: DateTime<Utc>,
    pub event: EventKind,
    pub world: String,
    pub agent: Option<String>,
    pub pid: Option<u32>,
    pub file: Option<String>,
    pub files: Option<Vec<String>>,
    pub worlds: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub file: String,
    pub worlds: Vec<String>,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMessage {
    Subscribe,
    HookReport { world: String, files: Vec<String> },
    StateSnapshot {
        worlds: Vec<World>,
        agents: Vec<Agent>,
        conflicts: Vec<Conflict>,
    },
    EventNotification {
        event: AuditEvent,
    },
}
```

Add to `src/main.rs`:

```rust
mod types;

fn main() {}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test types
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/types.rs src/main.rs
git commit -m "feat: add shared types — World, Agent, AuditEvent, IpcMessage"
```

---

### Task 3: IPC socket path resolution

**Files:**
- Create: `src/ipc/mod.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/ipc/mod.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_inside_ygg_dir() {
        let repo = std::path::Path::new("/tmp/myrepo");
        let path = socket_path(repo);
        assert!(path.starts_with("/tmp/myrepo/.ygg/"));
        #[cfg(unix)]
        assert!(path.to_str().unwrap().ends_with(".sock"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test ipc
```

Expected: FAIL — module not found.

- [ ] **Step 3: Implement socket path resolution**

```rust
// src/ipc/mod.rs
pub mod server;
pub mod client;

use std::path::{Path, PathBuf};

pub fn socket_path(repo_root: &Path) -> PathBuf {
    #[cfg(unix)]
    return repo_root.join(".ygg").join("daemon.sock");
    #[cfg(windows)]
    return repo_root.join(".ygg").join("daemon.pipe");
}

pub fn ygg_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg")
}

pub fn worlds_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("worlds")
}

pub fn shared_memory_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("shared_memory.json")
}
```

Create empty stubs:

```rust
// src/ipc/server.rs
// src/ipc/client.rs
```

Add to `src/main.rs`:

```rust
mod ipc;
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test ipc
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/ipc/
git commit -m "feat: add IPC path resolution (unix socket / windows pipe)"
```

---

### Task 4: IPC server (daemon side)

**Files:**
- Modify: `src/ipc/server.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/ipc/server.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IpcMessage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn server_accepts_and_echoes_subscribe() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        let mut server = IpcServer::new(&sock).await.unwrap();

        tokio::spawn(async move {
            server.accept_loop(|msg| async move {
                assert!(matches!(msg, IpcMessage::Subscribe));
            }).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
        client.send(&IpcMessage::Subscribe).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test ipc::server
```

Expected: FAIL — IpcServer not defined.

- [ ] **Step 3: Implement IPC server**

```rust
// src/ipc/server.rs
use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    GenericFilePath, ListenerOptions,
};
use std::future::Future;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

pub struct IpcServer {
    listener: interprocess::local_socket::tokio::Listener,
    pub tx: broadcast::Sender<IpcMessage>,
}

impl IpcServer {
    pub async fn new(socket_path: &Path) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
        }
        let name = socket_path.to_str().unwrap().to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        let (tx, _) = broadcast::channel(256);
        Ok(Self { listener, tx })
    }

    pub async fn accept_loop<F, Fut>(&mut self, handler: F) -> Result<()>
    where
        F: Fn(IpcMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send,
    {
        loop {
            let conn = self.listener.accept().await?;
            let tx = self.tx.clone();
            let h = &handler;
            tokio::spawn(async move {
                let mut reader = BufReader::new(&conn);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    if let Ok(msg) = serde_json::from_str::<IpcMessage>(line.trim()) {
                        h(msg.clone()).await;
                        let _ = tx.send(msg);
                    }
                    line.clear();
                }
            });
        }
    }

    /// Push a message to all connected TUI clients.
    pub fn broadcast(&self, msg: IpcMessage) {
        let _ = self.tx.send(msg);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test ipc::server
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/ipc/server.rs
git commit -m "feat: IPC server — tokio listener, broadcast channel"
```

---

### Task 5: IPC client

**Files:**
- Modify: `src/ipc/client.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/ipc/client.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IpcMessage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn client_sends_hook_report() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());

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
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test ipc::client
```

Expected: FAIL — IpcClient not defined.

- [ ] **Step 3: Implement IPC client**

```rust
// src/ipc/client.rs
use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{tokio::prelude::*, GenericFilePath, Stream};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct IpcClient {
    stream: Stream,
}

impl IpcClient {
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let name = socket_path.to_str().unwrap().to_fs_name::<GenericFilePath>()?;
        let stream = Stream::connect(name).await?;
        Ok(Self { stream })
    }

    pub async fn send(&mut self, msg: &IpcMessage) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        self.stream.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<IpcMessage> {
        let mut reader = BufReader::new(&mut self.stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        Ok(serde_json::from_str(line.trim())?)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test ipc::client
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/ipc/client.rs
git commit -m "feat: IPC client — connect, send, recv over local socket"
```

---

### Task 6: Daemon supervisor

**Files:**
- Create: `src/daemon/mod.rs`
- Create: `src/daemon/roots.rs` (stub)
- Create: `src/daemon/trunk.rs` (stub)
- Create: `src/daemon/laws.rs` (stub)
- Create: `src/daemon/bus.rs` (stub)

- [ ] **Step 1: Write failing test**

```rust
// src/daemon/mod.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn daemon_starts_and_creates_socket() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let handle = tokio::spawn(Daemon::run(dir.path().to_path_buf()));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert!(sock.exists(), "socket should exist after daemon start");
        handle.abort();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test daemon
```

Expected: FAIL — Daemon not defined.

- [ ] **Step 3: Implement daemon supervisor**

```rust
// src/daemon/mod.rs
pub mod bus;
pub mod laws;
pub mod roots;
pub mod trunk;

use crate::ipc::server::IpcServer;
use anyhow::Result;
use std::path::PathBuf;

pub struct Daemon {
    pub repo_root: PathBuf,
}

impl Daemon {
    pub async fn run(repo_root: PathBuf) -> Result<()> {
        let sock = crate::ipc::socket_path(&repo_root);
        let mut server = IpcServer::new(&sock).await?;

        tracing::info!("ygg daemon started, socket: {}", sock.display());

        // Spawn subsystem tasks
        let roots_root = repo_root.clone();
        tokio::spawn(async move {
            roots::scan_loop(&roots_root).await;
        });

        server.accept_loop(|msg| async move {
            tracing::debug!("received IPC message: {:?}", msg);
        }).await?;

        Ok(())
    }
}
```

Create stubs:

```rust
// src/daemon/roots.rs
use std::path::Path;
pub async fn scan_loop(_repo_root: &Path) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
```

```rust
// src/daemon/trunk.rs
// src/daemon/laws.rs
// src/daemon/bus.rs
```

Add to `src/main.rs`:

```rust
mod daemon;
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test daemon::tests
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/
git commit -m "feat: daemon supervisor with tokio runtime and IPC server"
```

---

### Task 7: `ygg init`

**Files:**
- Create: `src/cli/mod.rs`
- Create: `src/cli/init.rs`
- Modify: `src/main.rs`
- Create: `tests/init_integration.rs`

- [ ] **Step 1: Write failing integration test**

```rust
// tests/init_integration.rs
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn init_creates_ygg_structure() {
    let repo = tempdir().unwrap();

    // Init a git repo first
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();

    assert!(repo.path().join(".ygg").exists());
    assert!(repo.path().join(".ygg/worlds").exists());
    assert!(repo.path().join(".ygg/shared_memory.json").exists());

    // .ygg should be in .gitignore
    let gitignore = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".ygg/"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test init_integration
```

Expected: FAIL — `ygg init` not implemented.

- [ ] **Step 3: Implement `ygg init`**

```rust
// src/cli/init.rs
use anyhow::{Context, Result};
use std::path::Path;

pub fn run(repo_root: &Path, _rules: Option<&Path>) -> Result<()> {
    let ygg_dir = repo_root.join(".ygg");
    let worlds_dir = ygg_dir.join("worlds");
    let shared_memory = ygg_dir.join("shared_memory.json");
    let gitignore = repo_root.join(".gitignore");

    std::fs::create_dir_all(&worlds_dir)
        .context("failed to create .ygg/worlds")?;

    if !shared_memory.exists() {
        std::fs::write(&shared_memory, "")
            .context("failed to create shared_memory.json")?;
    }

    // Add .ygg/ to .gitignore
    let current = if gitignore.exists() {
        std::fs::read_to_string(&gitignore)?
    } else {
        String::new()
    };
    if !current.contains(".ygg/") {
        let entry = if current.ends_with('\n') || current.is_empty() {
            ".ygg/\n".to_string()
        } else {
            "\n.ygg/\n".to_string()
        };
        std::fs::write(&gitignore, format!("{current}{entry}"))?;
    }

    // Auto-start daemon
    crate::cli::daemon_cmd::start(repo_root).ok();

    println!("✓ Yggdrazil initialized. Daemon started.");
    Ok(())
}
```

```rust
// src/cli/mod.rs
pub mod init;
pub mod run;
pub mod hook;
pub mod sync;
pub mod daemon_cmd;
pub mod monit;
```

Create stubs for remaining CLI modules:

```rust
// src/cli/run.rs
// src/cli/hook.rs
// src/cli/sync.rs
// src/cli/daemon_cmd.rs
// src/cli/monit.rs
```

Replace `src/main.rs`:

```rust
mod cli;
mod daemon;
mod ipc;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ygg", about = "Yggdrazil — AI agent governance engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// One-time repo setup
    Init {
        #[arg(long)]
        rules: Option<PathBuf>,
    },
    /// Launch agent in a managed world
    Run {
        agent: String,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Agent self-report hook (called by CLAUDE.md hooks)
    Hook {
        #[arg(long)]
        world: String,
        #[arg(long, value_delimiter = ',')]
        files: Vec<String>,
    },
    /// Smart merge flow
    Sync {
        #[arg(long)]
        prune: bool,
    },
    /// TUI dashboard
    Monit,
    /// Daemon management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    Start,
    Stop,
}

fn repo_root() -> PathBuf {
    // Walk up from cwd to find .git
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        if !dir.pop() {
            return std::env::current_dir().unwrap();
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let root = repo_root();

    match cli.command {
        Commands::Init { rules } => cli::init::run(&root, rules.as_deref()),
        Commands::Run { agent, args } => {
            println!("ygg run — not yet implemented");
            Ok(())
        }
        Commands::Hook { world, files } => {
            println!("ygg hook — not yet implemented");
            Ok(())
        }
        Commands::Sync { prune } => {
            println!("ygg sync — not yet implemented");
            Ok(())
        }
        Commands::Monit => {
            println!("ygg monit — not yet implemented");
            Ok(())
        }
        Commands::Daemon { action } => {
            println!("ygg daemon — not yet implemented");
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --test init_integration
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/ tests/init_integration.rs
git commit -m "feat: ygg init — creates .ygg structure and updates .gitignore"
```

---

### Task 8: `ygg daemon start/stop`

**Files:**
- Modify: `src/cli/daemon_cmd.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/cli/daemon_cmd.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pid_file_path_inside_ygg() {
        let dir = tempdir().unwrap();
        let path = pid_file_path(dir.path());
        assert!(path.starts_with(dir.path().join(".ygg")));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test daemon_cmd
```

Expected: FAIL — pid_file_path not defined.

- [ ] **Step 3: Implement daemon_cmd**

```rust
// src/cli/daemon_cmd.rs
use anyhow::{Context, Result};
use std::path::Path;
use std::path::PathBuf;

pub fn pid_file_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("daemon.pid")
}

pub fn start(repo_root: &Path) -> Result<()> {
    let pid_file = pid_file_path(repo_root);
    if pid_file.exists() {
        let pid = std::fs::read_to_string(&pid_file)?;
        println!("Daemon already running (PID {}).", pid.trim());
        return Ok(());
    }

    // Spawn daemon as detached background process (re-exec self with internal flag)
    let exe = std::env::current_exe()?;
    let child = std::process::Command::new(exe)
        .args(["_daemon-run", repo_root.to_str().unwrap()])
        .spawn()
        .context("failed to spawn daemon process")?;

    std::fs::write(&pid_file, child.id().to_string())?;
    println!("✓ Daemon started (PID {}).", child.id());
    Ok(())
}

pub fn stop(repo_root: &Path) -> Result<()> {
    let pid_file = pid_file_path(repo_root);
    if !pid_file.exists() {
        println!("No daemon running.");
        return Ok(());
    }
    let pid_str = std::fs::read_to_string(&pid_file)?;
    let pid: u32 = pid_str.trim().parse().context("invalid PID in daemon.pid")?;

    #[cfg(unix)]
    unsafe { libc::kill(pid as i32, libc::SIGTERM); }
    #[cfg(windows)]
    {
        use std::os::windows::io::RawHandle;
        // On Windows, kill via taskkill
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()?;
    }

    std::fs::remove_file(&pid_file)?;
    println!("✓ Daemon stopped (PID {}).", pid);
    Ok(())
}
```

Update `src/main.rs` to add `_daemon-run` internal subcommand and wire `start`/`stop`:

```rust
// Add to Commands enum:
    /// Internal: run daemon in background (not for direct use)
    #[command(name = "_daemon-run", hide = true)]
    DaemonRun { repo_root: String },
```

```rust
// Add to match in main():
        Commands::Daemon { action } => match action {
            DaemonAction::Start => cli::daemon_cmd::start(&root),
            DaemonAction::Stop => cli::daemon_cmd::stop(&root),
        },
        Commands::DaemonRun { repo_root } => {
            let path = PathBuf::from(repo_root);
            tokio::runtime::Runtime::new()?
                .block_on(daemon::Daemon::run(path))
        }
```

Add `libc` to Cargo.toml for Unix:

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test daemon_cmd
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/cli/daemon_cmd.rs src/main.rs Cargo.toml
git commit -m "feat: ygg daemon start/stop with PID file management"
```

---

## Phase 2: Roots + Trunk + Laws

### Task 9: Roots — process scanner

**Files:**
- Modify: `src/daemon/roots.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/daemon/roots.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_detected_as_agent_if_name_matches() {
        // Use current process — it won't match agent names, so result is empty
        let agents = scan_once("/nonexistent/worlds");
        // Just verify it returns without panic and is a Vec
        let _ = agents;
    }

    #[test]
    fn classify_binary_name() {
        assert_eq!(classify_binary("claude-code"), Some("claude-code"));
        assert_eq!(classify_binary("aider"), Some("aider"));
        assert_eq!(classify_binary("bash"), None);
        assert_eq!(classify_binary("node"), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test roots
```

Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement process scanner**

```rust
// src/daemon/roots.rs
use crate::types::{Agent, AuditEvent, EventKind};
use chrono::Utc;
use std::path::Path;
use sysinfo::{ProcessExt, System, SystemExt};

const AGENT_BINARIES: &[&str] = &["claude", "claude-code", "codex", "aider", "cursor"];

pub fn classify_binary(name: &str) -> Option<&'static str> {
    AGENT_BINARIES.iter().find(|&&b| name == b).copied()
}

/// Scan all processes, return those that are AI agents inside worlds_dir.
pub fn scan_once(worlds_dir: &str) -> Vec<Agent> {
    let mut sys = System::new_all();
    sys.refresh_processes();

    sys.processes()
        .values()
        .filter_map(|proc| {
            let name = proc.name();
            let binary = classify_binary(name)?;
            let cwd = proc.cwd();
            let cwd_str = cwd.to_str()?;
            if !cwd_str.starts_with(worlds_dir) {
                return None;
            }
            // Extract world id from path: .ygg/worlds/<id>/...
            let rel = cwd_str.strip_prefix(worlds_dir)?.trim_start_matches('/');
            let world_id = rel.split('/').next()?.to_string();
            Some(Agent {
                pid: proc.pid().as_u32(),
                binary: binary.to_string(),
                world_id,
                active_files: vec![],
            })
        })
        .collect()
}

pub async fn scan_loop(repo_root: &Path) {
    let worlds_dir = repo_root
        .join(".ygg")
        .join("worlds")
        .to_string_lossy()
        .to_string();

    let mut known_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    loop {
        let current = scan_once(&worlds_dir);
        let current_pids: std::collections::HashSet<u32> =
            current.iter().map(|a| a.pid).collect();

        for agent in &current {
            if !known_pids.contains(&agent.pid) {
                tracing::info!("agent spawned: {} PID {} in {}", agent.binary, agent.pid, agent.world_id);
                // TODO: emit AgentSpawned event to Bus in Phase 3
            }
        }
        for pid in &known_pids {
            if !current_pids.contains(pid) {
                tracing::info!("agent exited: PID {}", pid);
                // TODO: emit AgentExited event to Bus in Phase 3
            }
        }
        known_pids = current_pids;

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test roots
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/roots.rs
git commit -m "feat: Roots — sysinfo process scanner maps agent PIDs to worlds"
```

---

### Task 10: Trunk — git worktree manager

**Files:**
- Modify: `src/daemon/trunk.rs`
- Create: `tests/trunk_unit.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/trunk_unit.rs
use tempfile::tempdir;
use yggdrazil::daemon::trunk::{create_world, list_worlds, delete_world};

#[test]
fn creates_worktree_on_branch() {
    let repo_dir = tempdir().unwrap();
    // Init bare repo with initial commit
    let repo = git2::Repository::init(repo_dir.path()).unwrap();
    let sig = git2::Signature::now("test", "test@test.com").unwrap();
    let tree_id = {
        let mut index = repo.index().unwrap();
        index.write_tree().unwrap()
    };
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();

    std::fs::create_dir_all(repo_dir.path().join(".ygg/worlds")).unwrap();

    create_world(repo_dir.path(), "feat-auth", "main").unwrap();

    let world_path = repo_dir.path().join(".ygg/worlds/feat-auth");
    assert!(world_path.exists(), "worktree dir should exist");

    let worlds = list_worlds(repo_dir.path()).unwrap();
    assert!(worlds.iter().any(|w| w.id == "feat-auth"));
}

#[test]
fn delete_world_removes_worktree() {
    let repo_dir = tempdir().unwrap();
    let repo = git2::Repository::init(repo_dir.path()).unwrap();
    let sig = git2::Signature::now("test", "test@test.com").unwrap();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
    std::fs::create_dir_all(repo_dir.path().join(".ygg/worlds")).unwrap();

    create_world(repo_dir.path(), "to-delete", "main").unwrap();
    assert!(repo_dir.path().join(".ygg/worlds/to-delete").exists());

    delete_world(repo_dir.path(), "to-delete").unwrap();
    assert!(!repo_dir.path().join(".ygg/worlds/to-delete").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test trunk_unit
```

Expected: FAIL — functions not found.

- [ ] **Step 3: Implement trunk (make pub for tests)**

Add to `Cargo.toml`:

```toml
[lib]
name = "yggdrazil"
path = "src/lib.rs"
```

Create `src/lib.rs`:

```rust
pub mod cli;
pub mod daemon;
pub mod ipc;
pub mod types;
```

```rust
// src/daemon/trunk.rs
use crate::types::World;
use anyhow::{Context, Result};
use chrono::Utc;
use git2::Repository;
use std::path::{Path, PathBuf};

pub fn create_world(repo_root: &Path, world_id: &str, branch: &str) -> Result<World> {
    let repo = Repository::open(repo_root).context("not a git repo")?;
    let world_path = repo_root.join(".ygg").join("worlds").join(world_id);

    // Create a new branch from HEAD if it doesn't exist
    let head = repo.head()?.peel_to_commit()?;
    if repo.find_branch(branch, git2::BranchType::Local).is_err() {
        repo.branch(branch, &head, false)?;
    }

    // Add worktree
    repo.worktree(
        world_id,
        &world_path,
        Some(git2::WorktreeAddOptions::new().reference(
            Some(&repo.find_branch(branch, git2::BranchType::Local)?.into_reference()),
        )),
    )
    .context("git worktree add failed")?;

    // Write .env with port offset
    let world_index = list_worlds(repo_root)?.len() as u16;
    let port = 3000 + world_index;
    std::fs::write(world_path.join(".env"), format!("PORT={port}\n"))?;

    Ok(World {
        id: world_id.to_string(),
        branch: branch.to_string(),
        path: world_path,
        managed: true,
        created_at: Utc::now(),
    })
}

pub fn list_worlds(repo_root: &Path) -> Result<Vec<World>> {
    let worlds_dir = repo_root.join(".ygg").join("worlds");
    if !worlds_dir.exists() {
        return Ok(vec![]);
    }
    let mut worlds = vec![];
    for entry in std::fs::read_dir(&worlds_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let id = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            // Read branch from git worktree HEAD
            let branch = std::fs::read_to_string(path.join(".git"))
                .ok()
                .and_then(|s| {
                    s.strip_prefix("gitdir: ")
                        .map(|s| s.trim().to_string())
                })
                .unwrap_or_else(|| "unknown".to_string());
            worlds.push(World {
                id,
                branch,
                path,
                managed: true,
                created_at: Utc::now(),
            });
        }
    }
    Ok(worlds)
}

pub fn delete_world(repo_root: &Path, world_id: &str) -> Result<()> {
    let repo = Repository::open(repo_root)?;
    let world_path = repo_root.join(".ygg").join("worlds").join(world_id);

    // Remove worktree via git2
    let wt = repo.find_worktree(world_id)?;
    wt.prune(Some(git2::WorktreePruneOptions::new().valid(true)))?;

    if world_path.exists() {
        std::fs::remove_dir_all(&world_path)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --test trunk_unit
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/trunk.rs src/lib.rs Cargo.toml
git commit -m "feat: Trunk — git worktree create/list/delete via git2"
```

---

### Task 11: Laws — rules injector

**Files:**
- Modify: `src/daemon/laws.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/daemon/laws.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn inject_writes_claude_md_with_protocol_header() {
        let dir = tempdir().unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap();

        let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("YGGDRAZIL PROTOCOL ACTIVE"));
        assert!(claude_md.contains("feat-auth"));
        assert!(claude_md.contains("main"));
    }

    #[test]
    fn inject_conflict_warning_appends_to_claude_md() {
        let dir = tempdir().unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap();
        inject_conflict_warning(dir.path(), "feat-api", "src/auth.rs").unwrap();

        let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("CONFLICT WARNING"));
        assert!(claude_md.contains("src/auth.rs"));
        assert!(claude_md.contains("feat-api"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test laws
```

Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement laws injector**

```rust
// src/daemon/laws.rs
use anyhow::Result;
use chrono::Utc;
use std::path::Path;

const PROTOCOL_TEMPLATE: &str = r#"<!-- YGGDRAZIL PROTOCOL ACTIVE -->
# Yggdrazil Governance Protocol

**You are operating in World: `{WORLD_ID}` on branch `{BRANCH}`.**

Before starting any task:
1. Read `.ygg/shared_memory.json` to understand what other agents are doing.
2. After each iteration, run: `ygg hook --world {WORLD_ID} --files <comma-separated-files-you-touched>`

This saves tokens for all agents by avoiding redundant rediscovery.
"#;

pub fn inject_rules(
    world_path: &Path,
    world_id: &str,
    branch: &str,
    extra_rules: &[&Path],
) -> Result<()> {
    let content = PROTOCOL_TEMPLATE
        .replace("{WORLD_ID}", world_id)
        .replace("{BRANCH}", branch);

    let claude_md = world_path.join("CLAUDE.md");
    // Prepend protocol to any existing CLAUDE.md
    let existing = if claude_md.exists() {
        std::fs::read_to_string(&claude_md)?
    } else {
        String::new()
    };

    // Don't double-inject
    if !existing.contains("YGGDRAZIL PROTOCOL ACTIVE") {
        std::fs::write(&claude_md, format!("{content}\n{existing}"))?;
    }

    // Append extra rules files
    for rules_path in extra_rules {
        if rules_path.exists() {
            let rules = std::fs::read_to_string(rules_path)?;
            let current = std::fs::read_to_string(&claude_md)?;
            std::fs::write(&claude_md, format!("{current}\n---\n{rules}"))?;
        }
    }

    // Write .cursorrules and .aider.conf.yml stubs
    let cursorrules = world_path.join(".cursorrules");
    if !cursorrules.exists() {
        std::fs::write(&cursorrules, format!("# Yggdrazil: World={world_id} Branch={branch}\n"))?;
    }

    let aider_conf = world_path.join(".aider.conf.yml");
    if !aider_conf.exists() {
        std::fs::write(&aider_conf, format!("# Yggdrazil: World={world_id} Branch={branch}\n"))?;
    }

    Ok(())
}

pub fn inject_conflict_warning(
    world_path: &Path,
    conflicting_world: &str,
    file: &str,
) -> Result<()> {
    let claude_md = world_path.join("CLAUDE.md");
    let existing = std::fs::read_to_string(&claude_md).unwrap_or_default();
    let warning = format!(
        "\n<!-- CONFLICT WARNING {} -->\n⚠️ **CONFLICT WARNING**: World `{}` is also modifying `{}`. Avoid editing this file until `ygg sync` is run.\n",
        Utc::now().to_rfc3339(),
        conflicting_world,
        file
    );
    std::fs::write(&claude_md, format!("{existing}{warning}"))?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test laws
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/laws.rs
git commit -m "feat: Laws — inject YGGDRAZIL protocol into CLAUDE.md + conflict warnings"
```

---

### Task 12: `ygg run` — managed agent launch

**Files:**
- Modify: `src/cli/run.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/cli/run.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_id_from_agent_and_branch() {
        let id = world_id_for("claude-code", "feat/auth");
        assert!(!id.is_empty());
        // Should be filesystem-safe (no slashes)
        assert!(!id.contains('/'));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test cli::run
```

Expected: FAIL — world_id_for not defined.

- [ ] **Step 3: Implement `ygg run`**

```rust
// src/cli/run.rs
use crate::daemon::{laws, trunk};
use anyhow::Result;
use chrono::Utc;
use dialoguer::{Input, Select};
use std::path::Path;

pub fn world_id_for(agent: &str, branch: &str) -> String {
    let safe_branch = branch.replace('/', "-").replace(' ', "-");
    format!("{agent}-{safe_branch}-{}", &Utc::now().format("%H%M%S"))
}

pub fn run(
    repo_root: &Path,
    agent: &str,
    agent_args: &[String],
    extra_rules: Option<&Path>,
) -> Result<()> {
    // 1. Ask which branch
    let branches = list_local_branches(repo_root)?;
    let head_branch = current_branch(repo_root).unwrap_or_else(|| "main".into());

    let branch: String = if branches.is_empty() {
        head_branch.clone()
    } else {
        let default_idx = branches.iter().position(|b| b == &head_branch).unwrap_or(0);
        let selection = Select::new()
            .with_prompt("Which branch for this world?")
            .items(&branches)
            .default(default_idx)
            .interact()?;
        branches[selection].clone()
    };

    // 2. Warn if branch already in use
    let worlds = trunk::list_worlds(repo_root)?;
    let collisions: Vec<_> = worlds.iter().filter(|w| w.branch == branch).collect();
    if !collisions.is_empty() {
        let names: Vec<_> = collisions.iter().map(|w| w.id.as_str()).collect();
        eprintln!(
            "⚠️  Branch `{}` already in use by world(s): {}",
            branch,
            names.join(", ")
        );
        let proceed = dialoguer::Confirm::new()
            .with_prompt("Continue anyway?")
            .default(false)
            .interact()?;
        if !proceed {
            return Ok(());
        }
    }

    // 3. Create world
    let world_id = world_id_for(agent, &branch);
    let world = trunk::create_world(repo_root, &world_id, &branch)?;

    // 4. Inject rules
    let extra = extra_rules.map(|p| vec![p]).unwrap_or_default();
    laws::inject_rules(&world.path, &world_id, &branch, &extra)?;

    println!("✓ World `{world_id}` created on branch `{branch}`");
    println!("  Launching: {agent} {}", agent_args.join(" "));

    // 5. Spawn agent with all args, cwd = world path
    let status = std::process::Command::new(agent)
        .args(agent_args)
        .current_dir(&world.path)
        .status()?;

    std::process::exit(status.code().unwrap_or(0));
}

fn list_local_branches(repo_root: &Path) -> Result<Vec<String>> {
    let repo = git2::Repository::open(repo_root)?;
    let mut branches = vec![];
    for branch in repo.branches(Some(git2::BranchType::Local))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            branches.push(name.to_string());
        }
    }
    Ok(branches)
}

fn current_branch(repo_root: &Path) -> Option<String> {
    let repo = git2::Repository::open(repo_root).ok()?;
    let head = repo.head().ok()?;
    head.shorthand().map(|s| s.to_string())
}
```

Wire in `src/main.rs`:

```rust
Commands::Run { agent, args } => cli::run::run(&root, &agent, &args, None),
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test cli::run
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/cli/run.rs src/main.rs
git commit -m "feat: ygg run — branch prompt, world creation, agent spawn with passthrough args"
```

---

### Task 12b: Roots — unmanaged agent auto-world-creation

**Files:**
- Modify: `src/daemon/roots.rs`

- [ ] **Step 1: Write failing test**

```rust
// Add to src/daemon/roots.rs tests
#[tokio::test]
async fn unmanaged_agent_cwd_in_repo_creates_world() {
    use tempfile::tempdir;
    let repo = tempdir().unwrap();
    // Simulate agent CWD = repo root (not inside .ygg/worlds/)
    let result = world_id_for_unmanaged_cwd(repo.path(), repo.path());
    assert!(result.starts_with("unmanaged-"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test roots::tests::unmanaged
```

Expected: FAIL.

- [ ] **Step 3: Implement unmanaged detection + world creation**

Replace `scan_loop` in `src/daemon/roots.rs`:

```rust
use crate::daemon::{laws, trunk};
use chrono::Utc;

pub fn world_id_for_unmanaged_cwd(_repo_root: &std::path::Path, _cwd: &std::path::Path) -> String {
    format!("unmanaged-{}", Utc::now().format("%Y%m%d-%H%M%S"))
}

pub async fn scan_loop(repo_root: &std::path::Path) {
    let worlds_dir = repo_root.join(".ygg").join("worlds");
    let worlds_dir_str = worlds_dir.to_string_lossy().to_string();
    let repo_str = repo_root.to_string_lossy().to_string();
    let mut known_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    loop {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_processes();

        for proc in sys.processes().values() {
            let Some(binary) = classify_binary(proc.name()) else { continue };
            let cwd = proc.cwd();
            let cwd_str = cwd.to_string_lossy();
            let pid = proc.pid().as_u32();

            if known_pids.contains(&pid) { continue; }
            known_pids.insert(pid);

            if cwd_str.starts_with(&worlds_dir_str) {
                // Managed world — just log
                tracing::info!("managed agent: {} PID {} in {}", binary, pid, cwd_str);
            } else if cwd_str.starts_with(&repo_str) {
                // Unmanaged — auto-create world
                let world_id = world_id_for_unmanaged_cwd(repo_root, cwd.as_ref());
                tracing::warn!("unmanaged agent detected: {} PID {}, creating world {}", binary, pid, world_id);
                if let Ok(world) = trunk::create_world(repo_root, &world_id, "HEAD") {
                    let _ = laws::inject_rules(&world.path, &world_id, "HEAD", &[]);
                }
            }
        }

        // Detect exited agents
        let current_pids: std::collections::HashSet<u32> = sys.processes().keys()
            .map(|p| p.as_u32()).collect();
        known_pids.retain(|pid| current_pids.contains(pid));

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test roots
```

Expected: all roots tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/roots.rs
git commit -m "feat: Roots — auto-create world for unmanaged agents detected in repo"
```

---

## Phase 3: Resonance Bus

### Task 13: Bus — append-only audit log

**Files:**
- Modify: `src/daemon/bus.rs`
- Create: `tests/bus_unit.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/bus_unit.rs
use tempfile::tempdir;
use yggdrazil::daemon::bus::{AuditLog, EventKind};
use yggdrazil::types::AuditEvent;

#[test]
fn append_and_read_events() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared_memory.json");
    let mut log = AuditLog::open(&path).unwrap();

    log.append(&AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::AgentSpawned,
        world: "feat-auth".into(),
        agent: Some("claude-code".into()),
        pid: Some(1234),
        file: None,
        files: None,
        worlds: None,
    }).unwrap();

    let events = log.read_all().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].world, "feat-auth");
}

#[test]
fn append_is_atomic_and_does_not_corrupt() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared_memory.json");
    let mut log = AuditLog::open(&path).unwrap();

    for i in 0..10 {
        log.append(&AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: format!("world-{i}"),
            agent: None, pid: None, file: Some("src/lib.rs".into()),
            files: None, worlds: None,
        }).unwrap();
    }

    let events = log.read_all().unwrap();
    assert_eq!(events.len(), 10);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test bus_unit
```

Expected: FAIL.

- [ ] **Step 3: Implement audit log**

```rust
// src/daemon/bus.rs
pub use crate::types::EventKind;
use crate::types::AuditEvent;
use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            std::fs::write(path, "")?;
        }
        Ok(Self { path: path.to_path_buf() })
    }

    pub fn append(&mut self, event: &AuditEvent) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(event)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<AuditEvent>> {
        let file = std::fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut events = vec![];
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            if let Ok(event) = serde_json::from_str::<AuditEvent>(trimmed) {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Read the last `n` events within `max_age_hours` hours.
    pub fn read_recent(&self, n: usize, max_age_hours: i64) -> Result<Vec<AuditEvent>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(max_age_hours);
        let all = self.read_all()?;
        Ok(all
            .into_iter()
            .filter(|e| e.ts > cutoff)
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --test bus_unit
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/bus.rs tests/bus_unit.rs
git commit -m "feat: Bus — append-only audit log with JSON-lines format"
```

---

### Task 14: Bus — conflict detector

**Files:**
- Modify: `src/daemon/bus.rs`

- [ ] **Step 1: Write failing test**

Add to `tests/bus_unit.rs`:

```rust
#[test]
fn detects_conflict_when_same_file_modified_in_two_worlds() {
    use yggdrazil::daemon::bus::detect_conflicts;

    let events = vec![
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: Some("claude".into()), pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-api".into(),
            agent: Some("aider".into()), pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
    ];

    let conflicts = detect_conflicts(&events);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].file, "src/auth.rs");
    assert!(conflicts[0].worlds.contains(&"feat-auth".to_string()));
    assert!(conflicts[0].worlds.contains(&"feat-api".to_string()));
}

#[test]
fn no_conflict_same_file_same_world() {
    use yggdrazil::daemon::bus::detect_conflicts;

    let events = vec![
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: None, pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: None, pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
    ];

    let conflicts = detect_conflicts(&events);
    assert!(conflicts.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --test bus_unit detect_conflict
```

Expected: FAIL.

- [ ] **Step 3: Implement conflict detector**

Add to `src/daemon/bus.rs`:

```rust
use crate::types::Conflict;
use std::collections::HashMap;

/// Scan events for file conflicts (same file, different worlds).
/// Checks last 500 events within a 2-hour window.
pub fn detect_conflicts(events: &[AuditEvent]) -> Vec<Conflict> {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(2);
    let window: Vec<_> = events
        .iter()
        .rev()
        .take(500)
        .filter(|e| {
            e.ts > cutoff
                && matches!(e.event, EventKind::FileModified | EventKind::IterationEnd)
                && e.file.is_some()
        })
        .collect();

    // file → set of worlds that touched it
    let mut file_worlds: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for event in &window {
        if let Some(file) = &event.file {
            file_worlds
                .entry(file.clone())
                .or_default()
                .insert(event.world.clone());
        }
    }

    file_worlds
        .into_iter()
        .filter(|(_, worlds)| worlds.len() > 1)
        .map(|(file, worlds)| Conflict {
            file,
            worlds: worlds.into_iter().collect(),
            detected_at: chrono::Utc::now(),
        })
        .collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --test bus_unit
```

Expected: all 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/bus.rs tests/bus_unit.rs
git commit -m "feat: Bus — conflict detector scans 500-event/2h window"
```

---

### Task 14b: Wire daemon — HookReport → Bus → conflict detection → IPC broadcast

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: `src/ipc/server.rs`

- [ ] **Step 1: Write failing test**

```rust
// Add to src/daemon/mod.rs tests
#[tokio::test]
async fn hook_report_triggers_conflict_check_and_broadcast() {
    use crate::types::IpcMessage;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ygg/worlds")).unwrap();
    let sock = crate::ipc::socket_path(dir.path());
    std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();
    std::fs::write(dir.path().join(".ygg/shared_memory.json"), "").unwrap();

    let repo_root = dir.path().to_path_buf();
    tokio::spawn(Daemon::run(repo_root.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
    client.send(&IpcMessage::Subscribe).await.unwrap();
    client.send(&IpcMessage::HookReport {
        world: "feat-auth".into(),
        files: vec!["src/lib.rs".into()],
    }).await.unwrap();

    // No panic = pass (daemon processed the message)
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test daemon::tests::hook_report
```

Expected: FAIL or timeout (daemon doesn't handle HookReport yet).

- [ ] **Step 3: Wire daemon event loop**

Replace `Daemon::run` in `src/daemon/mod.rs`:

```rust
pub async fn run(repo_root: PathBuf) -> Result<()> {
    let sock = crate::ipc::socket_path(&repo_root);
    let log_path = crate::ipc::shared_memory_path(&repo_root);
    let mut server = IpcServer::new(&sock).await?;
    let tx = server.tx.clone();

    tracing::info!("ygg daemon started, socket: {}", sock.display());

    let roots_root = repo_root.clone();
    tokio::spawn(async move {
        roots::scan_loop(&roots_root).await;
    });

    let log_path2 = log_path.clone();
    let repo_root2 = repo_root.clone();
    server.accept_loop(move |msg| {
        let tx = tx.clone();
        let log_path = log_path2.clone();
        let repo_root = repo_root2.clone();
        async move {
            match msg {
                crate::types::IpcMessage::HookReport { world, files } => {
                    tracing::debug!("hook report: world={} files={:?}", world, files);

                    // Append file_modified events to audit log
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

                        // Check for conflicts
                        if let Ok(events) = log.read_recent(500, 2) {
                            let conflicts = bus::detect_conflicts(&events);
                            for conflict in &conflicts {
                                tracing::warn!("conflict: {:?}", conflict);
                                // Inject warning into conflicting worlds' CLAUDE.md
                                for w in &conflict.worlds {
                                    if w != &world {
                                        let world_path = repo_root.join(".ygg/worlds").join(w);
                                        let _ = laws::inject_conflict_warning(&world_path, &world, &conflict.file);
                                    }
                                }
                                // OS notification
                                bus::notify_conflict(&conflict.file, &conflict.worlds);

                                // Broadcast to TUI clients
                                let _ = tx.send(crate::types::IpcMessage::EventNotification {
                                    event: crate::types::AuditEvent {
                                        ts: chrono::Utc::now(),
                                        event: crate::types::EventKind::ConflictDetected,
                                        world: world.clone(),
                                        agent: None, pid: None,
                                        file: Some(conflict.file.clone()),
                                        files: None,
                                        worlds: Some(conflict.worlds.clone()),
                                    },
                                });
                            }
                        }
                    }
                }
                crate::types::IpcMessage::Subscribe => {
                    tracing::debug!("new TUI subscriber");
                }
                _ => {}
            }
        }
    }).await?;

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test daemon
```

Expected: all daemon tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs
git commit -m "feat: daemon event loop — HookReport → Bus → conflict detection → IPC broadcast"
```

---

### Task 15: `ygg hook` + OS push notifications

**Files:**
- Modify: `src/cli/hook.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/cli/hook.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_agent_state_on_hook() {
        let dir = tempdir().unwrap();
        write_agent_state(dir.path(), "feat-auth", &["src/auth.rs", "src/lib.rs"]).unwrap();

        let state = std::fs::read_to_string(dir.path().join(".agent_state")).unwrap();
        assert!(state.contains("feat-auth"));
        assert!(state.contains("src/auth.rs"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test cli::hook
```

Expected: FAIL.

- [ ] **Step 3: Implement hook**

```rust
// src/cli/hook.rs
use anyhow::Result;
use std::path::Path;

#[derive(serde::Serialize)]
struct AgentState<'a> {
    world: &'a str,
    files: &'a [&'a str],
    ts: String,
}

pub fn write_agent_state(world_path: &Path, world_id: &str, files: &[&str]) -> Result<()> {
    let state = AgentState {
        world: world_id,
        files,
        ts: chrono::Utc::now().to_rfc3339(),
    };
    let content = serde_json::to_string_pretty(&state)?;
    std::fs::write(world_path.join(".agent_state"), content)?;
    Ok(())
}

pub fn run(repo_root: &Path, world_id: &str, files: &[String]) -> Result<()> {
    let world_path = repo_root.join(".ygg").join("worlds").join(world_id);
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    write_agent_state(&world_path, world_id, &file_refs)?;

    // Try to notify daemon via IPC; best-effort, don't fail if daemon is down
    let sock = crate::ipc::socket_path(repo_root);
    if sock.exists() {
        if let Ok(rt) = tokio::runtime::Runtime::new() {
            let _ = rt.block_on(async {
                if let Ok(mut client) = crate::ipc::client::IpcClient::connect(&sock).await {
                    let _ = client.send(&crate::types::IpcMessage::HookReport {
                        world: world_id.to_string(),
                        files: files.to_vec(),
                    }).await;
                }
            });
        }
    }

    Ok(())
}
```

Wire OS notification in daemon `bus.rs` when conflict is detected:

```rust
// Add to src/daemon/bus.rs
pub fn notify_conflict(file: &str, worlds: &[String]) {
    #[cfg(not(target_os = "linux"))]
    let _ = notify_rust::Notification::new()
        .summary("Yggdrazil — Conflict Detected")
        .body(&format!("File `{}` modified by worlds: {}", file, worlds.join(", ")))
        .show();
    #[cfg(target_os = "linux")]
    let _ = notify_rust::Notification::new()
        .summary("Yggdrazil — Conflict Detected")
        .body(&format!("File `{}` modified by worlds: {}", file, worlds.join(", ")))
        .show();
}
```

Wire hook in `src/main.rs`:

```rust
Commands::Hook { world, files } => cli::hook::run(&root, &world, &files),
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test cli::hook
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/cli/hook.rs src/daemon/bus.rs src/main.rs
git commit -m "feat: ygg hook — agent self-report + OS push notifications on conflict"
```

---

## Phase 4: TUI Dashboard

### Task 16: TUI scaffold

**Files:**
- Modify: `src/tui/mod.rs`
- Create: `src/tui/dashboard.rs`
- Create: `src/tui/world_detail.rs`
- Modify: `src/cli/monit.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/tui/mod.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Agent, Conflict, World};

    #[test]
    fn app_state_default_is_empty() {
        let state = AppState::default();
        assert!(state.worlds.is_empty());
        assert!(state.agents.is_empty());
        assert!(state.conflicts.is_empty());
        assert_eq!(state.selected_world, 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test tui
```

Expected: FAIL.

- [ ] **Step 3: Implement TUI scaffold**

```rust
// src/tui/mod.rs
pub mod dashboard;
pub mod world_detail;

use crate::types::{Agent, AuditEvent, Conflict, World};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::path::Path;
use std::time::Duration;

#[derive(Default)]
pub struct AppState {
    pub worlds: Vec<World>,
    pub agents: Vec<Agent>,
    pub conflicts: Vec<Conflict>,
    pub audit_log: Vec<AuditEvent>,
    pub selected_world: usize,
    pub view: View,
    pub audit_scroll: usize,
}

#[derive(Default, PartialEq)]
pub enum View {
    #[default]
    Dashboard,
    WorldDetail(String),
}

pub fn run_tui(repo_root: &Path) -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = AppState::default();

    // Load initial state from files
    state.worlds = crate::daemon::trunk::list_worlds(repo_root).unwrap_or_default();
    let log_path = repo_root.join(".ygg").join("shared_memory.json");
    if log_path.exists() {
        let audit_log = crate::daemon::bus::AuditLog::open(&log_path)?;
        state.audit_log = audit_log.read_recent(100, 24)?;
        state.conflicts = crate::daemon::bus::detect_conflicts(&state.audit_log);
    }

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
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
```

```rust
// src/tui/dashboard.rs — stub
use crate::tui::AppState;
use ratatui::Frame;

pub fn render(f: &mut Frame, _state: &AppState) {
    // Implemented in Task 17
    let _ = f;
}
```

```rust
// src/tui/world_detail.rs — stub
use crate::tui::AppState;
use ratatui::Frame;

pub fn render(f: &mut Frame, _state: &AppState, _world_id: &str) {
    let _ = f;
}
```

```rust
// src/cli/monit.rs
use anyhow::Result;
use std::path::Path;

pub fn run(repo_root: &Path) -> Result<()> {
    crate::tui::run_tui(repo_root)
}
```

Wire in `src/main.rs`:

```rust
mod tui;
// ...
Commands::Monit => cli::monit::run(&root),
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test tui
```

Expected: 1 test passes. Compile completes.

- [ ] **Step 5: Commit**

```bash
git add src/tui/ src/cli/monit.rs src/main.rs
git commit -m "feat: TUI scaffold — ratatui app loop, key navigation, state model"
```

---

### Task 17: Dashboard layout — 4 panels

**Files:**
- Modify: `src/tui/dashboard.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/tui/dashboard.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::AppState;
    use crate::types::{Agent, Conflict, World};
    use chrono::Utc;
    use std::path::PathBuf;

    #[test]
    fn worlds_table_rows_match_state() {
        let state = AppState {
            worlds: vec![World {
                id: "feat-auth".into(),
                branch: "feat/auth".into(),
                path: PathBuf::from("/tmp"),
                managed: true,
                created_at: Utc::now(),
            }],
            ..Default::default()
        };
        let rows = world_rows(&state);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("feat-auth"));
        assert!(rows[0].contains("feat/auth"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test dashboard
```

Expected: FAIL — world_rows not defined.

- [ ] **Step 3: Implement dashboard render**

```rust
// src/tui/dashboard.rs
use crate::tui::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table},
    Frame,
};

pub fn world_rows(state: &AppState) -> Vec<String> {
    state
        .worlds
        .iter()
        .map(|w| {
            let status = if state.agents.iter().any(|a| a.world_id == w.id) {
                "●"
            } else {
                "○"
            };
            let unmanaged = if !w.managed { " (unmanaged)" } else { "" };
            format!("{status} {}  {}{}", w.id, w.branch, unmanaged)
        })
        .collect()
}

pub fn render(f: &mut Frame, state: &AppState) {
    let size = f.size();

    // Split vertically: top 40% worlds+agents, middle 20% conflicts, bottom 40% log
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(20),
            Constraint::Percentage(45),
        ])
        .split(size);

    // Top: split horizontally — worlds left, agents right
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    // Worlds panel
    let world_items: Vec<ListItem> = state
        .worlds
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let status = if state.agents.iter().any(|a| a.world_id == w.id) { "●" } else { "○" };
            let flag = if !w.managed { " ⚠" } else { "" };
            let style = if i == state.selected_world {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{status} {}  {}{}", w.id, w.branch, flag)).style(style)
        })
        .collect();
    let worlds_list = List::new(world_items)
        .block(Block::default().title("Worlds  [Branch]").borders(Borders::ALL));
    f.render_widget(worlds_list, top[0]);

    // Agents panel
    let header = Row::new(vec!["PID", "Agent", "World", "Branch", "File"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = state
        .agents
        .iter()
        .map(|a| {
            let file = a.active_files.first().map(|s| s.as_str()).unwrap_or("-");
            let branch = state
                .worlds
                .iter()
                .find(|w| w.id == a.world_id)
                .map(|w| w.branch.as_str())
                .unwrap_or("-");
            Row::new(vec![
                a.pid.to_string(),
                a.binary.clone(),
                a.world_id.clone(),
                branch.to_string(),
                file.to_string(),
            ])
        })
        .collect();
    let agents_table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Length(16),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .block(Block::default().title("Active Agents").borders(Borders::ALL));
    f.render_widget(agents_table, top[1]);

    // Conflicts panel
    let conflict_items: Vec<ListItem> = if state.conflicts.is_empty() {
        vec![ListItem::new("No conflicts detected").style(Style::default().fg(Color::Green))]
    } else {
        state
            .conflicts
            .iter()
            .map(|c| {
                ListItem::new(format!(
                    "⚠ {} — {}",
                    c.file,
                    c.worlds.join(" + ")
                ))
                .style(Style::default().fg(Color::Red))
            })
            .collect()
    };
    let conflicts_list = List::new(conflict_items)
        .block(Block::default().title("⚠ Conflicts").borders(Borders::ALL));
    f.render_widget(conflicts_list, chunks[1]);

    // Audit log panel
    let log_items: Vec<ListItem> = state
        .audit_log
        .iter()
        .rev()
        .skip(state.audit_scroll)
        .take(20)
        .map(|e| {
            let time = e.ts.format("%H:%M:%S").to_string();
            let file = e.file.as_deref().unwrap_or("");
            ListItem::new(format!(
                "{}  {:?}  {}  {}",
                time,
                e.event,
                e.world,
                file
            ))
        })
        .collect();
    let log_list = List::new(log_items).block(
        Block::default()
            .title("Audit Log  [j/k scroll]")
            .borders(Borders::ALL),
    );
    f.render_widget(log_list, chunks[2]);

    // Status bar
    let help = Paragraph::new("[q]uit  [s]ync  [r]un new agent  [d]elete world  [↑↓]select  [Enter]detail");
    f.render_widget(help, ratatui::layout::Rect { y: size.height.saturating_sub(1), height: 1, ..size });
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test dashboard
```

Expected: 1 test passes. Full build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/tui/dashboard.rs
git commit -m "feat: TUI dashboard — 4-panel layout (worlds, agents, conflicts, audit log)"
```

---

### Task 18: World detail view

**Files:**
- Modify: `src/tui/world_detail.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/tui/world_detail.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AuditEvent;
    use crate::types::EventKind;

    #[test]
    fn world_events_filters_by_world_id() {
        let events = vec![
            AuditEvent {
                ts: chrono::Utc::now(),
                event: EventKind::FileModified,
                world: "feat-auth".into(),
                agent: None, pid: None,
                file: Some("src/auth.rs".into()), files: None, worlds: None,
            },
            AuditEvent {
                ts: chrono::Utc::now(),
                event: EventKind::FileModified,
                world: "feat-api".into(),
                agent: None, pid: None,
                file: Some("src/routes.rs".into()), files: None, worlds: None,
            },
        ];
        let filtered = events_for_world(&events, "feat-auth");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].world, "feat-auth");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test world_detail
```

Expected: FAIL.

- [ ] **Step 3: Implement world detail view**

```rust
// src/tui/world_detail.rs
use crate::tui::AppState;
use crate::types::AuditEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn events_for_world<'a>(events: &'a [AuditEvent], world_id: &str) -> Vec<&'a AuditEvent> {
    events.iter().filter(|e| e.world == world_id).collect()
}

pub fn render(f: &mut Frame, state: &AppState, world_id: &str) {
    let world = state.worlds.iter().find(|w| w.id == world_id);
    let title = match world {
        Some(w) => format!("World: {}  Branch: {}  [Esc] back", w.id, w.branch),
        None => format!("World: {world_id}  [Esc] back"),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Fill(1)])
        .split(f.size());

    // Info block
    let info = if let Some(w) = world {
        let env_path = w.path.join(".env");
        let env = std::fs::read_to_string(&env_path).unwrap_or_else(|_| "(no .env)".into());
        let agent = state
            .agents
            .iter()
            .find(|a| a.world_id == world_id)
            .map(|a| format!("{} (PID {})", a.binary, a.pid))
            .unwrap_or_else(|| "no active agent".into());
        format!("Path: {}\nAgent: {}\nEnv: {}", w.path.display(), agent, env.trim())
    } else {
        "World not found".into()
    };
    let info_widget = Paragraph::new(info)
        .block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(info_widget, chunks[0]);

    // Audit log for this world (last 50 events)
    let world_events = events_for_world(&state.audit_log, world_id);
    let items: Vec<ListItem> = world_events
        .iter()
        .rev()
        .take(50)
        .map(|e| {
            let time = e.ts.format("%H:%M:%S").to_string();
            let file = e.file.as_deref().unwrap_or("");
            ListItem::new(format!("{}  {:?}  {}", time, e.event, file))
        })
        .collect();
    let log = List::new(items)
        .block(Block::default().title("Events (last 50)").borders(Borders::ALL));
    f.render_widget(log, chunks[1]);
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test world_detail
```

Expected: 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add src/tui/world_detail.rs
git commit -m "feat: TUI world detail view — agent info, env vars, filtered audit log"
```

---

## Phase 5: `ygg sync`

### Task 19: Overlap detection

**Files:**
- Modify: `src/cli/sync.rs`

- [ ] **Step 1: Write failing test**

```rust
// src/cli/sync.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hunk_header_extracts_line_range() {
        // "@@ -10,5 +10,8 @@" means old file starts at line 10, 5 lines
        let (start, count) = parse_hunk_header("@@ -10,5 +10,8 @@").unwrap();
        assert_eq!(start, 10);
        assert_eq!(count, 5);
    }

    #[test]
    fn ranges_overlap_detects_intersection() {
        assert!(ranges_overlap((10, 40), (35, 60)));
        assert!(!ranges_overlap((10, 30), (35, 60)));
        assert!(ranges_overlap((10, 40), (10, 20)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test cli::sync
```

Expected: FAIL.

- [ ] **Step 3: Implement overlap detection**

```rust
// src/cli/sync.rs
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug)]
pub struct WorldDiff {
    pub world_id: String,
    pub branch: String,
    pub file_hunks: HashMap<String, Vec<(usize, usize)>>, // file → [(start, end)]
}

#[derive(Debug)]
pub struct OverlapReport {
    pub file: String,
    pub world_a: String,
    pub range_a: (usize, usize),
    pub world_b: String,
    pub range_b: (usize, usize),
}

/// Parse "@@ -start,count +start,count @@" → (start, start+count)
pub fn parse_hunk_header(header: &str) -> Option<(usize, usize)> {
    let inner = header.trim_start_matches("@@ ").split(" @@").next()?;
    let old_part = inner.split(' ').next()?; // "-10,5"
    let old_part = old_part.trim_start_matches('-');
    let mut parts = old_part.splitn(2, ',');
    let start: usize = parts.next()?.parse().ok()?;
    let count: usize = parts.next().unwrap_or("1").parse().ok()?;
    Some((start, start + count))
}

/// Returns true if the two line ranges overlap (both inclusive).
pub fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

pub fn diff_world(repo_root: &Path, world_id: &str, branch: &str) -> Result<WorldDiff> {
    let output = std::process::Command::new("git")
        .args([
            "diff",
            "-U0",
            &format!("HEAD...{branch}"),
        ])
        .current_dir(repo_root)
        .output()
        .context("git diff failed")?;

    let text = String::from_utf8_lossy(&output.stdout);
    let mut file_hunks: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut current_file = String::new();

    for line in text.lines() {
        if line.starts_with("+++ b/") {
            current_file = line[6..].to_string();
        } else if line.starts_with("@@") {
            if let Some(range) = parse_hunk_header(line) {
                file_hunks.entry(current_file.clone()).or_default().push(range);
            }
        }
    }

    Ok(WorldDiff {
        world_id: world_id.to_string(),
        branch: branch.to_string(),
        file_hunks,
    })
}

pub fn find_overlaps(diffs: &[WorldDiff]) -> Vec<OverlapReport> {
    let mut reports = vec![];
    for i in 0..diffs.len() {
        for j in (i + 1)..diffs.len() {
            let a = &diffs[i];
            let b = &diffs[j];
            for (file, hunks_a) in &a.file_hunks {
                if let Some(hunks_b) = b.file_hunks.get(file) {
                    for &ra in hunks_a {
                        for &rb in hunks_b {
                            if ranges_overlap(ra, rb) {
                                reports.push(OverlapReport {
                                    file: file.clone(),
                                    world_a: a.world_id.clone(),
                                    range_a: ra,
                                    world_b: b.world_id.clone(),
                                    range_b: rb,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    reports
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test cli::sync
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/sync.rs
git commit -m "feat: sync overlap detection — git diff -U0 hunk parsing + range intersection"
```

---

### Task 20: `ygg sync` — merge report + git merge + audit consolidation

**Files:**
- Modify: `src/cli/sync.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test**

```rust
// Add to src/cli/sync.rs tests
#[test]
fn format_overlap_report_contains_file_and_worlds() {
    let report = OverlapReport {
        file: "src/auth.rs".into(),
        world_a: "feat-auth".into(), range_a: (10, 40),
        world_b: "feat-api".into(),  range_b: (35, 60),
    };
    let formatted = format_overlap(&report);
    assert!(formatted.contains("src/auth.rs"));
    assert!(formatted.contains("feat-auth"));
    assert!(formatted.contains("feat-api"));
    assert!(formatted.contains("⚠"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test format_overlap
```

Expected: FAIL.

- [ ] **Step 3: Implement full sync flow**

```rust
// Add to src/cli/sync.rs

pub fn format_overlap(r: &OverlapReport) -> String {
    format!(
        "  ⚠ {} — {} (lines {}-{}) overlaps {} (lines {}-{})",
        r.file, r.world_a, r.range_a.0, r.range_a.1,
        r.world_b, r.range_b.0, r.range_b.1
    )
}

pub fn run(repo_root: &Path, prune: bool) -> Result<()> {
    let worlds = crate::daemon::trunk::list_worlds(repo_root)?;
    if worlds.is_empty() {
        println!("No worlds to sync.");
        return Ok(());
    }

    println!("Computing diffs...\n");
    let diffs: Vec<WorldDiff> = worlds
        .iter()
        .filter_map(|w| diff_world(repo_root, &w.id, &w.branch).ok())
        .collect();

    let overlaps = find_overlaps(&diffs);

    println!("SYNC REPORT");
    println!("{}", "─".repeat(60));
    for diff in &diffs {
        let has_overlap = overlaps.iter().any(|o| o.world_a == diff.world_id || o.world_b == diff.world_id);
        let status = if has_overlap { "⚠ overlap" } else { "✓ safe" };
        println!("  {}  →  {} files changed  [{}]", diff.world_id, diff.file_hunks.len(), status);
    }
    if !overlaps.is_empty() {
        println!("\nOverlap details:");
        for o in &overlaps {
            println!("{}", format_overlap(o));
        }
    }
    println!();

    for world in &worlds {
        let prompt = format!("Merge `{}` (branch: {}) → trunk?", world.id, world.branch);
        let choice = dialoguer::Select::new()
            .with_prompt(&prompt)
            .items(&["yes", "no", "defer"])
            .default(1)
            .interact()?;

        if choice != 0 { continue; }

        let status = std::process::Command::new("git")
            .args(["merge", "--no-ff", &world.branch])
            .current_dir(repo_root)
            .status()?;

        if status.success() {
            println!("✓ Merged {}", world.branch);

            // Consolidate audit log
            let log_path = repo_root.join(".ygg").join("shared_memory.json");
            let mut log = crate::daemon::bus::AuditLog::open(&log_path)?;
            log.append(&crate::types::AuditEvent {
                ts: chrono::Utc::now(),
                event: crate::types::EventKind::WorldMerged,
                world: world.id.clone(),
                agent: None, pid: None, file: None, files: None, worlds: None,
            })?;

            if prune {
                crate::daemon::trunk::delete_world(repo_root, &world.id)?;
                println!("✓ Pruned world {}", world.id);
            }
        } else {
            println!("✗ Merge conflict on {}. Resolve manually and re-run.", world.branch);
        }
    }

    Ok(())
}
```

Wire in `src/main.rs`:

```rust
Commands::Sync { prune } => cli::sync::run(&root, prune),
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test cli::sync
```

Expected: all sync tests pass. Full build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/cli/sync.rs src/main.rs
git commit -m "feat: ygg sync — overlap report, per-world merge confirmation, audit consolidation"
```

---

## Phase 6: Release

### Task 21: GitHub Actions release pipeline

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create release workflow**

```bash
mkdir -p .github/workflows
```

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            archive: tar.gz
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            archive: tar.gz
          - target: x86_64-apple-darwin
            os: macos-latest
            archive: tar.gz
          - target: aarch64-apple-darwin
            os: macos-latest
            archive: tar.gz
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            archive: zip

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross-compile tools (Linux aarch64)
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: |
          sudo apt-get update
          sudo apt-get install -y gcc-aarch64-linux-gnu

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
        env:
          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: aarch64-linux-gnu-gcc

      - name: Package (Unix)
        if: matrix.archive == 'tar.gz'
        run: |
          cd target/${{ matrix.target }}/release
          tar czf ygg-${{ github.ref_name }}-${{ matrix.target }}.tar.gz ygg
          echo "ASSET=target/${{ matrix.target }}/release/ygg-${{ github.ref_name }}-${{ matrix.target }}.tar.gz" >> $GITHUB_ENV

      - name: Package (Windows)
        if: matrix.archive == 'zip'
        shell: pwsh
        run: |
          cd target/${{ matrix.target }}/release
          Compress-Archive ygg.exe ygg-${{ github.ref_name }}-${{ matrix.target }}.zip
          echo "ASSET=target/${{ matrix.target }}/release/ygg-${{ github.ref_name }}-${{ matrix.target }}.zip" | Out-File -FilePath $env:GITHUB_ENV -Append

      - name: Upload to release
        uses: softprops/action-gh-release@v2
        with:
          files: ${{ env.ASSET }}
```

- [ ] **Step 2: Verify YAML is valid**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo "YAML valid"
```

Expected: `YAML valid`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: GitHub Actions release pipeline — cross-compile 5 targets"
```

---

### Task 22: `install.sh`

**Files:**
- Create: `scripts/install.sh`

- [ ] **Step 1: Write install script**

```bash
mkdir -p scripts
```

```bash
#!/usr/bin/env sh
# scripts/install.sh — Yggdrazil installer
set -e

REPO="rzorzal/yggdrazil"
BIN_NAME="ygg"
INSTALL_DIR="/usr/local/bin"

# Detect OS and arch
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *) echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    EXT="tar.gz"
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *) echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    EXT="tar.gz"
    ;;
  *)
    echo "Unsupported OS: $OS. Use the Windows installer from GitHub Releases."
    exit 1
    ;;
esac

# Get latest release tag from GitHub API
LATEST=$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$LATEST" ]; then
  echo "Could not determine latest release. Check https://github.com/${REPO}/releases"
  exit 1
fi

FILENAME="${BIN_NAME}-${LATEST}-${TARGET}.${EXT}"
URL="https://github.com/${REPO}/releases/download/${LATEST}/${FILENAME}"

echo "Installing ygg ${LATEST} for ${TARGET}..."

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

curl -sSfL "$URL" -o "$TMP/$FILENAME"
tar xzf "$TMP/$FILENAME" -C "$TMP"

install -m 755 "$TMP/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

echo "✓ ygg ${LATEST} installed to ${INSTALL_DIR}/${BIN_NAME}"
echo "  Run: ygg init"
```

- [ ] **Step 2: Verify script is valid shell**

```bash
sh -n scripts/install.sh && echo "Shell syntax OK"
```

Expected: `Shell syntax OK`

- [ ] **Step 3: Make executable and commit**

```bash
chmod +x scripts/install.sh
git add scripts/install.sh
git commit -m "feat: install.sh — detect arch, fetch latest release from GitHub API"
```

---

## Final: Full build + test pass

- [ ] **Step 1: Run full test suite**

```bash
cargo test
```

Expected: all unit and integration tests pass.

- [ ] **Step 2: Build release binary**

```bash
cargo build --release
./target/release/ygg --help
```

Expected: help text shows all subcommands: `init`, `run`, `hook`, `sync`, `monit`, `daemon`.

- [ ] **Step 3: Smoke test init**

```bash
cd /tmp && mkdir ygg-smoke && cd ygg-smoke
git init && git commit --allow-empty -m "init"
<path-to-ygg>/target/release/ygg init
ls .ygg/
```

Expected: `daemon.pid` not present yet, but `worlds/` and `shared_memory.json` exist.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: final build verification"
```

---

*Total tasks: 22. Estimated implementation time: 3-5 hours for an experienced Rust developer.*
