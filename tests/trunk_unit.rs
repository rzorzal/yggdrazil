use tempfile::tempdir;
use yggdrazil::daemon::trunk::{create_world, delete_world, list_worlds};

fn make_repo_with_commit(dir: &std::path::Path) {
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn creates_worktree_on_branch() {
    let repo_dir = tempdir().unwrap();
    make_repo_with_commit(repo_dir.path());
    std::fs::create_dir_all(repo_dir.path().join(".ygg/worlds")).unwrap();

    let world = create_world(repo_dir.path(), "feat-auth", "main").unwrap();

    assert!(world.path.exists(), "worktree dir should exist");
    assert_eq!(world.id, "feat-auth");
    assert_eq!(world.branch, "main");

    let worlds = list_worlds(repo_dir.path()).unwrap();
    assert!(worlds.iter().any(|w| w.id == "feat-auth"));
}

#[test]
fn lists_worlds_empty_when_none() {
    let repo_dir = tempdir().unwrap();
    make_repo_with_commit(repo_dir.path());
    std::fs::create_dir_all(repo_dir.path().join(".ygg/worlds")).unwrap();

    let worlds = list_worlds(repo_dir.path()).unwrap();
    assert!(worlds.is_empty());
}
