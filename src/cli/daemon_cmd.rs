use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn pid_file_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("daemon.pid")
}

pub fn start(repo_root: &Path) -> Result<()> {
    let pid_file = pid_file_path(repo_root);
    if pid_file.exists() {
        let pid = std::fs::read_to_string(&pid_file)?;
        println!("Daemon already running (PID {}).", pid.trim());
        return Ok(());
    }

    let exe = std::env::current_exe().context("could not find current executable")?;
    let child = std::process::Command::new(&exe)
        .args(["_daemon-run", repo_root.to_str().unwrap_or(".")])
        .spawn()
        .context("failed to spawn daemon process")?;

    std::fs::write(&pid_file, child.id().to_string())?;
    println!("✓ Daemon started (PID {}).", child.id());
    Ok(())
}

pub fn stop(repo_root: &Path) -> Result<()> {
    let pid_file = pid_file_path(repo_root);
    if !pid_file.exists() {
        println!("No daemon running.");
        return Ok(());
    }
    let pid_str = std::fs::read_to_string(&pid_file)?;
    let pid: u32 = pid_str.trim().parse().context("invalid PID in daemon.pid")?;

    kill_process(pid);

    std::fs::remove_file(&pid_file)?;
    println!("✓ Daemon stopped (PID {}).", pid);
    Ok(())
}

#[cfg(unix)]
fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

#[cfg(windows)]
fn kill_process(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_path_inside_ygg() {
        let dir = std::path::PathBuf::from("/tmp/myrepo");
        let path = pid_file_path(&dir);
        assert!(path.starts_with("/tmp/myrepo/.ygg"));
        assert!(path.to_str().unwrap().ends_with(".pid"));
    }
}
