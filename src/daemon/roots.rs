use std::path::Path;
use sysinfo::System;

use crate::types::Agent;

const AGENT_BINARIES: &[&str] = &["claude", "claude-code", "codex", "aider", "cursor"];

pub fn classify_binary(name: &str) -> Option<&'static str> {
    AGENT_BINARIES.iter().find(|&&b| name == b).copied()
}

/// Scan all processes, return AI agents whose CWD is inside worlds_dir.
pub fn scan_once(worlds_dir: &str) -> Vec<Agent> {
    let mut sys = System::new_all();
    sys.refresh_processes();

    sys.processes()
        .values()
        .filter_map(|proc| {
            let name = proc.name();
            let binary = classify_binary(name)?;
            let cwd = proc.cwd()?;
            let cwd_str = cwd.to_str()?;
            if !cwd_str.starts_with(worlds_dir) {
                return None;
            }
            // Extract world id: .ygg/worlds/<id>/...
            let rel = cwd_str.strip_prefix(worlds_dir)?.trim_start_matches('/');
            let world_id = rel.split('/').next()?.to_string();
            if world_id.is_empty() {
                return None;
            }
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
                tracing::info!(
                    "agent spawned: {} PID {} in {}",
                    agent.binary, agent.pid, agent.world_id
                );
            }
        }
        for pid in &known_pids {
            if !current_pids.contains(pid) {
                tracing::info!("agent exited: PID {}", pid);
            }
        }
        known_pids = current_pids;

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_binary_matches_known_agents() {
        assert_eq!(classify_binary("claude"), Some("claude"));
        assert_eq!(classify_binary("claude-code"), Some("claude-code"));
        assert_eq!(classify_binary("aider"), Some("aider"));
        assert_eq!(classify_binary("codex"), Some("codex"));
        assert_eq!(classify_binary("cursor"), Some("cursor"));
        assert_eq!(classify_binary("bash"), None);
        assert_eq!(classify_binary("node"), None);
        assert_eq!(classify_binary("python"), None);
    }

    #[test]
    fn scan_once_returns_vec() {
        // Just verify it runs without panic and returns a Vec
        let agents = scan_once("/nonexistent/worlds");
        let _ = agents; // May be empty — that's fine
    }
}
