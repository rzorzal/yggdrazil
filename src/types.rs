use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub id: String,
    pub branch: String,
    pub path: PathBuf,
    pub managed: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub pid: u32,
    pub binary: String,
    pub world_id: String,
    pub active_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AgentSpawned,
    AgentExited,
    FileModified,
    IterationEnd,
    ConflictDetected,
    WarningInjected,
    WorldCreated,
    WorldMerged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub ts: DateTime<Utc>,
    pub event: EventKind,
    pub world: String,
    pub agent: Option<String>,
    pub pid: Option<u32>,
    pub file: Option<String>,
    pub files: Option<Vec<String>>,
    pub worlds: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub file: String,
    pub worlds: Vec<String>,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMessage {
    Subscribe,
    HookReport { world: String, files: Vec<String> },
    StateSnapshot {
        worlds: Vec<World>,
        agents: Vec<Agent>,
        conflicts: Vec<Conflict>,
    },
    EventNotification {
        event: AuditEvent,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_roundtrips_json() {
        let event = AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::AgentSpawned,
            world: "feature-auth".into(),
            agent: Some("claude-code".into()),
            pid: Some(1234),
            file: None,
            files: None,
            worlds: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.world, "feature-auth");
        assert_eq!(decoded.pid, Some(1234));
    }

    #[test]
    fn ipc_message_subscribe_roundtrips() {
        let msg = IpcMessage::Subscribe;
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: IpcMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, IpcMessage::Subscribe));
    }
}
