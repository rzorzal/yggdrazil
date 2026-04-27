use anyhow::Result;
use std::path::Path;

use crate::cli::local;

pub fn status(repo_root: &Path) -> Result<()> {
    match local::find_running_llama(repo_root) {
        Some(s) => {
            let gpu = s.gpu.as_deref().unwrap_or("CPU only");
            println!("llama-server: running");
            println!("  PID   : {}", s.pid);
            println!("  Port  : {}", s.port);
            println!("  Model : {}", s.model_name);
            println!("  GPU   : {gpu}");
        }
        None => {
            // Check if there's a stale file
            if local::llama_state_path(repo_root).exists() {
                println!("llama-server: not running (stale PID file removed)");
            } else {
                println!("llama-server: not running");
            }
        }
    }
    Ok(())
}

pub fn stop(repo_root: &Path) -> Result<()> {
    local::stop_server(repo_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn status_on_empty_repo_does_not_panic() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();
        status(dir.path()).unwrap();
    }

    #[test]
    fn stop_with_no_pid_file_is_noop() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ygg")).unwrap();
        stop(dir.path()).unwrap();
    }
}
