use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kap_core::attempt::NewAttempt;
use kap_core::model::Attempt;
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Init { path } => {
            let repo = Repo::init(&path).map_err(|e| e.to_string())?;
            println!("Initialized KAP in {}", repo.kap_dir().display());
            Ok(())
        }
        Command::Attempt { command } => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let repo = Repo::open(&cwd).map_err(|e| e.to_string())?;

            match command {
                AttemptCommand::Start { label, task, base } => {
                    let attempt = repo
                        .start_attempt(NewAttempt {
                            label: &label,
                            task: task.as_deref(),
                            base: base.as_deref(),
                        })
                        .map_err(|e| e.to_string())?;
                    print_attempt(&attempt);
                    Ok(())
                }
                AttemptCommand::List => {
                    let attempts = repo.attempts().map_err(|e| e.to_string())?;
                    if attempts.is_empty() {
                        println!("No attempts yet.");
                        return Ok(());
                    }
                    for attempt in &attempts {
                        println!("{}  {}", attempt.id, attempt.state);
                        println!("  label:    {}", attempt.label);
                        println!("  branch:   {}", attempt.branch);
                        println!("  created:  {}", attempt.created_at);
                    }
                    Ok(())
                }
            }
        }
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

fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}
