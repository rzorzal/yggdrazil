use crate::tui::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Row, Table},
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(20),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(size);

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
            let files = state.agent_states.get(&w.id).cloned().unwrap_or_default();
            let file_hint = if files.is_empty() {
                String::new()
            } else {
                format!("  [{}]", files.join(", "))
            };
            let style = if i == state.selected_world {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{status} {}  {}{}{}", w.id, w.branch, flag, file_hint))
                .style(style)
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
            Constraint::Min(0),
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
                ListItem::new(format!("⚠ {} — {}", c.file, c.worlds.join(" + ")))
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
            ListItem::new(format!("{}  {:?}  {}  {}", time, e.event, e.world, file))
        })
        .collect();
    let log_list = List::new(log_items).block(
        Block::default()
            .title("Audit Log  [j/k scroll]")
            .borders(Borders::ALL),
    );
    f.render_widget(log_list, chunks[2]);

    // Status bar — show transient message or default help
    let help_text = if let Some(ref msg) = state.status_msg {
        format!("  {}", msg)
    } else {
        "[q]uit  [d]elete world  [↑↓]select  [Enter]detail  [j/k]scroll log".into()
    };
    let help = Paragraph::new(help_text);
    f.render_widget(help, chunks[3]);

    // Confirmation overlay — rendered last so it appears on top
    if let Some(ref world_id) = state.confirm_delete {
        let popup_area = centered_rect(52, 4, size);
        f.render_widget(Clear, popup_area);
        let text = format!("Delete \"{}\" + kill agent?\n\n[y] confirm        [n] cancel", world_id);
        let popup = Paragraph::new(text)
            .block(Block::default().title(" Confirm Delete ").borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(popup, popup_area);
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::AppState;
    use crate::types::World;
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

    #[test]
    fn centered_rect_is_inside_parent() {
        use ratatui::layout::Rect;
        let area = Rect::new(0, 0, 80, 24);
        let popup = centered_rect(50, 4, area);
        assert!(popup.x >= area.x);
        assert!(popup.y >= area.y);
        assert!(popup.x + popup.width <= area.x + area.width);
        assert!(popup.y + popup.height <= area.y + area.height);
        assert_eq!(popup.height, 4);
    }
}
