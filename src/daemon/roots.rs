use sysinfo::System;

use crate::daemon::{laws, trunk};
use crate::types::Agent;
use chrono::Utc;

const AGENT_BINARIES: &[&str] = &["claude", "claude-code", "codex", "aider", "cursor"];

pub fn classify_binary(name: &str) -> Option<&'static str> {
    AGENT_BINARIES.iter().find(|&&b| name == b).copied()
}

/// Scan all processes, return AI agents whose CWD is inside repo_root.
/// `managed` field is true if CWD is inside worlds_dir.
pub fn scan_once(repo_root: &str, worlds_dir: &str) -> Vec<Agent> {
    let mut sys = System::new_all();
    sys.refresh_processes();

    sys.processes()
        .values()
        .filter_map(|proc| {
            let name = proc.name();
            let binary = classify_binary(name)?;
            let cwd = proc.cwd()?;
            let cwd_str = cwd.to_str()?;

            if cwd_str.starts_with(worlds_dir) {
                // Managed — extract world_id from path
                let rel = cwd_str.strip_prefix(worlds_dir)?.trim_start_matches('/');
                let world_id = rel.split('/').next()?.to_string();
                if world_id.is_empty() { return None; }
                Some(Agent {
                    pid: proc.pid().as_u32(),
                    binary: binary.to_string(),
                    world_id,
                    active_files: vec![],
                })
            } else if cwd_str.starts_with(repo_root) {
                // Unmanaged — world_id is empty (not yet created)
                Some(Agent {
                    pid: proc.pid().as_u32(),
                    binary: binary.to_string(),
                    world_id: String::new(),
                    active_files: vec![],
                })
            } else {
                None
            }
        })
        .collect()
}

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
            let Some(cwd) = proc.cwd() else { continue };
            let cwd_str = cwd.to_string_lossy();
            let pid = proc.pid().as_u32();

            if known_pids.contains(&pid) { continue; }
            known_pids.insert(pid);

            if cwd_str.starts_with(&worlds_dir_str) {
                // Managed world — just log
                tracing::info!("managed agent: {} PID {} in {}", binary, pid, cwd_str);
            } else if cwd_str.starts_with(&repo_str) {
                // Unmanaged — auto-create world
                let world_id = world_id_for_unmanaged_cwd(repo_root, cwd);
                tracing::warn!("unmanaged agent detected: {} PID {}, creating world {}", binary, pid, world_id);
                if let Ok(world) = trunk::create_world(repo_root, &world_id, "HEAD") {
                    let _ = laws::inject_rules(&world.path, &world_id, "HEAD", &[]);
                }
            }
        }

        // Detect exited agents
        let current_pids: std::collections::HashSet<u32> = sys.processes().keys()
            .map(|p| p.as_u32()).collect();
        known_pids.retain(|pid| {
            if current_pids.contains(pid) {
                true
            } else {
                tracing::info!("agent exited: PID {}", pid);
                false
            }
        });

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
        let agents = scan_once("/nonexistent/repo", "/nonexistent/worlds");
        let _ = agents;
    }

    #[test]
    fn unmanaged_world_id_starts_with_prefix() {
        use std::path::Path;
        let id = world_id_for_unmanaged_cwd(Path::new("/repo"), Path::new("/repo"));
        assert!(id.starts_with("unmanaged-"), "got: {id}");
    }
}
