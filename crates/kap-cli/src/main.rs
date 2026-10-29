use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kap_core::attempt::NewAttempt;
use kap_core::decision::NewDecision;
use kap_core::diff::AttemptDiff;
use kap_core::evidence::{EvidenceSource, NewEvidence};
use kap_core::hooks::HookInput;
use kap_core::model::{Attempt, Decision, Evidence, Snapshot, ToolEvent};
use kap_core::repo::Repo;
use kap_core::review::Verdict;

#[derive(Debug, Parser)]
#[command(
    name = "kap",
    version,
    about = "Versioning layer for work done by AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize KAP in a git repository
    Init {
        /// Repository to initialize (defaults to the current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Show where you are and what is recorded
    Status,
    /// Work with attempts
    Attempt {
        #[command(subcommand)]
        command: AttemptCommand,
    },
    /// Run a command inside an attempt and record it
    Exec {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
        /// The command to run, after `--`
        #[arg(last = true, required = true, value_name = "COMMAND")]
        command: Vec<String>,
    },
    /// Record what an agent decided and why
    Decision {
        #[command(subcommand)]
        command: DecisionCommand,
    },
    /// Show the timeline of commands and decisions for an attempt
    Log {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
    },
    /// Receive Claude Code hook events
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Attach evidence to an attempt, or list what is attached
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    /// Share this repository's history through a git remote
    Push {
        /// Git remote to push to
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// Bring in history pushed from another machine
    Pull {
        /// Git remote to pull from
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// Capture and restore the state of an attempt
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Inspect an attempt and record a verdict
    Review {
        /// The attempt to review
        attempt: String,
        /// Approve the attempt for merging
        #[arg(long)]
        approve: bool,
        /// Reject the attempt
        #[arg(long)]
        reject: bool,
        /// Note to record with the verdict
        #[arg(long)]
        note: Option<String>,
    },
    /// Merge an approved attempt into the current branch
    Merge {
        /// The attempt to merge
        attempt: String,
    },
    /// Compare two attempts
    Diff {
        /// The attempt to compare from
        from: String,
        /// The attempt to compare to
        to: String,
    },
}

#[derive(Debug, Subcommand)]
enum AttemptCommand {
    /// Open a new attempt in an isolated worktree
    Start {
        /// Short name for the attempt, e.g. "fix-flaky-test"
        label: String,
        /// What the agent is trying to achieve
        #[arg(long)]
        task: Option<String>,
        /// Commit or ref to branch from (defaults to HEAD)
        #[arg(long, value_name = "REV")]
        base: Option<String>,
    },
    /// Remove the worktree of a closed attempt
    Clean {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
    },
    /// Close an attempt and freeze its history
    Close {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
    },
    /// List attempts, newest first
    List,
}

#[derive(Debug, Subcommand)]
enum DecisionCommand {
    /// Record a decision
    Add {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
        /// What was decided
        #[arg(long)]
        summary: String,
        /// Why it was decided
        #[arg(long)]
        rationale: Option<String>,
        /// What else was considered
        #[arg(long)]
        alternatives: Option<String>,
    },
    /// List the decisions recorded for an attempt
    List {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SnapshotCommand {
    /// Capture the current state of an attempt
    Create {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
        /// Note to remember the snapshot by
        #[arg(long)]
        note: Option<String>,
    },
    /// List snapshots of an attempt
    List {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
    },
    /// Restore a snapshot as a new attempt
    Restore {
        /// Snapshot id
        snapshot: String,
    },
}

#[derive(Debug, Subcommand)]
enum HookCommand {
    /// SessionStart hook: brief the agent on prior work
    SessionStart,
    /// PostToolUse hook: record a tool call
    PostToolUse,
    /// PostToolUseFailure hook: record a failed tool call
    PostToolUseFailure,
    /// Stop hook: finish the run for this session
    Stop,
    /// Write the hooks into .claude/settings.json
    Setup,
}

#[derive(Debug, Subcommand)]
enum EvidenceCommand {
    /// Store an artifact as evidence, optionally linked to a decision
    Attach {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
        /// Where the evidence comes from
        #[arg(long, value_enum)]
        source: SourceArg,
        /// File to store and hash as the artifact
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// The command the evidence came from
        #[arg(long)]
        command: Option<String>,
        /// The command's exit code
        #[arg(long, value_name = "CODE")]
        exit_code: Option<i32>,
        /// Decision this evidence supports or undermines
        #[arg(long, value_name = "ID")]
        decision: Option<String>,
        /// Mark the evidence as partial
        #[arg(long)]
        incomplete: bool,
    },
    /// List the evidence recorded for an attempt
    List {
        /// Attempt id (inferred from the worktree you are in when omitted)
        #[arg(long)]
        attempt: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum SourceArg {
    ToolExecution,
    FileChange,
    TestResult,
    AgentReport,
    ExternalArtifact,
}

impl From<SourceArg> for EvidenceSource {
    fn from(arg: SourceArg) -> EvidenceSource {
        match arg {
            SourceArg::ToolExecution => EvidenceSource::ToolExecution,
            SourceArg::FileChange => EvidenceSource::FileChange,
            SourceArg::TestResult => EvidenceSource::TestResult,
            SourceArg::AgentReport => EvidenceSource::AgentReport,
            SourceArg::ExternalArtifact => EvidenceSource::ExternalArtifact,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    match Cli::parse().command {
        Command::Init { path } => {
            let repo = Repo::init(&path).map_err(show)?;
            println!("Initialized KAP in {}", repo.kap_dir().display());
            Ok(ExitCode::SUCCESS)
        }

        Command::Status => {
            let repo = open_repo()?;
            print_status(&repo)?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Attempt { command } => {
            let repo = open_repo()?;
            match command {
                AttemptCommand::Start { label, task, base } => {
                    let attempt = repo
                        .start_attempt(NewAttempt {
                            label: &label,
                            task: task.as_deref(),
                            base: base.as_deref(),
                        })
                        .map_err(show)?;
                    print_attempt(&attempt);
                    Ok(ExitCode::SUCCESS)
                }
                AttemptCommand::Clean { attempt } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let outcome = repo.clean_attempt(&attempt_id).map_err(show)?;
                    match (outcome.worktree_removed, outcome.branch_deleted) {
                        (true, true) => {
                            println!("Removed the worktree and the merged branch of {attempt_id}.")
                        }
                        (true, false) => println!(
                            "Removed the worktree of {attempt_id}; its branch is unmerged and was \
                             kept."
                        ),
                        (false, true) => {
                            println!("The worktree was already gone; deleted the merged branch.")
                        }
                        (false, false) => println!("Nothing to remove for {attempt_id}."),
                    }
                    Ok(ExitCode::SUCCESS)
                }
                AttemptCommand::Close { attempt } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let closed = repo.close_attempt(&attempt_id).map_err(show)?;
                    println!("{}  {}", closed.id, closed.state);
                    Ok(ExitCode::SUCCESS)
                }
                AttemptCommand::List => {
                    let attempts = repo.attempts().map_err(show)?;
                    if attempts.is_empty() {
                        println!("No attempts yet.");
                        return Ok(ExitCode::SUCCESS);
                    }
                    for attempt in &attempts {
                        println!("{}  {}", attempt.id, attempt.state);
                        println!("  label:    {}", attempt.label);
                        println!("  branch:   {}", attempt.branch);
                        println!("  created:  {}", attempt.created_at);
                    }
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        Command::Exec { attempt, command } => {
            let repo = open_repo()?;
            let attempt_id = require_attempt(&repo, attempt.as_deref())?;
            let output = repo.exec(&attempt_id, &command).map_err(show)?;

            std::io::stdout().write_all(&output.stdout).map_err(show)?;
            std::io::stderr().write_all(&output.stderr).map_err(show)?;
            eprintln!("kap: recorded command, exit {}", output.exit_code);

            Ok(exit_code(output.exit_code))
        }

        Command::Decision { command } => {
            let repo = open_repo()?;
            match command {
                DecisionCommand::Add {
                    attempt,
                    summary,
                    rationale,
                    alternatives,
                } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let decision = repo
                        .add_decision(
                            &attempt_id,
                            NewDecision {
                                summary: &summary,
                                rationale: rationale.as_deref(),
                                alternatives: alternatives.as_deref(),
                            },
                        )
                        .map_err(show)?;
                    println!("{}  {}", decision.id, decision.summary);
                    Ok(ExitCode::SUCCESS)
                }
                DecisionCommand::List { attempt } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let decisions = repo.decisions(&attempt_id).map_err(show)?;
                    if decisions.is_empty() {
                        println!("No decisions recorded for {attempt_id}.");
                        return Ok(ExitCode::SUCCESS);
                    }
                    for decision in &decisions {
                        print_decision(decision);
                    }
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        Command::Log { attempt } => {
            let repo = open_repo()?;
            match resolve_attempt(&repo, attempt.as_deref())? {
                Some(attempt_id) => print_log(&repo, &attempt_id)?,
                None => print_all_logs(&repo)?,
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Hook { command } => handle_hook(command),

        Command::Evidence { command } => {
            let repo = open_repo()?;
            match command {
                EvidenceCommand::Attach {
                    attempt,
                    source,
                    file,
                    command: evidence_command,
                    exit_code,
                    decision,
                    incomplete,
                } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let evidence = repo
                        .attach_evidence(
                            &attempt_id,
                            NewEvidence {
                                source: source.into(),
                                command: evidence_command.as_deref(),
                                exit_code,
                                artifact: file.as_deref(),
                                decision: decision.as_deref(),
                                complete: !incomplete,
                            },
                        )
                        .map_err(show)?;
                    print_evidence(&evidence);
                    Ok(ExitCode::SUCCESS)
                }
                EvidenceCommand::List { attempt } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let evidence = repo.evidence(&attempt_id).map_err(show)?;
                    if evidence.is_empty() {
                        println!("No evidence recorded for {attempt_id}.");
                        return Ok(ExitCode::SUCCESS);
                    }
                    for entry in &evidence {
                        print_evidence(entry);
                    }
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        Command::Push { remote } => {
            let repo = open_repo()?;
            let report = repo.push(&remote).map_err(show)?;
            if report.rows == 0 && report.objects == 0 {
                println!("Nothing new to push to {remote}.");
            } else {
                println!(
                    "Pushed {} rows and {} objects to {remote}",
                    report.rows, report.objects
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Pull { remote } => {
            let repo = open_repo()?;
            let report = repo.pull(&remote).map_err(show)?;
            if report.refs == 0 {
                println!("Nothing to pull from {remote}.");
            } else {
                println!(
                    "Pulled {} new rows, {} updated, {} objects from {} machine ref(s) on {remote}",
                    report.rows, report.updated, report.objects, report.refs
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Snapshot { command } => {
            let repo = open_repo()?;
            match command {
                SnapshotCommand::Create { attempt, note } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let snapshot = repo
                        .create_snapshot(&attempt_id, note.as_deref())
                        .map_err(show)?;
                    print_snapshot(&snapshot);
                    Ok(ExitCode::SUCCESS)
                }
                SnapshotCommand::List { attempt } => {
                    let attempt_id = require_attempt(&repo, attempt.as_deref())?;
                    let snapshots = repo.snapshots(&attempt_id).map_err(show)?;
                    if snapshots.is_empty() {
                        println!("No snapshots for {attempt_id}.");
                        return Ok(ExitCode::SUCCESS);
                    }
                    for snapshot in &snapshots {
                        print_snapshot(snapshot);
                    }
                    Ok(ExitCode::SUCCESS)
                }
                SnapshotCommand::Restore { snapshot } => {
                    let attempt = repo.restore_snapshot(&snapshot).map_err(show)?;
                    println!("Restored {snapshot} as a new attempt:");
                    print_attempt(&attempt);
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        Command::Review {
            attempt,
            approve,
            reject,
            note,
        } => {
            let repo = open_repo()?;
            if approve && reject {
                return Err("pass either --approve or --reject, not both".to_string());
            }
            if approve || reject {
                let verdict = if approve {
                    Verdict::Approved
                } else {
                    Verdict::Rejected
                };
                let review = repo
                    .review(&attempt, verdict, note.as_deref())
                    .map_err(show)?;
                println!(
                    "{}  {}  head {}",
                    review.id,
                    review.verdict,
                    short_commit(&review.head_commit)
                );
                if let Some(note) = &review.note {
                    println!("  note: {note}");
                }
            }
            print_review(&repo, &attempt)?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Merge { attempt } => {
            let repo = open_repo()?;
            let outcome = repo.merge_attempt(&attempt).map_err(show)?;
            println!(
                "Merged {} into {} at {} ({} file(s) changed); the attempt is closed",
                outcome.attempt_id,
                outcome.branch,
                short_commit(&outcome.commit),
                outcome.files
            );
            println!(
                "Its worktree is still in place; `kap attempt clean --attempt {}` removes it.",
                outcome.attempt_id
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Diff { from, to } => {
            let repo = open_repo()?;
            let diff = repo.diff_attempts(&from, &to).map_err(show)?;
            print_diff(&diff);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_log(repo: &Repo, attempt_id: &str) -> Result<(), String> {
    let events = repo.tool_events(Some(attempt_id)).map_err(show)?;
    let decisions = repo.decisions(attempt_id).map_err(show)?;

    if events.is_empty() && decisions.is_empty() {
        println!("Nothing recorded for {attempt_id} yet.");
        return Ok(());
    }

    let mut entries: Vec<(&str, Entry<'_>)> = Vec::with_capacity(events.len() + decisions.len());
    for event in &events {
        entries.push((&event.occurred_at, Entry::Event(event)));
    }
    for decision in &decisions {
        entries.push((&decision.created_at, Entry::Decision(decision)));
    }
    entries.sort_by(|a, b| a.0.cmp(b.0));

    for (_, entry) in entries {
        match entry {
            Entry::Event(event) => print_event(event),
            Entry::Decision(decision) => print_decision(decision),
        }
    }
    Ok(())
}

enum Entry<'a> {
    Event(&'a ToolEvent),
    Decision(&'a Decision),
}

fn print_event(event: &ToolEvent) {
    let command = event.command.as_deref().unwrap_or("<unknown>");
    let outcome = match event.exit_code {
        Some(code) => format!("  exit {code}"),
        None => String::new(),
    };
    let agent = match &event.agent_type {
        Some(kind) => format!("  [subagent: {kind}]"),
        None => String::new(),
    };
    println!(
        "{}  {:<9} {}{}{}",
        time_of(&event.occurred_at),
        event.kind,
        summarise(command),
        outcome,
        agent
    );
}

fn print_decision(decision: &Decision) {
    println!(
        "{}  decision  {}",
        time_of(&decision.created_at),
        decision.summary
    );
    if let Some(rationale) = &decision.rationale {
        println!("              why: {rationale}");
    }
    if let Some(alternatives) = &decision.alternatives {
        println!("              instead of: {alternatives}");
    }
}

fn handle_hook(command: HookCommand) -> Result<ExitCode, String> {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return Ok(ExitCode::SUCCESS),
    };

    if matches!(command, HookCommand::Setup) {
        // Write into the checkout the agent actually runs in. Inside a worktree that is the
        // worktree itself, not the main repository.
        let checkout = kap_core::git::Git::discover(&cwd)
            .map_err(show)?
            .root()
            .to_path_buf();
        setup_hooks(&checkout)?;
        return Ok(ExitCode::SUCCESS);
    }

    let mut json = String::new();
    let _ = std::io::stdin().read_to_string(&mut json);

    // A hook must never break the agent's session. Outside a KAP repository there is nothing to
    // record, and inside one a failure is logged rather than shown to the agent.
    let Ok(repo) = Repo::open(&cwd) else {
        return Ok(ExitCode::SUCCESS);
    };
    let Some(attempt_id) = attempt_at(&repo, &cwd) else {
        return Ok(ExitCode::SUCCESS);
    };

    if let Err(message) = run_hook(&repo, &attempt_id, command, &json) {
        log_hook_failure(&repo, &message);
    }

    Ok(ExitCode::SUCCESS)
}

fn run_hook(repo: &Repo, attempt_id: &str, command: HookCommand, json: &str) -> Result<(), String> {
    let input = HookInput::parse(json).map_err(show)?;
    let attempt = repo.attempt(attempt_id).map_err(show)?;

    match command {
        HookCommand::SessionStart => {
            let context = repo.session_context(&attempt).map_err(show)?;
            let payload = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": context,
                }
            });
            println!("{payload}");
        }
        HookCommand::PostToolUse | HookCommand::PostToolUseFailure => {
            let run = repo
                .run_for_session(attempt_id, input.session())
                .map_err(show)?;
            let failed = matches!(command, HookCommand::PostToolUseFailure);
            repo.record_hook_event(&attempt, &run.id, &input, failed)
                .map_err(show)?;
        }
        HookCommand::Stop => {
            repo.finish_run_for_session(input.session(), "completed")
                .map_err(show)?;
            repo.record_worktree_snapshot(&attempt).map_err(show)?;
        }
        HookCommand::Setup => unreachable!("setup is handled before reading stdin"),
    }

    Ok(())
}

/// Record a hook failure without disturbing the agent. The file is the only place to look when
/// something silently stops being recorded.
fn log_hook_failure(repo: &Repo, message: &str) {
    let dir = repo.kap_dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }

    let path = dir.join("hooks.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{}  {message}", kap_core::model::now_rfc3339());
    }
}

const HOOK_EVENTS: &[(&str, &str)] = &[
    ("SessionStart", "session-start"),
    ("PostToolUse", "post-tool-use"),
    ("PostToolUseFailure", "post-tool-use-failure"),
    ("Stop", "stop"),
];

fn setup_hooks(checkout: &Path) -> Result<(), String> {
    let dir = checkout.join(".claude");
    std::fs::create_dir_all(&dir).map_err(show)?;
    let path = dir.join("settings.json");

    let mut settings: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(show)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(show(error)),
    };

    let root = settings
        .as_object_mut()
        .ok_or_else(|| "settings.json must contain a JSON object".to_string())?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "`hooks` in settings.json must be an object".to_string())?;

    let binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kap"));

    for (event, subcommand) in HOOK_EVENTS {
        let command = hook_command(&binary, subcommand);
        let entries = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("`hooks.{event}` must be an array"))?;

        let present = entries.iter().any(|entry| {
            entry
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .map(|hooks| {
                    hooks
                        .iter()
                        .any(|hook| hook.get("command") == Some(&serde_json::json!(command)))
                })
                .unwrap_or(false)
        });

        if !present {
            entries.push(serde_json::json!({
                "hooks": [{ "type": "command", "command": command }]
            }));
        }
    }

    let text = serde_json::to_string_pretty(&settings).map_err(show)?;
    std::fs::write(&path, format!("{text}\n")).map_err(show)?;
    println!("Wired KAP hooks into {}", path.display());
    println!(
        "The hooks call {}, so they work without KAP on PATH. Commit this file to share it.",
        binary.display()
    );
    Ok(())
}

fn print_evidence(evidence: &Evidence) {
    let state = match (evidence.observed, evidence.complete) {
        (true, true) => "observed, complete",
        (true, false) => "observed, partial",
        (false, _) => "reported, not observed",
    };
    let artifact = evidence.artifact_hash.as_deref().unwrap_or("<no artifact>");
    println!("{}  {}  {state}  {artifact}", evidence.id, evidence.source);
    if let Some(command) = &evidence.command {
        let code = evidence
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "?".to_string());
        println!("  command: {command}  -> exit {code}");
    }
    if let Some(path) = &evidence.path {
        println!("  file: {path}");
    }
    if let Some(decision) = &evidence.decision_id {
        println!("  decision: {decision}");
    }
}

fn print_snapshot(snapshot: &Snapshot) {
    println!(
        "{}  {}  {}",
        snapshot.id,
        short_commit(&snapshot.commit),
        snapshot.created_at
    );
    if let Some(note) = &snapshot.note {
        println!("  note: {note}");
    }
}

fn hook_command(binary: &Path, subcommand: &str) -> String {
    let binary = binary.to_string_lossy();
    if binary.contains(' ') {
        format!("\"{binary}\" hook {subcommand}")
    } else {
        format!("{binary} hook {subcommand}")
    }
}

fn print_status(repo: &Repo) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(show)?;
    let machine = repo.machine_id().map_err(show)?;

    println!("repo:    {}", repo.root().display());
    println!("store:   {}", repo.kap_dir().display());
    println!("machine: {machine}");

    match attempt_at(repo, &cwd) {
        Some(attempt_id) => {
            let attempt = repo.attempt(&attempt_id).map_err(show)?;
            let changes = repo.changes_since_base(&attempt_id).map_err(show)?;
            let uncommitted = repo.worktree_changes(&attempt_id).map_err(show)?;
            let decisions = repo.decisions(&attempt_id).map_err(show)?;
            let evidence = repo.evidence(&attempt_id).map_err(show)?;
            let review = repo.latest_review(&attempt_id).map_err(show)?;
            let observed = evidence.iter().filter(|entry| entry.observed).count();

            println!();
            println!("attempt: {}  {}", attempt.id, attempt.state);
            println!("  label:  {}", attempt.label);
            println!("  branch: {}", attempt.branch);
            println!(
                "  changed files: {} since base, {} uncommitted",
                changes.len(),
                uncommitted.len()
            );
            println!("  decisions: {}", decisions.len());
            println!(
                "  evidence: {observed} observed, {} reported",
                evidence.len() - observed
            );
            match review {
                Some(review) => println!(
                    "  review: {} at {}",
                    review.verdict,
                    short_commit(&review.head_commit)
                ),
                None => println!("  review: none"),
            }
        }
        None => {
            let open = repo
                .attempts()
                .map_err(show)?
                .iter()
                .filter(|attempt| attempt.state == kap_core::model::AttemptState::Open)
                .count();
            println!();
            println!("attempt: none (you are in the main checkout)");
            println!("open attempts: {open}");
            println!(
                "note: hooks record only inside an attempt worktree; start one with \
                 `kap attempt start`."
            );
        }
    }

    Ok(())
}

fn print_all_logs(repo: &Repo) -> Result<(), String> {
    let attempts = repo.attempts().map_err(show)?;
    if attempts.is_empty() {
        println!("No attempts yet.");
        return Ok(());
    }

    for attempt in attempts.iter().rev() {
        println!("== {}  {}  {}", attempt.id, attempt.state, attempt.label);
        print_log(repo, &attempt.id)?;
    }
    Ok(())
}

fn print_review(repo: &Repo, attempt_id: &str) -> Result<(), String> {
    let attempt = repo.attempt(attempt_id).map_err(show)?;
    let changes = repo.changes_since_base(attempt_id).map_err(show)?;
    let decisions = repo.decisions(attempt_id).map_err(show)?;
    let evidence = repo.evidence(attempt_id).map_err(show)?;
    let review = repo.latest_review(attempt_id).map_err(show)?;

    println!(
        "{}  {}  base {}",
        attempt.id,
        attempt.state,
        short_commit(&attempt.base_commit)
    );
    if let Some(task) = &attempt.task {
        println!("  task: {task}");
    }

    println!("  changes vs base:");
    if changes.is_empty() {
        println!("    (none)");
    }
    for file in &changes {
        let added = file
            .added
            .map(|n| format!("+{n}"))
            .unwrap_or_else(|| "+?".to_string());
        let removed = file
            .removed
            .map(|n| format!("-{n}"))
            .unwrap_or_else(|| "-?".to_string());
        println!("    {added:>7} {removed:>7}  {}", file.path);
    }

    let observed = evidence.iter().filter(|entry| entry.observed).count();
    println!("  decisions: {}", decisions.len());
    println!(
        "  evidence: {observed} observed, {} reported",
        evidence.len() - observed
    );

    match review {
        Some(review) => println!(
            "  review: {} by {} at {}",
            review.verdict,
            review.reviewer,
            short_commit(&review.head_commit)
        ),
        None => println!("  review: none — `kap review {attempt_id} --approve` allows merging"),
    }
    Ok(())
}

fn print_diff(diff: &AttemptDiff) {
    println!("{} -> {}", diff.from.id, diff.to.id);
    if diff.is_empty() {
        println!("No file changes between the two attempts.");
        return;
    }

    for file in &diff.files {
        let added = file
            .added
            .map(|n| format!("+{n}"))
            .unwrap_or_else(|| "+?".to_string());
        let removed = file
            .removed
            .map(|n| format!("-{n}"))
            .unwrap_or_else(|| "-?".to_string());
        println!("  {added:>7} {removed:>7}  {}", file.path);
    }

    let (added, removed) = diff.totals();
    println!("{} file(s) changed, +{added} -{removed}", diff.files.len());
}

fn print_attempt(attempt: &Attempt) {
    println!("{}  {}", attempt.id, attempt.state);
    println!("  label:    {}", attempt.label);
    if let Some(task) = &attempt.task {
        println!("  task:     {task}");
    }
    println!("  branch:   {}", attempt.branch);
    println!("  worktree: {}", attempt.worktree.display());
    println!("  base:     {}", short_commit(&attempt.base_commit));
    println!("  created:  {}", attempt.created_at);
}

fn open_repo() -> Result<Repo, String> {
    let cwd = std::env::current_dir().map_err(show)?;
    Repo::open(&cwd).map_err(show)
}

/// The attempt to operate on: the one asked for, or the one whose worktree we are inside.
fn resolve_attempt(repo: &Repo, requested: Option<&str>) -> Result<Option<String>, String> {
    if let Some(id) = requested {
        return Ok(Some(id.to_string()));
    }
    let cwd = std::env::current_dir().map_err(show)?;
    Ok(attempt_at(repo, &cwd))
}

fn require_attempt(repo: &Repo, requested: Option<&str>) -> Result<String, String> {
    resolve_attempt(repo, requested)?
        .ok_or_else(|| "not inside an attempt worktree; pass --attempt <id>".to_string())
}

fn attempt_at(repo: &Repo, cwd: &Path) -> Option<String> {
    let worktrees = repo.kap_dir().join("worktrees").canonicalize().ok()?;
    let cwd = cwd.canonicalize().ok()?;
    let rest = cwd.strip_prefix(&worktrees).ok()?;
    rest.components()
        .next()?
        .as_os_str()
        .to_str()
        .map(str::to_owned)
}

/// Keep a log line short enough to scan: first line only, capped.
fn summarise(command: &str) -> String {
    let first = command.lines().next().unwrap_or_default();
    let mut text: String = first.chars().take(90).collect();
    if text.len() < command.len() {
        text.push('…');
    }
    text
}

fn time_of(timestamp: &str) -> &str {
    timestamp.get(11..19).unwrap_or(timestamp)
}

fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}

fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(code.clamp(0, 255) as u8)
}

fn show(error: impl std::fmt::Display) -> String {
    error.to_string()
}
