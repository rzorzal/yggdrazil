pub mod client;
pub mod server;

use std::path::{Path, PathBuf};

pub fn socket_path(repo_root: &Path) -> PathBuf {
    #[cfg(unix)]
    return repo_root.join(".ygg").join("daemon.sock");
    #[cfg(windows)]
    return repo_root.join(".ygg").join("daemon.pipe");
}

pub fn ygg_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg")
}

pub fn worlds_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("worlds")
}

pub fn shared_memory_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("shared_memory.json")
}

pub fn audit_log_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".ygg").join("audit.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_log_path_inside_ygg_dir() {
        let repo = std::path::Path::new("/tmp/myrepo");
        let path = audit_log_path(repo);
        assert_eq!(path, std::path::PathBuf::from("/tmp/myrepo/.ygg/audit.log"));
    }

    #[test]
    fn socket_path_inside_ygg_dir() {
        let repo = std::path::Path::new("/tmp/myrepo");
        let path = socket_path(repo);
        assert!(path.starts_with("/tmp/myrepo/.ygg/"));
        #[cfg(unix)]
        assert!(path.to_str().unwrap().ends_with(".sock"));
    }
}
