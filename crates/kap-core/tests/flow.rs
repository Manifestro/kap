use std::fs;
use std::path::Path;
use std::process::Command;

use kap_core::attempt::NewAttempt;
use kap_core::model::AttemptState;
use kap_core::repo::Repo;
use kap_core::Error;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=KAP Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be installed");

    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repository_with_commit() -> (tempfile::TempDir, Repo) {
    let dir = tempfile::tempdir().expect("create temp dir");
    git(dir.path(), &["init"]);
    fs::write(dir.path().join("README.md"), "# test\n").expect("write README");
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    let repo = Repo::init(dir.path()).expect("initialize KAP");
    (dir, repo)
}

#[test]
fn init_creates_the_store_and_ignores_it() {
    let (dir, repo) = repository_with_commit();

    assert!(repo.db_path().is_file());
    assert!(repo.kap_dir().join("objects").is_dir());
    assert!(repo.kap_dir().join("worktrees").is_dir());

    let ignore = fs::read_to_string(dir.path().join(".gitignore")).expect("read .gitignore");
    assert!(ignore.lines().any(|line| line.trim() == ".kap/"));

    let status = git(dir.path(), &["status", "--porcelain"]);
    assert!(
        !status.contains(".kap"),
        "the store must not show up as work: {status}"
    );
}

#[test]
fn init_refuses_to_overwrite_existing_history() {
    let (dir, _repo) = repository_with_commit();

    match Repo::init(dir.path()) {
        Err(Error::AlreadyInitialized { .. }) => {}
        other => panic!("expected AlreadyInitialized, got {other:?}"),
    }
}

#[test]
fn init_requires_a_git_repository() {
    let dir = tempfile::tempdir().expect("create temp dir");

    match Repo::init(dir.path()) {
        Err(Error::NotAGitRepository { .. }) => {}
        other => panic!("expected NotAGitRepository, got {other:?}"),
    }
}

#[test]
fn start_attempt_creates_an_isolated_worktree() {
    let (dir, repo) = repository_with_commit();

    let attempt = repo
        .start_attempt(NewAttempt {
            label: "first",
            task: Some("make the tests pass"),
            base: None,
        })
        .expect("start attempt");

    assert_eq!(attempt.state, AttemptState::Open);
    assert_eq!(attempt.label, "first");
    assert_eq!(attempt.task.as_deref(), Some("make the tests pass"));
    assert!(attempt.branch.starts_with("kap/attempt-"));
    assert!(attempt.worktree.is_dir());
    assert!(attempt.worktree.join("README.md").is_file());
    assert_eq!(attempt.base_commit, git(dir.path(), &["rev-parse", "HEAD"]));

    let branches = git(dir.path(), &["branch", "--list", &attempt.branch]);
    assert!(
        branches.contains(&attempt.branch),
        "branch missing: {branches}"
    );
}

#[test]
fn attempts_are_recorded_and_listed_newest_first() {
    let (_dir, repo) = repository_with_commit();

    let first = repo
        .start_attempt(NewAttempt {
            label: "first",
            task: None,
            base: None,
        })
        .expect("start first attempt");
    let second = repo
        .start_attempt(NewAttempt {
            label: "second",
            task: Some("other"),
            base: None,
        })
        .expect("start second attempt");

    let attempts = repo.attempts().expect("list attempts");
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].id, second.id);
    assert_eq!(attempts[1].id, first.id);
    assert_ne!(first.worktree, second.worktree);
}

#[test]
fn open_finds_the_store_from_a_subdirectory() {
    let (dir, _repo) = repository_with_commit();
    let nested = dir.path().join("src/deep");
    fs::create_dir_all(&nested).expect("create subdirectory");

    let repo = Repo::open(&nested).expect("open KAP from a subdirectory");
    let expected = dir
        .path()
        .canonicalize()
        .expect("canonicalize the repository root");
    assert_eq!(repo.root(), expected.as_path());
}
