use anyhow::Result;
use std::path::Path;

pub fn run(repo_root: &Path) -> Result<()> {
    crate::tui::run_tui(repo_root)
}
