pub use crate::types::EventKind;
use crate::types::AuditEvent;
use crate::types::Conflict;
use anyhow::Result;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            std::fs::write(path, "")?;
        }
        Ok(Self { path: path.to_path_buf() })
    }

    pub fn append(&mut self, event: &AuditEvent) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(event)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<AuditEvent>> {
        let file = std::fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut events = vec![];
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<AuditEvent>(trimmed) {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Read the last `n` events within `max_age_hours` hours.
    pub fn read_recent(&self, n: usize, max_age_hours: i64) -> Result<Vec<AuditEvent>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(max_age_hours);
        let all = self.read_all()?;
        Ok(all
            .into_iter()
            .filter(|e| e.ts > cutoff)
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
}

/// Scan events for file conflicts (same file, different worlds).
/// Pure aggregation — caller is responsible for pre-filtering by time/count.
pub fn detect_conflicts(events: &[AuditEvent]) -> Vec<Conflict> {
    let mut file_worlds: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for event in events {
        if matches!(event.event, EventKind::FileModified | EventKind::IterationEnd) {
            if let Some(file) = &event.file {
                file_worlds
                    .entry(file.clone())
                    .or_default()
                    .insert(event.world.clone());
            }
        }
    }

    file_worlds
        .into_iter()
        .filter(|(_, worlds)| worlds.len() > 1)
        .map(|(file, worlds)| Conflict {
            file,
            worlds: {
                let mut v: Vec<String> = worlds.into_iter().collect();
                v.sort();
                v
            },
            detected_at: chrono::Utc::now(),
        })
        .collect()
}

pub fn notify_conflict(file: &str, worlds: &[String]) {
    let body = format!("Conflict in {}: worlds {:?}", file, worlds);
    tracing::warn!("{}", body);
    #[cfg(not(test))]
    {
        let _ = notify_rust::Notification::new()
            .summary("Yggdrazil: Conflict Detected")
            .body(&body)
            .show();
    }
}
