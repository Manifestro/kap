use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kap_core::attempt::NewAttempt;
use kap_core::decision::NewDecision;
use kap_core::model::{Attempt, Decision, ToolEvent};
use kap_core::repo::Repo;

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
            let attempt_id = require_attempt(&repo, attempt.as_deref())?;
            print_log(&repo, &attempt_id)?;
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
    let code = event
        .exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "?".to_string());
    println!(
        "{}  cmd       {command}  -> exit {code}",
        time_of(&event.occurred_at)
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
