use assert_cmd::Command;
use tempfile::tempdir;

fn make_git_repo(path: &std::path::Path) {
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(path)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
}

#[test]
fn init_creates_ygg_structure() {
    let repo = tempdir().unwrap();
    make_git_repo(repo.path());

    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();

    assert!(repo.path().join(".ygg").exists());
    assert!(repo.path().join(".ygg/worlds").exists());
    assert!(repo.path().join(".ygg/shared_memory.json").exists());
    assert!(repo.path().join(".ygg/audit.log").exists());

    let gitignore = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".ygg/"));
}

#[test]
fn init_creates_claude_governance_files() {
    let repo = tempdir().unwrap();
    make_git_repo(repo.path());

    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();

    // settings.json with Stop hook
    let settings_path = repo.path().join(".claude/settings.json");
    assert!(settings_path.exists(), ".claude/settings.json should be created");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert!(settings["hooks"]["Stop"].is_array(), "Stop hook must be present");

    // hook scripts
    assert!(
        repo.path().join(".claude/hooks/ygg-stop.sh").exists(),
        ".claude/hooks/ygg-stop.sh should be created"
    );

    // rules file
    assert!(
        repo.path().join(".claude/rules/ygg-governance.md").exists(),
        ".claude/rules/ygg-governance.md should be created"
    );
}

#[test]
fn init_shared_memory_is_valid_json() {
    let repo = tempdir().unwrap();
    make_git_repo(repo.path());

    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();

    let content =
        std::fs::read_to_string(repo.path().join(".ygg/shared_memory.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.is_object(), "shared_memory.json must be a JSON object");
}

#[test]
fn init_is_idempotent() {
    let repo = tempdir().unwrap();
    make_git_repo(repo.path());

    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();
    Command::cargo_bin("ygg")
        .unwrap()
        .args(["init"])
        .current_dir(repo.path())
        .assert()
        .success();

    let gitignore = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert_eq!(gitignore.matches(".ygg/").count(), 1);
}
