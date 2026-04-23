use crate::tui::AppState;
use crate::types::AuditEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout},
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
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(f.size());

    let info = if let Some(w) = world {
        let env = state.world_env.as_deref().unwrap_or("(no .env)");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuditEvent, EventKind};

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
