// src/ipc/client.rs — full IPC client implementation
use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{tokio::prelude::*, ConnectOptions, GenericFilePath};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct IpcClient {
    stream: interprocess::local_socket::tokio::Stream,
}

impl IpcClient {
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let name = socket_path
            .to_str()
            .unwrap()
            .to_fs_name::<GenericFilePath>()?;
        let stream = ConnectOptions::new().name(name).connect_tokio().await?;
        Ok(Self { stream })
    }

    pub async fn send(&mut self, msg: &IpcMessage) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        self.stream.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<IpcMessage> {
        let mut reader = BufReader::new(&mut self.stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let msg = serde_json::from_str(line.trim())?;
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IpcMessage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn client_sends_hook_report() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let mut server = crate::ipc::server::IpcServer::new(&sock).await.unwrap();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::channel(1);

        tokio::spawn(async move {
            server.accept_loop(move |msg| {
                let tx = result_tx.clone();
                async move { let _ = tx.send(msg).await; }
            }).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = IpcClient::connect(&sock).await.unwrap();
        client.send(&IpcMessage::HookReport {
            world: "feat-auth".into(),
            files: vec!["src/auth.rs".into()],
        }).await.unwrap();

        let received = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            result_rx.recv(),
        ).await.unwrap().unwrap();

        assert!(matches!(received, IpcMessage::HookReport { .. }));
    }
}
