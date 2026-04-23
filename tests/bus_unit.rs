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
fn sequential_appends_preserve_all_events() {
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

#[test]
fn read_recent_respects_n_cap() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared_memory.json");
    let mut log = AuditLog::open(&path).unwrap();

    for i in 0..20 {
        log.append(&AuditEvent {
            ts: chrono::Utc::now(),
            event: EventKind::FileModified,
            world: format!("world-{i}"),
            agent: None, pid: None,
            file: Some("src/lib.rs".into()),
            files: None, worlds: None,
        }).unwrap();
    }

    let recent = log.read_recent(5, 2).unwrap();
    assert_eq!(recent.len(), 5);
    // Should be the last 5 worlds (world-15 through world-19)
    assert_eq!(recent[4].world, "world-19");
}

#[test]
fn read_recent_filters_old_events() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shared_memory.json");
    let mut log = AuditLog::open(&path).unwrap();

    // One old event (5 hours ago)
    let old_ts = chrono::Utc::now() - chrono::Duration::hours(5);
    log.append(&AuditEvent {
        ts: old_ts,
        event: EventKind::FileModified,
        world: "old-world".into(),
        agent: None, pid: None,
        file: Some("src/lib.rs".into()),
        files: None, worlds: None,
    }).unwrap();

    // One recent event
    log.append(&AuditEvent {
        ts: chrono::Utc::now(),
        event: EventKind::FileModified,
        world: "new-world".into(),
        agent: None, pid: None,
        file: Some("src/lib.rs".into()),
        files: None, worlds: None,
    }).unwrap();

    // With max_age_hours=2, only the recent event should appear
    let recent = log.read_recent(100, 2).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].world, "new-world");
}
