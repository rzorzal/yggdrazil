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
