use anyhow::{Context, Result};
use std::path::Path;

pub fn run(repo_root: &Path, _rules: Option<&Path>) -> Result<()> {
    let ygg_dir = repo_root.join(".ygg");
    let worlds_dir = ygg_dir.join("worlds");
    let shared_memory = ygg_dir.join("shared_memory.json");
    let gitignore = repo_root.join(".gitignore");

    std::fs::create_dir_all(&worlds_dir)
        .context("failed to create .ygg/worlds")?;

    if !shared_memory.exists() {
        std::fs::write(&shared_memory, "")
            .context("failed to create shared_memory.json")?;
    }

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

    println!("✓ Yggdrazil initialized. Run `ygg daemon start` to begin monitoring.");
    Ok(())
}
