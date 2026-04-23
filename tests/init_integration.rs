use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn init_creates_ygg_structure() {
    let repo = tempdir().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Need at least one commit for git to work properly
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();

    assert!(repo.path().join(".ygg").exists());
    assert!(repo.path().join(".ygg/worlds").exists());
    assert!(repo.path().join(".ygg/shared_memory.json").exists());

    let gitignore = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".ygg/"));
}
