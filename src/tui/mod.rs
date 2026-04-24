pub mod dashboard;
pub mod world_detail;

use crate::ipc::client::IpcClient;
use crate::types::{Agent, AuditEvent, Conflict, IpcMessage, World};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::path::Path;
use std::sync::mpsc;
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
    pub world_env: Option<String>,
}

#[derive(Default, PartialEq)]
pub enum View {
    #[default]
    Dashboard,
    WorldDetail(String),
}

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
                    Err(_) => break, // daemon disconnected or EOF
                    _ => {}
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

    let (ipc_tx, ipc_rx) = mpsc::channel::<crate::types::AuditEvent>();
    spawn_ipc_listener(repo_root.to_path_buf(), ipc_tx);

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
        const AUDIT_CAP: usize = 500;
        let mut got_new = false;
        while let Ok(event) = ipc_rx.try_recv() {
            state.audit_log.push(event);
            got_new = true;
        }
        if state.audit_log.len() > AUDIT_CAP {
            state.audit_log.drain(..state.audit_log.len() - AUDIT_CAP);
        }
        if got_new {
            state.conflicts = crate::daemon::bus::detect_conflicts(&state.audit_log);
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

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
}
