use anyhow::{Context, Result};
use std::path::Path;

fn ygg_stop_hook(ygg_bin: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         # Yggdrazil stop hook — signals session end when running inside a managed world.\n\
         grep -q 'YGGDRAZIL PROTOCOL' CLAUDE.md 2>/dev/null \\\n\
         \t&& \"{ygg_bin}\" hook --world \"$(basename \"$PWD\")\" 2>/dev/null\n\
         exit 0\n"
    )
}

fn ygg_post_tool_hook(ygg_bin: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         # Yggdrazil PostToolUse hook — only fires inside managed worlds.\n\
         grep -q 'YGGDRAZIL PROTOCOL' CLAUDE.md 2>/dev/null || exit 0\n\
         world_id=$(grep -o 'World: `[^`]*`' CLAUDE.md 2>/dev/null | sed 's/World: `//;s/`//')\n\
         file=$(python3 -c \"\
         import sys,json; \
         d=json.load(sys.stdin); \
         print(d.get('tool_input',{{}}).get('file_path',''))\
         \" 2>/dev/null)\n\
         [ -n \"$file\" ] && [ -n \"$world_id\" ] \
         && \"{ygg_bin}\" hook --world \"$world_id\" --files \"$file\" 2>/dev/null\n\
         exit 0\n"
    )
}

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

    let ygg_bin = std::env::current_exe()
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ygg".to_string());

    // .claude/ governance files
    let claude_dir = repo_root.join(".claude");
    let hooks_dir = claude_dir.join("hooks");
    let rules_dir = claude_dir.join("rules");
    std::fs::create_dir_all(&hooks_dir).context("failed to create .claude/hooks")?;
    std::fs::create_dir_all(&rules_dir).context("failed to create .claude/rules")?;

    let stop_script = hooks_dir.join("ygg-stop.sh");
    let post_tool_script = hooks_dir.join("ygg-post-tool.sh");

    if !stop_script.exists() {
        std::fs::write(&stop_script, ygg_stop_hook(&ygg_bin))
            .context("failed to create .claude/hooks/ygg-stop.sh")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stop_script, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    if !post_tool_script.exists() {
        std::fs::write(&post_tool_script, ygg_post_tool_hook(&ygg_bin))
            .context("failed to create .claude/hooks/ygg-post-tool.sh")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&post_tool_script, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    // Governance rules
    let rules_file = rules_dir.join("ygg-governance.md");
    if !rules_file.exists() {
        std::fs::write(&rules_file, YGG_GOVERNANCE_RULES)
            .context("failed to create .claude/rules/ygg-governance.md")?;
    }

    // settings.json — absolute paths so hooks fire regardless of CWD
    let stop_abs = stop_script.display().to_string();
    let post_tool_abs = post_tool_script.display().to_string();
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.exists() {
        let settings = serde_json::json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Write|Edit|MultiEdit",
                    "hooks": [{
                        "type": "command",
                        "command": format!("bash \"{post_tool_abs}\"")
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": format!("bash \"{stop_abs}\"")
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
