use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug)]
pub struct WorldDiff {
    pub world_id: String,
    pub branch: String,
    pub file_hunks: HashMap<String, Vec<(usize, usize)>>, // file → [(start, end)]
}

#[derive(Debug)]
pub struct OverlapReport {
    pub file: String,
    pub world_a: String,
    pub range_a: (usize, usize),
    pub world_b: String,
    pub range_b: (usize, usize),
}

/// Parse "@@ -start,count +start,count @@" → (start, start+count-1) inclusive end
pub fn parse_hunk_header(header: &str) -> Option<(usize, usize)> {
    let inner = header.trim_start_matches("@@ ").split(" @@").next()?;
    let old_part = inner.split(' ').next()?; // "-10,5"
    let old_part = old_part.trim_start_matches('-');
    let mut parts = old_part.splitn(2, ',');
    let start: usize = parts.next()?.parse().ok()?;
    let count: usize = parts.next().unwrap_or("1").parse().ok()?;
    Some((start, start + count.saturating_sub(1)))
}

/// Returns true if the two line ranges overlap (both inclusive).
pub fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

pub fn diff_world(repo_root: &Path, world_id: &str, branch: &str) -> Result<WorldDiff> {
    let output = std::process::Command::new("git")
        .args(["diff", "-U0", &format!("HEAD...refs/heads/{branch}")])
        .current_dir(repo_root)
        .output()
        .context("git diff failed")?;

    let text = String::from_utf8_lossy(&output.stdout);
    let mut file_hunks: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut current_file = String::new();

    for line in text.lines() {
        if line.starts_with("+++ b/") {
            current_file = line[6..].to_string();
        } else if line.starts_with("@@") {
            if let Some(range) = parse_hunk_header(line) {
                file_hunks.entry(current_file.clone()).or_default().push(range);
            }
        }
    }

    Ok(WorldDiff {
        world_id: world_id.to_string(),
        branch: branch.to_string(),
        file_hunks,
    })
}

pub fn find_overlaps(diffs: &[WorldDiff]) -> Vec<OverlapReport> {
    let mut reports = vec![];
    for i in 0..diffs.len() {
        for j in (i + 1)..diffs.len() {
            let a = &diffs[i];
            let b = &diffs[j];
            for (file, hunks_a) in &a.file_hunks {
                if let Some(hunks_b) = b.file_hunks.get(file) {
                    for &ra in hunks_a {
                        for &rb in hunks_b {
                            if ranges_overlap(ra, rb) {
                                reports.push(OverlapReport {
                                    file: file.clone(),
                                    world_a: a.world_id.clone(),
                                    range_a: ra,
                                    world_b: b.world_id.clone(),
                                    range_b: rb,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    reports
}

pub fn format_overlap(r: &OverlapReport) -> String {
    format!(
        "  ⚠ {} — {} (lines {}-{}) overlaps {} (lines {}-{})",
        r.file, r.world_a, r.range_a.0, r.range_a.1,
        r.world_b, r.range_b.0, r.range_b.1
    )
}

fn current_branch(repo_root: &Path) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root)
        .output()
        .context("git rev-parse failed")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run(repo_root: &Path, prune: bool) -> Result<()> {
    let worlds = crate::daemon::trunk::list_worlds(repo_root)?;
    if worlds.is_empty() {
        println!("No worlds to sync.");
        return Ok(());
    }

    println!("Computing diffs...\n");
    let diffs: Vec<WorldDiff> = worlds
        .iter()
        .filter_map(|w| diff_world(repo_root, &w.id, &w.branch).ok())
        .collect();

    let overlaps = find_overlaps(&diffs);

    println!("SYNC REPORT");
    println!("{}", "─".repeat(60));
    for diff in &diffs {
        let has_overlap = overlaps.iter().any(|o| o.world_a == diff.world_id || o.world_b == diff.world_id);
        let status = if has_overlap { "⚠ overlap" } else { "✓ safe" };
        println!("  {}  →  {} files changed  [{}]", diff.world_id, diff.file_hunks.len(), status);
    }
    if !overlaps.is_empty() {
        println!("\nOverlap details:");
        for o in &overlaps {
            println!("{}", format_overlap(o));
        }
    }
    println!();

    let trunk = current_branch(repo_root)?;
    if trunk == "HEAD" {
        anyhow::bail!("Cannot merge: repo is in detached HEAD state. Checkout a branch first.");
    }
    println!("Merging into: {trunk}\n");

    for world in &worlds {
        let prompt = format!("Merge `{}` (branch: {}) → trunk?", world.id, world.branch);
        let choice = dialoguer::Select::new()
            .with_prompt(&prompt)
            .items(&["yes", "no", "defer"])
            .default(1)
            .interact()?;

        if choice != 0 { continue; }

        let status = std::process::Command::new("git")
            .args(["merge", "--no-ff", &world.branch])
            .current_dir(repo_root)
            .status()?;

        if status.success() {
            println!("✓ Merged {}", world.branch);

            let log_path = repo_root.join(".ygg").join("shared_memory.json");
            let mut log = crate::daemon::bus::AuditLog::open(&log_path)?;
            log.append(&crate::types::AuditEvent {
                ts: chrono::Utc::now(),
                event: crate::types::EventKind::WorldMerged,
                world: world.id.clone(),
                agent: None, pid: None, file: None, files: None, worlds: None,
            })?;

            if prune {
                crate::daemon::trunk::delete_world(repo_root, &world.id)?;
                println!("✓ Pruned world {}", world.id);
            }
        } else {
            println!("✗ Merge conflict on {}. Resolve manually and re-run.", world.branch);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hunk_header_extracts_line_range() {
        let (start, end) = parse_hunk_header("@@ -10,5 +10,8 @@").unwrap();
        assert_eq!(start, 10);
        assert_eq!(end, 14);  // inclusive: lines 10-14 (5 lines)
    }

    #[test]
    fn ranges_overlap_detects_intersection() {
        assert!(ranges_overlap((10, 40), (35, 60)));
        assert!(!ranges_overlap((10, 30), (35, 60)));
        assert!(ranges_overlap((10, 40), (10, 20)));
    }

    #[test]
    fn format_overlap_report_contains_file_and_worlds() {
        let report = OverlapReport {
            file: "src/auth.rs".into(),
            world_a: "feat-auth".into(), range_a: (10, 40),
            world_b: "feat-api".into(),  range_b: (35, 60),
        };
        let formatted = format_overlap(&report);
        assert!(formatted.contains("src/auth.rs"));
        assert!(formatted.contains("feat-auth"));
        assert!(formatted.contains("feat-api"));
        assert!(formatted.contains("⚠"));
    }
}
