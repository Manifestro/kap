use std::fs;
use std::path::{Component, Path};
use std::process::Command;

use crate::error::{Error, Result};
use crate::model::Attempt;
use crate::repo::Repo;

/// File in `.kap/` that says how to make a new worktree usable.
pub const WORKTREE_CONFIG: &str = "worktree";

/// How a fresh attempt worktree is made usable.
///
/// A worktree holds only what git tracks. Datasets, checkpoints, and environments are ignored by
/// design, so a new worktree cannot run the project's tests or scripts. `.kap/worktree` closes the
/// gap, one directive per line:
///
/// ```text
/// link data            # symlink the main checkout's data/ into the worktree
/// disposable .venv     # made by setup; deleted with the worktree without asking
/// setup uv sync --frozen
/// ```
///
/// The file lives in `.kap/`, which is not versioned: it describes this machine, not the project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeConfig {
    /// Paths shared with the main checkout through a symlink.
    pub links: Vec<String>,
    /// Paths a worktree may create that are safe to delete with it.
    pub disposable: Vec<String>,
    /// Shell commands run in the worktree after it is created, in order.
    pub setup: Vec<String>,
}

impl WorktreeConfig {
    /// Parse the directives in `text`.
    pub fn parse(text: &str) -> Result<WorktreeConfig> {
        let mut config = WorktreeConfig::default();

        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |message: &str| Error::WorktreeConfig {
                line: index + 1,
                message: message.to_string(),
            };

            let (directive, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let value = value.trim();
            if value.is_empty() {
                return Err(bad(&format!("`{directive}` needs a value")));
            }

            match directive {
                "link" | "disposable" => {
                    let path = value.split(" #").next().unwrap_or(value).trim();
                    let path = path.trim_end_matches('/');
                    if !is_plain_relative(path) {
                        return Err(bad(
                            "paths must be relative to the repository and must not use `..`",
                        ));
                    }
                    if directive == "link" {
                        config.links.push(path.to_string());
                    } else {
                        config.disposable.push(path.to_string());
                    }
                }
                "setup" => config.setup.push(value.to_string()),
                other => return Err(bad(&format!("unknown directive `{other}`"))),
            }
        }

        Ok(config)
    }

    /// Whether `path`, relative to the worktree, is a link or disposable path or lies inside one.
    pub fn owns(&self, path: &str) -> bool {
        let path = path.trim_end_matches('/');
        self.links
            .iter()
            .chain(&self.disposable)
            .any(|owned| path == owned || path.starts_with(&format!("{owned}/")))
    }
}

/// What preparing a worktree did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrepareReport {
    /// Links created.
    pub linked: Vec<String>,
    /// Links skipped because the main checkout does not have the path.
    pub missing: Vec<String>,
    /// Links skipped because the worktree already has something at the path.
    pub present: Vec<String>,
    /// Setup commands and their exit codes, in order. A failure stops the rest.
    pub setup: Vec<(String, i32)>,
}

impl PrepareReport {
    /// Whether every setup command succeeded.
    pub fn setup_ok(&self) -> bool {
        self.setup.iter().all(|(_, code)| *code == 0)
    }
}

impl Repo {
    /// The worktree configuration, or an empty one when `.kap/worktree` does not exist.
    pub fn worktree_config(&self) -> Result<WorktreeConfig> {
        let path = self.kap_dir().join(WORKTREE_CONFIG);
        match fs::read_to_string(&path) {
            Ok(text) => WorktreeConfig::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(WorktreeConfig::default())
            }
            Err(error) => Err(Error::io(&path, error)),
        }
    }

    /// Link shared paths into an attempt's worktree and run its setup commands.
    ///
    /// Setup runs are recorded as commands of the attempt, so a broken environment shows up in its
    /// history like any other failure. Running it again is safe: existing paths are left alone.
    pub fn prepare_worktree(&self, attempt: &Attempt, run_setup: bool) -> Result<PrepareReport> {
        let config = self.worktree_config()?;
        let mut report = PrepareReport::default();

        for path in &config.links {
            let target = self.root().join(path);
            let link = attempt.worktree.join(path);
            if fs::symlink_metadata(&link).is_ok() {
                report.present.push(path.clone());
                continue;
            }
            if !target.exists() {
                report.missing.push(path.clone());
                continue;
            }
            if let Some(parent) = link.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            symlink(&target, &link)?;
            report.linked.push(path.clone());
        }

        if run_setup {
            for command in &config.setup {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&attempt.worktree)
                    .env("KAP_ROOT", self.root())
                    .env("KAP_WORKTREE", &attempt.worktree)
                    .env("KAP_ATTEMPT", &attempt.id)
                    .output()
                    .map_err(|e| Error::io(&attempt.worktree, e))?;
                let code = output.status.code().unwrap_or(127);
                self.record_tool_event(
                    &attempt.id,
                    &[format!("kap setup: {command}")],
                    code,
                    &output.stdout,
                    &output.stderr,
                )?;
                report.setup.push((command.clone(), code));
                if code != 0 {
                    break;
                }
            }
        }

        Ok(report)
    }

    /// Remove the links `.kap/worktree` created, so nothing that deletes the worktree can reach
    /// through them into the main checkout.
    pub(crate) fn unlink_shared(&self, attempt: &Attempt, config: &WorktreeConfig) -> Result<()> {
        for path in &config.links {
            let link = attempt.worktree.join(path);
            if fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink()) {
                fs::remove_file(&link).map_err(|e| Error::io(&link, e))?;
            }
        }
        Ok(())
    }
}

fn is_plain_relative(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).map_err(|e| Error::io(link, e))
}

#[cfg(not(unix))]
fn symlink(target: &Path, link: &Path) -> Result<()> {
    let result = if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    result.map_err(|e| Error::io(link, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_directives_and_comments() {
        let config = WorktreeConfig::parse(
            "# dauys\nlink data/\nlink checkpoints  # weights\ndisposable .venv\nsetup uv sync --frozen\n",
        )
        .expect("parse");
        assert_eq!(config.links, ["data", "checkpoints"]);
        assert_eq!(config.disposable, [".venv"]);
        assert_eq!(config.setup, ["uv sync --frozen"]);
    }

    #[test]
    fn rejects_paths_that_escape_the_repository() {
        for text in [
            "link ../secrets",
            "link /etc",
            "disposable a/../../b",
            "link",
        ] {
            assert!(WorktreeConfig::parse(text).is_err(), "accepted: {text}");
        }
        assert!(WorktreeConfig::parse("mount data").is_err());
    }

    #[test]
    fn owns_paths_inside_links_and_disposables() {
        let config = WorktreeConfig::parse("link data\ndisposable .venv").expect("parse");
        assert!(config.owns("data"));
        assert!(config.owns("data/"));
        assert!(config.owns(".venv/lib/x.py"));
        assert!(!config.owns("database.py"));
        assert!(!config.owns("reports/x.md"));
    }
}
