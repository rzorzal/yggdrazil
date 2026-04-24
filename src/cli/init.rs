use anyhow::{Context, Result};
use std::path::Path;

const YGG_STOP_HOOK: &str = "\
#!/usr/bin/env bash
# Yggdrazil stop hook — signals session end when running inside a managed world.
# Fires on every Claude Code session stop; guard ensures it only acts in ygg worlds.
grep -q 'YGGDRAZIL PROTOCOL' CLAUDE.md 2>/dev/null \\
    && ygg hook --world \"$(basename \"$PWD\")\" 2>/dev/null
exit 0
";

const YGG_GOVERNANCE_RULES: &str = "\
# Yggdrazil Governance Rules

This project uses **Yggdrazil** multi-agent governance. These rules apply whenever
you are running inside a managed world (CLAUDE.md contains `YGGDRAZIL PROTOCOL ACTIVE`).

## Required Behaviour

1. **Before starting any task** — read `.ygg/shared_memory.json` to see what files
   other agents are currently modifying.
2. **After every file modification** — call `ygg hook` so other agents know what
   you are touching:
   ```
   ygg hook --world <WORLD_ID> --files <comma-separated-relative-paths>
   ```
3. **On conflict warnings** — if CLAUDE.md gains a `CONFLICT WARNING` block,
   stop editing that file and notify the human before continuing.

## Why

Each agent runs in an isolated git worktree. `shared_memory.json` is the only
shared state. Without it, agents will silently clobber each other's work.
";

pub fn run(repo_root: &Path, _rules: Option<&Path>) -> Result<()> {
    let ygg_dir = repo_root.join(".ygg");
    let worlds_dir = ygg_dir.join("worlds");
    let shared_memory = ygg_dir.join("shared_memory.json");
    let audit_log = crate::ipc::audit_log_path(repo_root);
    let gitignore = repo_root.join(".gitignore");

    std::fs::create_dir_all(&worlds_dir).context("failed to create .ygg/worlds")?;

    if !shared_memory.exists() {
        std::fs::write(&shared_memory, "{}").context("failed to create shared_memory.json")?;
    }

    if !audit_log.exists() {
        std::fs::write(&audit_log, "").context("failed to create audit.log")?;
    }

    // .gitignore
    let current = if gitignore.exists() {
        std::fs::read_to_string(&gitignore)?
    } else {
        String::new()
    };
    if !current.contains(".ygg/") {
        let entry = if current.ends_with('\n') || current.is_empty() {
            ".ygg/\n".to_string()
        } else {
            "\n.ygg/\n".to_string()
        };
        std::fs::write(&gitignore, format!("{current}{entry}"))?;
    }

    // .claude/ governance files
    let claude_dir = repo_root.join(".claude");
    let hooks_dir = claude_dir.join("hooks");
    let rules_dir = claude_dir.join("rules");
    std::fs::create_dir_all(&hooks_dir).context("failed to create .claude/hooks")?;
    std::fs::create_dir_all(&rules_dir).context("failed to create .claude/rules")?;

    // Stop hook script
    let stop_script = hooks_dir.join("ygg-stop.sh");
    if !stop_script.exists() {
        std::fs::write(&stop_script, YGG_STOP_HOOK)
            .context("failed to create .claude/hooks/ygg-stop.sh")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stop_script, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    // Governance rules
    let rules_file = rules_dir.join("ygg-governance.md");
    if !rules_file.exists() {
        std::fs::write(&rules_file, YGG_GOVERNANCE_RULES)
            .context("failed to create .claude/rules/ygg-governance.md")?;
    }

    // settings.json — references the stop hook script
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.exists() {
        let settings = serde_json::json!({
            "hooks": {
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": "bash .claude/hooks/ygg-stop.sh"
                    }]
                }]
            }
        });
        std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)
            .context("failed to create .claude/settings.json")?;
    }

    println!("✓ Yggdrazil initialized. Run `ygg daemon start` to begin monitoring.");
    Ok(())
}
