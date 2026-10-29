use std::fs;
use std::path::Path;
use std::process::Command;

use kap_core::attempt::NewAttempt;
use kap_core::decision::NewDecision;
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

#[test]
fn exec_records_an_event_and_stores_its_output() {
    let (_dir, repo) = repository_with_commit();
    let attempt = repo
        .start_attempt(NewAttempt {
            label: "run",
            task: None,
            base: None,
        })
        .expect("start attempt");

    let output = repo
        .exec(
            &attempt.id,
            &[
                "sh".to_string(),
                "-c".to_string(),
                "echo hello; echo oops >&2; exit 3".to_string(),
            ],
        )
        .expect("exec");

    assert_eq!(output.exit_code, 3);
    assert_eq!(output.stdout, b"hello\n");
    assert_eq!(output.stderr, b"oops\n");

    let events = repo.tool_events(Some(&attempt.id)).expect("list events");
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.exit_code, Some(3));
    assert!(event.observed);

    let stdout = repo
        .objects()
        .get(event.stdout_hash.as_deref().unwrap())
        .expect("read stdout object");
    assert_eq!(stdout, b"hello\n");

    let stderr = repo
        .objects()
        .get(event.stderr_hash.as_deref().unwrap())
        .expect("read stderr object");
    assert_eq!(stderr, b"oops\n");
}

#[test]
fn identical_output_is_stored_once() {
    let (_dir, repo) = repository_with_commit();
    let attempt = repo
        .start_attempt(NewAttempt {
            label: "twice",
            task: None,
            base: None,
        })
        .expect("start attempt");

    let command = ["echo".to_string(), "same".to_string()];
    repo.exec(&attempt.id, &command).expect("first exec");
    repo.exec(&attempt.id, &command).expect("second exec");

    let events = repo.tool_events(Some(&attempt.id)).expect("list events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].stdout_hash, events[1].stdout_hash);

    let address = events[0].stdout_hash.as_deref().unwrap();
    assert!(repo.objects().path(address).unwrap().is_file());
}

#[test]
fn decisions_are_recorded_with_rationale() {
    let (_dir, repo) = repository_with_commit();
    let attempt = repo
        .start_attempt(NewAttempt {
            label: "decide",
            task: None,
            base: None,
        })
        .expect("start attempt");

    let decision = repo
        .add_decision(
            &attempt.id,
            NewDecision {
                summary: "Use a worktree per attempt",
                rationale: Some("changes must not leak between attempts"),
                alternatives: Some("reuse one checkout and stash between runs"),
            },
        )
        .expect("add decision");

    assert_eq!(decision.source, "agent_report");
    assert!(decision.id.starts_with("decision-"));

    let decisions = repo.decisions(&attempt.id).expect("list decisions");
    assert_eq!(decisions.len(), 1);
    assert_eq!(
        decisions[0].alternatives.as_deref(),
        Some("reuse one checkout and stash between runs")
    );
}

#[test]
fn decisions_need_an_existing_attempt() {
    let (_dir, repo) = repository_with_commit();

    match repo.add_decision(
        "attempt-missing",
        NewDecision {
            summary: "x",
            rationale: None,
            alternatives: None,
        },
    ) {
        Err(Error::AttemptNotFound { .. }) => {}
        other => panic!("expected AttemptNotFound, got {other:?}"),
    }
}

#[test]
fn open_inside_a_worktree_finds_the_main_store() {
    let (_dir, repo) = repository_with_commit();
    let attempt = repo
        .start_attempt(NewAttempt {
            label: "nested",
            task: None,
            base: None,
        })
        .expect("start attempt");

    let inside = Repo::open(&attempt.worktree).expect("open from the attempt worktree");
    assert_eq!(inside.root(), repo.root());
    assert_eq!(inside.attempts().expect("list attempts").len(), 1);
}

#[test]
fn closing_an_attempt_freezes_its_state() {
    let (_dir, repo) = repository_with_commit();
    let attempt = repo
        .start_attempt(NewAttempt {
            label: "close me",
            task: None,
            base: None,
        })
        .expect("start attempt");

    let closed = repo.close_attempt(&attempt.id).expect("close attempt");
    assert_eq!(closed.state, AttemptState::Closed);
    assert!(closed.closed_at.is_some());

    // Closing twice changes nothing.
    let again = repo.close_attempt(&attempt.id).expect("close again");
    assert_eq!(again.closed_at, closed.closed_at);
    assert_eq!(
        repo.attempts().expect("list")[0].state,
        AttemptState::Closed
    );
}

#[test]
fn a_closed_attempt_rejects_new_commands() {
    let (_dir, repo) = repository_with_commit();
    let attempt = repo
        .start_attempt(NewAttempt {
            label: "frozen",
            task: None,
            base: None,
        })
        .expect("start attempt");
    repo.close_attempt(&attempt.id).expect("close attempt");

    let error = repo
        .exec(&attempt.id, &["echo".to_string(), "late".to_string()])
        .expect_err("exec must fail on a closed attempt");
    assert!(matches!(error, Error::AttemptClosed { .. }));
}

#[test]
fn diff_reports_files_changed_between_attempts() {
    let (_dir, repo) = repository_with_commit();
    let first = repo
        .start_attempt(NewAttempt {
            label: "first",
            task: None,
            base: None,
        })
        .expect("start first");
    let second = repo
        .start_attempt(NewAttempt {
            label: "second",
            task: None,
            base: None,
        })
        .expect("start second");

    fs::write(first.worktree.join("parser.rs"), "fn parse() {}\n").expect("write first");
    git(&first.worktree, &["add", "."]);
    git(&first.worktree, &["commit", "-m", "parser"]);

    fs::write(
        second.worktree.join("parser.rs"),
        "fn parse() { /* commas */ }\nfn more() {}\n",
    )
    .expect("write second");
    git(&second.worktree, &["add", "."]);
    git(&second.worktree, &["commit", "-m", "trailing commas"]);

    let diff = repo
        .diff_attempts(&first.id, &second.id)
        .expect("diff attempts");

    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "parser.rs");
    assert_eq!(diff.files[0].added, Some(2));
    assert_eq!(diff.files[0].removed, Some(1));
    assert_eq!(diff.totals(), (2, 1));
}
