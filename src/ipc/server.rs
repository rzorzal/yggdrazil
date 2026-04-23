use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{
    tokio::{prelude::*, Listener},
    GenericFilePath, ListenerOptions,
};
use std::future::Future;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
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

    pub async fn accept_loop<F, Fut>(&mut self, _handler: F) -> Result<()>
    where
        F: Fn(IpcMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send,
    {
        loop {
            let conn = self.listener.accept().await?;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(&conn);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    if let Ok(msg) = serde_json::from_str::<IpcMessage>(line.trim()) {
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
