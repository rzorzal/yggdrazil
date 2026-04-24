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

const AUDIT_CAP: usize = 500;

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

#[derive(Default, PartialEq)]
pub enum View {
    #[default]
    Dashboard,
    WorldDetail(String),
}

pub fn apply_ipc_msg(state: &mut AppState, msg: crate::types::IpcMessage) {
    use crate::types::IpcMessage;
    match msg {
        IpcMessage::StateSnapshot { worlds, agents, conflicts } => {
            state.worlds = worlds;
            state.agents = agents;
            state.conflicts = conflicts;
        }
        IpcMessage::EventNotification { event } => {
            state.audit_log.push(event);
            if state.audit_log.len() > AUDIT_CAP {
                state.audit_log.drain(..state.audit_log.len() - AUDIT_CAP);
            }
            state.conflicts = crate::daemon::bus::detect_conflicts(&state.audit_log);
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

fn spawn_ipc_thread(
    repo_root: std::path::PathBuf,
    evt_tx: std::sync::mpsc::Sender<crate::types::IpcMessage>,
    cmd_rx: std::sync::mpsc::Receiver<crate::types::IpcMessage>,
) {
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Runtime::new() else { return };
        rt.block_on(async move {
            let sock = crate::ipc::socket_path(&repo_root);
            let Ok(mut client) = crate::ipc::client::IpcClient::connect(&sock).await else {
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
                // Receive one incoming message with 100ms timeout
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

    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<crate::types::IpcMessage>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<crate::types::IpcMessage>();
    let socket_path = crate::ipc::socket_path(repo_root);
    if socket_path.exists() {
        spawn_ipc_thread(repo_root.to_path_buf(), evt_tx, cmd_rx);
        state.ipc_tx = Some(cmd_tx);
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

        // Drain IPC messages into state
        while let Ok(msg) = evt_rx.try_recv() {
            apply_ipc_msg(&mut state, msg);
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_default_is_empty() {
        let state = AppState::default();
        assert!(state.worlds.is_empty());
        assert!(state.agents.is_empty());
        assert!(state.conflicts.is_empty());
        assert_eq!(state.selected_world, 0);
    }

    #[test]
    fn confirm_delete_starts_none() {
        let state = AppState::default();
        assert!(state.confirm_delete.is_none());
        assert!(state.ipc_tx.is_none());
        assert!(state.status_msg.is_none());
    }

    #[test]
    fn apply_ipc_msg_state_snapshot_replaces_state() {
        use crate::types::{IpcMessage, World};
        use std::path::PathBuf;

        let world = World {
            id: "feat-auth".into(),
            branch: "feat/auth".into(),
            path: PathBuf::from("/tmp"),
            managed: true,
            created_at: chrono::Utc::now(),
        };
        let mut state = AppState::default();
        apply_ipc_msg(&mut state, IpcMessage::StateSnapshot {
            worlds: vec![world.clone()],
            agents: vec![],
            conflicts: vec![],
        });
        assert_eq!(state.worlds.len(), 1);
        assert_eq!(state.worlds[0].id, "feat-auth");
        assert!(state.agents.is_empty());
    }

    #[test]
    fn apply_ipc_msg_world_deleted_removes_world_and_clears_confirm() {
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

    #[test]
    fn apply_ipc_msg_event_notification_appends_to_log() {
        use crate::types::{AuditEvent, EventKind, IpcMessage};

        let mut state = AppState::default();
        let event = AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: None, pid: None,
            file: Some("src/lib.rs".into()),
            files: None, worlds: None,
        };
        apply_ipc_msg(&mut state, IpcMessage::EventNotification { event });
        assert_eq!(state.audit_log.len(), 1);
    }

    #[test]
    fn ipc_events_appended_to_audit_log() {
        use crate::types::{AuditEvent, EventKind, IpcMessage};

        let (tx, rx) = std::sync::mpsc::channel::<IpcMessage>();

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

        tx.send(IpcMessage::EventNotification { event: event1 }).unwrap();
        tx.send(IpcMessage::EventNotification { event: event2 }).unwrap();
        drop(tx);

        let mut state = AppState::default();
        while let Ok(msg) = rx.try_recv() {
            apply_ipc_msg(&mut state, msg);
        }

        assert_eq!(state.audit_log.len(), 2);
        // Same file touched by two different worlds → conflict
        assert_eq!(state.conflicts.len(), 1);
        assert_eq!(state.conflicts[0].file, "src/auth.rs");
    }
}
