use crate::daemon::{laws, trunk};
use anyhow::Result;
use dialoguer::{Confirm, Select};
use std::path::Path;

pub fn world_id_for(agent: &str, branch: &str) -> String {
    let now = chrono::Utc::now();
    let safe_branch = branch.replace(['/', ' '], "-");
    let agent_short = agent.split('/').last().unwrap_or(agent);
    format!("{}-{}-{}", agent_short, safe_branch, now.format("%H%M%S%3f"))
}

fn list_local_branches(repo_root: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        _ => vec![],
    }
}

fn current_branch(repo_root: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() { "main".to_string() } else { s }
        }
        _ => "main".to_string(),
    }
}

pub fn run(
    repo_root: &Path,
    agent: &str,
    agent_args: &[String],
    extra_rules: Option<&Path>,
) -> Result<()> {
    // 1. Prompt for branch
    let branches = list_local_branches(repo_root);
    let head_branch = current_branch(repo_root);

    let branch = if branches.is_empty() {
        head_branch.clone()
    } else {
        let default_idx = branches
            .iter()
            .position(|b| b == &head_branch)
            .unwrap_or(0);
        let selection = Select::new()
            .with_prompt("Which branch for this world?")
            .items(&branches)
            .default(default_idx)
            .interact()?;
        branches[selection].clone()
    };

    // 2. Warn if branch already in use
    let worlds = trunk::list_worlds(repo_root)?;
    let collisions: Vec<_> = worlds.iter().filter(|w| w.branch == branch).collect();
    if !collisions.is_empty() {
        let names: Vec<&str> = collisions.iter().map(|w| w.id.as_str()).collect();
        eprintln!(
            "⚠️  Branch `{}` already in use by: {}",
            branch,
            names.join(", ")
        );
        let proceed = Confirm::new()
            .with_prompt("Continue anyway?")
            .default(false)
            .interact()?;
        if !proceed {
            return Ok(());
        }
    }

    // 3. Create world
    let world_id = world_id_for(agent, &branch);
    let world = trunk::create_world(repo_root, &world_id, &branch)?;

    // 4. Inject rules
    let extra = extra_rules.map(|p| vec![p]).unwrap_or_default();
    laws::inject_rules(&world.path, &world_id, &branch, &extra)?;

    println!("✓ World `{world_id}` created on branch `{branch}`");
    println!("  Launching: {agent} {}", agent_args.join(" "));

    // 5. Spawn agent with all args, cwd = world path
    let status = std::process::Command::new(agent)
        .args(agent_args)
        .current_dir(&world.path)
        .status()?;

    std::process::exit(status.code().unwrap_or(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_id_for_is_filesystem_safe() {
        let id = world_id_for("claude-code", "feat/auth");
        assert!(!id.is_empty());
        assert!(!id.contains('/'));
        assert!(!id.contains(' '));
    }

    #[test]
    fn world_id_for_uses_agent_and_branch() {
        let id = world_id_for("aider", "main");
        assert!(id.contains("aider"));
        assert!(id.contains("main"));
    }
}
