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
