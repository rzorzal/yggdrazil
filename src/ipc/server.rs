use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{
    tokio::{prelude::*, Listener},
    GenericFilePath, ListenerOptions,
};
use std::future::Future;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

pub struct IpcServer {
    listener: Listener,
    pub tx: broadcast::Sender<IpcMessage>,
}

impl IpcServer {
    pub async fn new(socket_path: &Path) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
        }
        let name = socket_path
            .to_str()
            .unwrap()
            .to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        let (tx, _) = broadcast::channel(256);
        Ok(Self { listener, tx })
    }

    pub async fn accept_loop<F, Fut>(&mut self, handler: F) -> Result<()>
    where
        F: Fn(IpcMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let handler = std::sync::Arc::new(handler);
        loop {
            let conn = self.listener.accept().await?;
            let tx = self.tx.clone();
            let handler = handler.clone();
            let mut rx = self.tx.subscribe();

            let (read_half, mut write_half) = tokio::io::split(conn);

            // Write task: forward EventNotification broadcast messages to this client
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(msg) => {
                            if !matches!(msg, IpcMessage::EventNotification { .. }) {
                                continue;
                            }
                            match serde_json::to_string(&msg) {
                                Ok(mut line) => {
                                    line.push('\n');
                                    if write_half.write_all(line.as_bytes()).await.is_err() {
                                        break;
                                    }
                                }
                                Err(_) => continue,
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, // slow client: drop missed events, keep forwarding
                        Err(_) => break,
                    }
                }
            });

            // Read task: receive messages from client and call handler
            tokio::spawn(async move {
                let mut reader = BufReader::new(read_half);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    if let Ok(msg) = serde_json::from_str::<IpcMessage>(line.trim()) {
                        handler(msg.clone()).await;
                        let _ = tx.send(msg);
                    }
                    line.clear();
                }
            });
        }
    }

    pub fn broadcast(&self, msg: IpcMessage) {
        let _ = self.tx.send(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IpcMessage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn server_pushes_broadcast_to_connected_client() {
        use crate::types::{AuditEvent, EventKind, IpcMessage};

        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();

        let mut server = IpcServer::new(&sock).await.unwrap();
        let tx = server.tx.clone();

        tokio::spawn(async move {
            server.accept_loop(|_msg| async move {}).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
        // Send Subscribe so connection is established
        client.send(&IpcMessage::Subscribe).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Server broadcasts an event
        let _ = tx.send(IpcMessage::EventNotification {
            event: AuditEvent {
                ts: chrono::Utc::now(),
                event: EventKind::FileModified,
                world: "feat-auth".into(),
                agent: None,
                pid: None,
                file: Some("src/auth.rs".into()),
                files: None,
                worlds: None,
            },
        });

        let received = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.recv(),
        ).await.unwrap().unwrap();

        assert!(
            matches!(received, IpcMessage::EventNotification { .. }),
            "expected EventNotification, got {:?}", received
        );
    }

    #[tokio::test]
    async fn server_accepts_and_echoes_subscribe() {
        let dir = tempdir().unwrap();
        let sock = crate::ipc::socket_path(dir.path());
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();
        let mut server = IpcServer::new(&sock).await.unwrap();

        tokio::spawn(async move {
            server
                .accept_loop(|msg| async move {
                    assert!(matches!(msg, IpcMessage::Subscribe));
                })
                .await
                .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = crate::ipc::client::IpcClient::connect(&sock).await.unwrap();
        client.send(&IpcMessage::Subscribe).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
