// src/ipc/client.rs — minimal stub to allow server test to compile
use crate::types::IpcMessage;
use anyhow::Result;
use interprocess::local_socket::{tokio::prelude::*, ConnectOptions, GenericFilePath};
use std::path::Path;
use tokio::io::AsyncWriteExt;

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
}
