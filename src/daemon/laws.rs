use anyhow::Result;
use chrono::Utc;
use std::path::Path;


const PROTOCOL_TEMPLATE: &str = "<!-- YGGDRAZIL PROTOCOL ACTIVE -->\n\
# Yggdrazil Governance Protocol\n\
\n\
**You are operating in World: `{WORLD_ID}` on branch `{BRANCH}`.**\n\
\n\
Before starting any task:\n\
1. Read `.ygg/shared_memory.json` to understand what other agents are doing.\n\
2. After each iteration, run: `ygg hook --world {WORLD_ID} --files <comma-separated-files-you-touched>`\n\
\n\
This saves tokens for all agents by avoiding redundant rediscovery.\n";

pub fn inject_rules(
    world_path: &Path,
    world_id: &str,
    branch: &str,
    extra_rules: &[&Path],
) -> Result<()> {
    let content = PROTOCOL_TEMPLATE
        .replace("{WORLD_ID}", world_id)
        .replace("{BRANCH}", branch);

    let claude_md = world_path.join("CLAUDE.md");
    let existing = if claude_md.exists() {
        std::fs::read_to_string(&claude_md)?
    } else {
        String::new()
    };

    // Idempotent: don't inject twice
    if !existing.contains("YGGDRAZIL PROTOCOL ACTIVE") {
        std::fs::write(&claude_md, format!("{content}\n{existing}"))?;
    }

    // Append extra rules
    for rules_path in extra_rules {
        if rules_path.exists() {
            let rules = std::fs::read_to_string(rules_path)?;
            let current = std::fs::read_to_string(&claude_md)?;
            std::fs::write(&claude_md, format!("{current}\n---\n{rules}"))?;
        }
    }

    // Write .cursorrules stub
    let cursorrules = world_path.join(".cursorrules");
    if !cursorrules.exists() {
        std::fs::write(
            &cursorrules,
            format!("# Yggdrazil: World={world_id} Branch={branch}\n"),
        )?;
    }

    // Write .aider.conf.yml stub
    let aider_conf = world_path.join(".aider.conf.yml");
    if !aider_conf.exists() {
        std::fs::write(
            &aider_conf,
            format!("# Yggdrazil: World={world_id} Branch={branch}\n"),
        )?;
    }

    // .claude/ governance files — world-specific with hardcoded world_id
    let claude_dir = world_path.join(".claude");
    let hooks_dir = claude_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    // PostToolUse hook script — extracts file_path from Claude's stdin JSON
    let post_tool_script = hooks_dir.join("ygg-post-tool.sh");
    if !post_tool_script.exists() {
        let script = format!(
            "#!/usr/bin/env bash\n\
             # Yggdrazil PostToolUse hook for world {world_id}\n\
             file=$(python3 -c \"\
             import sys,json; \
             d=json.load(sys.stdin); \
             print(d.get('tool_input',{{}}).get('file_path',''))\
             \" 2>/dev/null)\n\
             [ -n \"$file\" ] && ygg hook --world {world_id} --files \"$file\" 2>/dev/null\n\
             exit 0\n"
        );
        std::fs::write(&post_tool_script, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&post_tool_script, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    // Stop hook script
    let stop_script = hooks_dir.join("ygg-stop.sh");
    if !stop_script.exists() {
        let script = format!(
            "#!/usr/bin/env bash\n\
             # Yggdrazil Stop hook for world {world_id}\n\
             ygg hook --world {world_id} 2>/dev/null\n\
             exit 0\n"
        );
        std::fs::write(&stop_script, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stop_script, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    // settings.json referencing the hook scripts
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.exists() {
        let settings = serde_json::json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Write|Edit|MultiEdit",
                    "hooks": [{
                        "type": "command",
                        "command": "bash .claude/hooks/ygg-post-tool.sh"
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": format!("bash .claude/hooks/ygg-stop.sh")
                    }]
                }]
            }
        });
        std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    }

    Ok(())
}

pub fn inject_conflict_warning(
    world_path: &Path,
    conflicting_world: &str,
    file: &str,
) -> Result<()> {
    let claude_md = world_path.join("CLAUDE.md");
    let existing = std::fs::read_to_string(&claude_md).unwrap_or_default();
    let warning = format!(
        "\n<!-- CONFLICT WARNING {} -->\n\
        ⚠️ **CONFLICT WARNING**: World `{}` is also modifying `{}`. \
        Avoid editing this file until `ygg sync` is run.\n",
        Utc::now().to_rfc3339(),
        conflicting_world,
        file
    );
    std::fs::write(&claude_md, format!("{existing}{warning}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn inject_writes_claude_md_with_protocol_header() {
        let dir = tempdir().unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap();

        let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("YGGDRAZIL PROTOCOL ACTIVE"));
        assert!(claude_md.contains("feat-auth"));
        assert!(claude_md.contains("main"));
    }

    #[test]
    fn inject_conflict_warning_appends_to_claude_md() {
        let dir = tempdir().unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap();
        inject_conflict_warning(dir.path(), "feat-api", "src/auth.rs").unwrap();

        let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("CONFLICT WARNING"));
        assert!(claude_md.contains("src/auth.rs"));
        assert!(claude_md.contains("feat-api"));
    }

    #[test]
    fn inject_writes_claude_settings_json() {
        let dir = tempdir().unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap();

        let settings_path = dir.path().join(".claude/settings.json");
        assert!(settings_path.exists(), ".claude/settings.json should be created");

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["hooks"]["PostToolUse"].is_array());
        assert!(parsed["hooks"]["Stop"].is_array());

        // World id is embedded in the stop script, not the settings command
        let stop_script_content =
            std::fs::read_to_string(dir.path().join(".claude/hooks/ygg-stop.sh")).unwrap();
        assert!(
            stop_script_content.contains("feat-auth"),
            "Stop hook script must reference world id"
        );

        assert!(
            dir.path().join(".claude/hooks/ygg-post-tool.sh").exists(),
            "ygg-post-tool.sh hook script should be created"
        );
        assert!(
            dir.path().join(".claude/hooks/ygg-stop.sh").exists(),
            "ygg-stop.sh hook script should be created"
        );
    }

    #[test]
    fn inject_is_idempotent() {
        let dir = tempdir().unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap();
        inject_rules(dir.path(), "feat-auth", "main", &[]).unwrap(); // second call

        let claude_md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        // Protocol header should appear only once
        assert_eq!(
            claude_md.matches("YGGDRAZIL PROTOCOL ACTIVE").count(),
            1
        );
    }
}
