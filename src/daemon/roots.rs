use sysinfo::System;

use crate::daemon::{laws, trunk};
use crate::types::Agent;

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

pub fn world_id_for_unmanaged_cwd(_repo_root: &std::path::Path, cwd: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cwd.hash(&mut h);
    format!("unmanaged-{:x}", h.finish())
}

const POLL_INTERVAL_SECS: u64 = 30;

pub async fn scan_loop(repo_root: &std::path::Path) {
    let worlds_dir = repo_root.join(".ygg").join("worlds");
    let worlds_dir_str = worlds_dir.to_string_lossy().to_string();
    let repo_str = repo_root.to_string_lossy().to_string();
    let mut known_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    loop {
        // Get current PIDs from OS (for exit detection)
        let mut sys = System::new_all();
        sys.refresh_processes();
        let current_pids: std::collections::HashSet<u32> =
            sys.processes().keys().map(|p| p.as_u32()).collect();

        // Use scan_once to get all repo agents (managed + unmanaged)
        for agent in scan_once(&repo_str, &worlds_dir_str) {
            let pid = agent.pid;
            if known_pids.contains(&pid) {
                continue;
            }
            known_pids.insert(pid);

            if agent.world_id.is_empty() {
                // Unmanaged agent
                let cwd = sys.processes().values()
                    .find(|p| p.pid().as_u32() == pid)
                    .and_then(|p| p.cwd())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| repo_root.to_path_buf());
                let world_id = world_id_for_unmanaged_cwd(repo_root, &cwd);
                tracing::warn!(
                    "unmanaged agent detected: {} PID {}, creating world {}",
                    agent.binary, pid, world_id
                );
                // Resolve HEAD to actual branch name
                let branch = {
                    std::process::Command::new("git")
                        .args(["rev-parse", "--abbrev-ref", "HEAD"])
                        .current_dir(repo_root)
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "HEAD".to_string())
                };
                if let Ok(world) = trunk::create_world(repo_root, &world_id, &branch) {
                    let _ = laws::inject_rules(&world.path, &world_id, &branch, &[]);
                }
            } else {
                tracing::info!(
                    "managed agent: {} PID {} in world {}",
                    agent.binary, pid, agent.world_id
                );
            }
        }

        // Detect exited agents
        known_pids.retain(|pid| {
            if current_pids.contains(pid) {
                true
            } else {
                tracing::info!("agent exited: PID {}", pid);
                false
            }
        });

        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
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
    fn scan_once_does_not_panic() {
        let agents = scan_once("/nonexistent/repo", "/nonexistent/worlds");
        let _ = agents;
    }

    #[test]
    fn unmanaged_world_id_is_stable_for_same_cwd() {
        use std::path::Path;
        let id1 = world_id_for_unmanaged_cwd(Path::new("/repo"), Path::new("/repo/subdir"));
        let id2 = world_id_for_unmanaged_cwd(Path::new("/repo"), Path::new("/repo/subdir"));
        assert!(id1.starts_with("unmanaged-"), "got: {id1}");
        assert_eq!(id1, id2, "same CWD must produce same world id");
    }
}
