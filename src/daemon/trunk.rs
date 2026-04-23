use crate::types::World;
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;

pub fn create_world(repo_root: &Path, world_id: &str, branch: &str) -> Result<World> {
    let world_path = repo_root.join(".ygg").join("worlds").join(world_id);

    // Use git CLI instead of git2 for worktree operations (more reliable cross-platform).
    // Determine the worktree branch name. If `branch` is already checked out in another
    // worktree (including the main worktree), we create a new branch named `world_id`
    // based off `branch` to avoid "already checked out" errors.
    let worktree_branch = resolve_worktree_branch(repo_root, world_id, branch)?;

    // Add worktree
    let output = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            world_path.to_str().unwrap(),
            &worktree_branch,
        ])
        .current_dir(repo_root)
        .output()
        .context("git worktree add failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {}", stderr);
    }

    // Write .env with port offset
    let existing_worlds = list_worlds(repo_root).unwrap_or_default();
    let port = 3000u16 + existing_worlds.len() as u16;
    std::fs::write(world_path.join(".env"), format!("PORT={port}\n"))?;

    Ok(World {
        id: world_id.to_string(),
        branch: branch.to_string(),
        path: world_path,
        managed: true,
        created_at: Utc::now(),
    })
}

/// Determine the git branch to use for the new worktree.
///
/// Git forbids checking out a branch that is already checked out in another
/// worktree. When that situation is detected we create a new branch named
/// `world_id` (based off `branch`) so the worktree gets its own branch.
fn resolve_worktree_branch(repo_root: &Path, world_id: &str, branch: &str) -> Result<String> {
    // List all branches that are currently checked out (worktree list output).
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .context("git worktree list failed")?;

    let porcelain = String::from_utf8_lossy(&output.stdout);
    let checked_out_branches: Vec<&str> = porcelain
        .lines()
        .filter_map(|l| l.strip_prefix("branch refs/heads/"))
        .collect();

    if checked_out_branches.iter().any(|b| *b == branch) {
        // Branch is already checked out — create a new branch named world_id based on branch.
        let new_branch = world_id.to_string();
        let exists = std::process::Command::new("git")
            .args(["branch", "--list", &new_branch])
            .current_dir(repo_root)
            .output()?
            .stdout
            .len() > 0;

        if !exists {
            let out = std::process::Command::new("git")
                .args(["branch", &new_branch, branch])
                .current_dir(repo_root)
                .output()
                .context("git branch (new) failed")?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!("git branch failed: {}", stderr);
            }
        }
        Ok(new_branch)
    } else {
        // Branch not checked out anywhere — ensure it exists then use it directly.
        let exists = std::process::Command::new("git")
            .args(["branch", "--list", branch])
            .current_dir(repo_root)
            .output()?
            .stdout
            .len() > 0;

        if !exists {
            let out = std::process::Command::new("git")
                .args(["branch", branch])
                .current_dir(repo_root)
                .output()
                .context("git branch failed")?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!("git branch failed: {}", stderr);
            }
        }
        Ok(branch.to_string())
    }
}

pub fn list_worlds(repo_root: &Path) -> Result<Vec<World>> {
    let worlds_dir = repo_root.join(".ygg").join("worlds");
    if !worlds_dir.exists() {
        return Ok(vec![]);
    }

    let mut worlds = vec![];
    for entry in std::fs::read_dir(&worlds_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();

        // Read branch from .git file (git worktrees have a `.git` file pointing to the gitdir)
        let branch = read_branch_from_worktree(&path).unwrap_or_else(|| "unknown".to_string());

        worlds.push(World {
            id,
            branch,
            path,
            managed: true,
            created_at: Utc::now(),
        });
    }
    Ok(worlds)
}

fn read_branch_from_worktree(worktree_path: &Path) -> Option<String> {
    // In a worktree, HEAD contains the branch ref
    let head = std::fs::read_to_string(worktree_path.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        Some(branch.to_string())
    } else {
        // Detached HEAD — return short SHA
        Some(head.chars().take(8).collect())
    }
}

pub fn delete_world(repo_root: &Path, world_id: &str) -> Result<()> {
    // Remove worktree via git CLI
    let output = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", world_id])
        .current_dir(repo_root)
        .output()
        .context("git worktree remove failed")?;

    if !output.status.success() {
        // Fallback: prune and remove directory manually
        let _ = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(repo_root)
            .output();

        let world_path = repo_root.join(".ygg").join("worlds").join(world_id);
        if world_path.exists() {
            std::fs::remove_dir_all(&world_path)?;
        }
    }

    Ok(())
}
