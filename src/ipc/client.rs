use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{tokio::prelude::*, ConnectOptions, GenericFilePath};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct IpcClient {
    reader: BufReader<tokio::io::ReadHalf<interprocess::local_socket::tokio::Stream>>,
    writer: tokio::io::WriteHalf<interprocess::local_socket::tokio::Stream>,
}

impl IpcClient {
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let name = socket_path
            .to_str()
            .unwrap()
            .to_fs_name::<GenericFilePath>()?;
        let stream = ConnectOptions::new().name(name).connect_tokio().await?;
        let (read_half, write_half) = tokio::io::split(stream);
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
        })
    }

    pub async fn send(&mut self, msg: &IpcMessage) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<IpcMessage> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("IPC connection closed (EOF)");
        }
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

    #[tokio::test]
    async fn client_receives_multiple_rapid_messages() {
        use crate::types::{AuditEvent, EventKind};

        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let mut server = crate::ipc::server::IpcServer::new(&sock).await.unwrap();
        let tx = server.tx.clone();

        tokio::spawn(async move {
            server.accept_loop(|_| async move {}).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = IpcClient::connect(&sock).await.unwrap();
        client.send(&IpcMessage::Subscribe).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let make_event = |f: &str| IpcMessage::EventNotification {
            event: AuditEvent {
                ts: chrono::Utc::now(),
                event: EventKind::FileModified,
                world: "w".into(),
                agent: None, pid: None,
                file: Some(f.to_string()),
                files: None, worlds: None,
            },
        };
        let _ = tx.send(make_event("a.rs"));
        let _ = tx.send(make_event("b.rs"));

        let msg1 = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.recv(),
        ).await.unwrap().unwrap();

        let msg2 = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.recv(),
        ).await.unwrap().unwrap();

        assert!(matches!(&msg1, IpcMessage::EventNotification { event } if event.file.as_deref() == Some("a.rs")));
        assert!(matches!(&msg2, IpcMessage::EventNotification { event } if event.file.as_deref() == Some("b.rs")));
    }
}
