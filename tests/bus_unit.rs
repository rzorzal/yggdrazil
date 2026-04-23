use tempfile::tempdir;
use yggdrazil::daemon::bus::{AuditLog, EventKind, detect_conflicts};
use yggdrazil::types::AuditEvent;

#[test]
fn append_and_read_events() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared_memory.json");
    let mut log = AuditLog::open(&path).unwrap();

    log.append(&AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::AgentSpawned,
        world: "feat-auth".into(),
        agent: Some("claude-code".into()),
        pid: Some(1234),
        file: None,
        files: None,
        worlds: None,
    }).unwrap();

    let events = log.read_all().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].world, "feat-auth");
}

#[test]
fn append_is_atomic_and_does_not_corrupt() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared_memory.json");
    let mut log = AuditLog::open(&path).unwrap();

    for i in 0..10 {
        log.append(&AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: format!("world-{i}"),
            agent: None, pid: None, file: Some("src/lib.rs".into()),
            files: None, worlds: None,
        }).unwrap();
    }

    let events = log.read_all().unwrap();
    assert_eq!(events.len(), 10);
}

#[test]
fn detects_conflict_when_same_file_modified_in_two_worlds() {
    let events = vec![
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: Some("claude".into()), pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-api".into(),
            agent: Some("aider".into()), pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
    ];

    let conflicts = detect_conflicts(&events);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].file, "src/auth.rs");
    assert!(conflicts[0].worlds.contains(&"feat-auth".to_string()));
    assert!(conflicts[0].worlds.contains(&"feat-api".to_string()));
}

#[test]
fn no_conflict_same_file_same_world() {
    let events = vec![
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: None, pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
        AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: "feat-auth".into(),
            agent: None, pid: None,
            file: Some("src/auth.rs".into()), files: None, worlds: None,
        },
    ];

    let conflicts = detect_conflicts(&events);
    assert!(conflicts.is_empty());
}
