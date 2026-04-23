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
        let mut server = IpcServer::new(&sock).await?;

        tracing::info!("ygg daemon started, socket: {}", sock.display());

        let roots_root = repo_root.clone();
        tokio::spawn(async move {
            roots::scan_loop(&roots_root).await;
        });

        server.accept_loop(|msg| async move {
            tracing::debug!("received IPC message: {:?}", msg);
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
}
