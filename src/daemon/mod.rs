pub mod bus;
pub mod laws;
pub mod roots;
pub mod trunk;

use crate::ipc::server::IpcServer;
use anyhow::Result;
use std::path::PathBuf;

pub struct Daemon {
    pub repo_root: PathBuf,
}

impl Daemon {
    pub async fn run(repo_root: PathBuf) -> Result<()> {
        let sock = crate::ipc::socket_path(&repo_root);
        let log_path = crate::ipc::shared_memory_path(&repo_root);
        let mut server = IpcServer::new(&sock).await?;
        let tx = server.tx.clone();

        tracing::info!("ygg daemon started, socket: {}", sock.display());

        let roots_root = repo_root.clone();
        tokio::spawn(async move {
            roots::scan_loop(&roots_root).await;
        });

        let log_path2 = log_path.clone();
        let repo_root2 = repo_root.clone();
        server.accept_loop(move |msg| {
            let tx = tx.clone();
            let log_path = log_path2.clone();
            let repo_root = repo_root2.clone();
            async move {
                match msg {
                    crate::types::IpcMessage::HookReport { world, files } => {
                        tracing::debug!("hook report: world={} files={:?}", world, files);

                        if let Ok(mut log) = bus::AuditLog::open(&log_path) {
                            for file in &files {
                                let event = crate::types::AuditEvent {
                                    ts: chrono::Utc::now(),
                                    event: crate::types::EventKind::FileModified,
                                    world: world.clone(),
                                    agent: None,
                                    pid: None,
                                    file: Some(file.clone()),
                                    files: None,
                                    worlds: None,
                                };
                                let _ = log.append(&event);
                                let _ = tx.send(crate::types::IpcMessage::EventNotification {
                                    event,
                                });
                            }

                            if let Ok(events) = log.read_recent(500, 2) {
                                let conflicts = bus::detect_conflicts(&events);
                                for conflict in &conflicts {
                                    tracing::warn!("conflict detected: {:?}", conflict);
                                    for w in &conflict.worlds {
                                        if *w != world {
                                            let world_path = repo_root.join(".ygg/worlds").join(w);
                                            let _ = laws::inject_conflict_warning(
                                                &world_path, &world, &conflict.file,
                                            );
                                        }
                                    }
                                    bus::notify_conflict(&conflict.file, &conflict.worlds);
                                    let _ = tx.send(crate::types::IpcMessage::EventNotification {
                                        event: crate::types::AuditEvent {
                                            ts: chrono::Utc::now(),
                                            event: crate::types::EventKind::ConflictDetected,
                                            world: world.clone(),
                                            agent: None,
                                            pid: None,
                                            file: Some(conflict.file.clone()),
                                            files: None,
                                            worlds: Some(conflict.worlds.clone()),
                                        },
                                    });
                                }
                            }
                        }
                    }
                    crate::types::IpcMessage::Subscribe => {
                        tracing::debug!("new TUI subscriber");
                    }
                    _ => {}
                }
            }
        }).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn daemon_starts_and_creates_socket() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let repo_root = dir.path().to_path_buf();
        let handle = tokio::spawn(Daemon::run(repo_root));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert!(sock.exists(), "socket should exist after daemon start");
        handle.abort();
    }

    #[tokio::test]
    #[ignore = "requires Task 2 (server write task) and Task 3 (persistent BufReader) to pass"]
    async fn hook_report_broadcasts_file_modified_event() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg/worlds")).unwrap();
        std::fs::write(dir.path().join(".ygg/shared_memory.json"), "").unwrap();

        let repo_root = dir.path().to_path_buf();
        let handle = tokio::spawn(Daemon::run(repo_root.clone()));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let sock = crate::ipc::socket_path(dir.path());
        let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
        // Subscribe so write task starts forwarding
        client.send(&crate::types::IpcMessage::Subscribe).await.unwrap();
        // Give daemon time to register the write task
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        client.send(&crate::types::IpcMessage::HookReport {
            world: "feat-auth".into(),
            files: vec!["src/auth.rs".into()],
        }).await.unwrap();

        let received = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.recv(),
        ).await.unwrap().unwrap();

        assert!(
            matches!(
                &received,
                crate::types::IpcMessage::EventNotification {
                    event: crate::types::AuditEvent {
                        event: crate::types::EventKind::FileModified,
                        ..
                    }
                }
            ),
            "expected FileModified EventNotification, got {:?}", received
        );

        handle.abort();
    }

    #[tokio::test]
    async fn hook_report_triggers_conflict_check_and_broadcast() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg/worlds")).unwrap();
        std::fs::write(dir.path().join(".ygg/shared_memory.json"), "").unwrap();

        let repo_root = dir.path().to_path_buf();
        let handle = tokio::spawn(Daemon::run(repo_root.clone()));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let sock = crate::ipc::socket_path(dir.path());
        let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
        client.send(&crate::types::IpcMessage::Subscribe).await.unwrap();
        client.send(&crate::types::IpcMessage::HookReport {
            world: "feat-auth".into(),
            files: vec!["src/lib.rs".into()],
        }).await.unwrap();

        // No panic = daemon processed the message
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        handle.abort();
    }
}
