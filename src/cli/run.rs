use crate::cli::local;
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
    use_local: bool,
    ctx_size: u32,
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

    // 3. Resolve local model setup (before world creation so errors surface early)
    let local_setup = if use_local {
        Some(local::setup(agent, ctx_size)?)
    } else {
        None
    };

    // 4. Create world
    let world_id = world_id_for(agent, &branch);
    let local_model_name = local_setup.as_ref().map(|s| s.model.name.clone());
    let world = trunk::create_world(repo_root, &world_id, &branch, local_model_name.as_deref())?;

    // 5. Inject rules
    let extra = extra_rules.map(|p| vec![p]).unwrap_or_default();
    laws::inject_rules(&world.path, &world_id, &branch, &extra)?;

    // 6. Start llama-server if local mode
    let mut llama_proc: Option<std::process::Child> = None;
    if let Some(ref setup) = local_setup {
        if let Some(ref bin) = setup.server_bin {
            println!(
                "  Starting llama-server on port {} with model: {}",
                setup.server_port, setup.model.name
            );
            match local::start_server(bin, &setup.model, setup.server_port, setup.ctx_size) {
                Ok(child) => {
                    println!("  llama-server ready.");
                    llama_proc = Some(child);
                }
                Err(e) => {
                    eprintln!("⚠  llama-server failed to start: {e}");
                    eprintln!("   Env vars still set — point them to a running server manually.");
                }
            }
        } else {
            eprintln!("⚠  llama-server unavailable. Env vars set but no local server started.");
        }

        println!("🏠 LOCAL mode: {} @ port {}", setup.model.name, setup.server_port);
        println!("   Env vars injected:");
        for (k, v) in &setup.env_vars {
            println!("     {k}={v}");
        }
    }

    println!("✓ World `{world_id}` created on branch `{branch}`");
    println!("  Launching: {agent} {}", agent_args.join(" "));

    // 7. Spawn agent with all args, cwd = world path
    let mut cmd = std::process::Command::new(agent);
    cmd.args(agent_args).current_dir(&world.path);
    if let Some(ref setup) = local_setup {
        for (k, v) in &setup.env_vars {
            cmd.env(k, v);
        }
    }
    let status = cmd.status()?;
    let exit_code = status.code().unwrap_or(0);

    // 8. Kill llama-server
    if let Some(mut child) = llama_proc {
        let _ = child.kill();
    }

    // 9. Clean up world
    println!("\n✓ Agent exited (code {exit_code}). Cleaning up world `{world_id}`...");
    if let Err(e) = trunk::delete_world(repo_root, &world_id) {
        eprintln!("⚠  Could not fully clean up world `{world_id}`: {e}");
    } else {
        println!("✓ World `{world_id}` removed.");
    }

    // 10. Offer to launch another world with the same agent
    let restart = Confirm::new()
        .with_prompt("Launch a new world with the same agent?")
        .default(false)
        .interact()?;

    if restart {
        run(repo_root, agent, agent_args, extra_rules, use_local, ctx_size)
    } else {
        std::process::exit(exit_code);
    }
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
