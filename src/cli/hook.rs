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

fn update_shared_memory(repo_root: &Path, world_id: &str, files: &[&str]) -> Result<()> {
    let sm_path = repo_root.join(".ygg").join("shared_memory.json");
    let existing = if sm_path.exists() {
        std::fs::read_to_string(&sm_path).unwrap_or_default()
    } else {
        String::new()
    };
    let mut state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&existing).unwrap_or_default();

    state.insert(
        world_id.to_string(),
        serde_json::json!({
            "files": files,
            "ts": chrono::Utc::now().to_rfc3339(),
        }),
    );

    std::fs::write(
        &sm_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(state))?,
    )?;
    Ok(())
}

pub fn run(repo_root: &Path, world_id: &str, files: &[String]) -> Result<()> {
    let world_path = crate::ipc::worlds_dir(repo_root).join(world_id);
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

    write_agent_state(&world_path, world_id, &file_refs)?;
    update_shared_memory(repo_root, world_id, &file_refs)?;

    // Best-effort IPC to daemon for live audit events
    let sock = crate::ipc::socket_path(repo_root);
    if sock.exists() {
        if let Ok(rt) = tokio::runtime::Runtime::new() {
            let _ = rt.block_on(async {
                if let Ok(mut client) = crate::ipc::client::IpcClient::connect(&sock).await {
                    let _ = client
                        .send(&crate::types::IpcMessage::HookReport {
                            world: world_id.to_string(),
                            files: files.to_vec(),
                        })
                        .await;
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

        let content = std::fs::read_to_string(dir.path().join(".agent_state")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["world"], "feat-auth");
        assert!(parsed["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "src/auth.rs"));
        assert!(parsed["ts"].as_str().is_some(), "ts field must be present");
    }

    #[test]
    fn hook_writes_shared_memory_without_daemon() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg/worlds/feat-auth")).unwrap();
        std::fs::write(dir.path().join(".ygg/shared_memory.json"), "{}").unwrap();

        run(dir.path(), "feat-auth", &["src/auth.rs".to_string()]).unwrap();

        let sm = std::fs::read_to_string(dir.path().join(".ygg/shared_memory.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sm).unwrap();
        assert_eq!(parsed["feat-auth"]["files"][0], "src/auth.rs");
        assert!(parsed["feat-auth"]["ts"].is_string());
    }

    #[test]
    fn hook_shared_memory_merges_multiple_worlds() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg/worlds/world-a")).unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg/worlds/world-b")).unwrap();
        std::fs::write(dir.path().join(".ygg/shared_memory.json"), "{}").unwrap();

        run(dir.path(), "world-a", &["src/a.rs".to_string()]).unwrap();
        run(dir.path(), "world-b", &["src/b.rs".to_string()]).unwrap();

        let sm = std::fs::read_to_string(dir.path().join(".ygg/shared_memory.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&sm).unwrap();
        assert_eq!(parsed["world-a"]["files"][0], "src/a.rs");
        assert_eq!(parsed["world-b"]["files"][0], "src/b.rs");
    }
}
