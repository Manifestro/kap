use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::{Error, Result};

/// A git repository that KAP records work in.
///
/// KAP drives git through the `git` binary rather than linking libgit2: the operations it needs
/// are exactly the ones the CLI exposes, they behave identically in the terminal and in KAP, and
/// failures are easy to reproduce by hand.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// Find the repository that contains `start`.
    pub fn discover(start: &Path) -> Result<Git> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(start)
            .output()
            .map_err(Error::GitMissing)?;

        if !output.status.success() {
            return Err(Error::NotAGitRepository {
                path: start.to_path_buf(),
            });
        }

        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Git {
            root: PathBuf::from(root),
        })
    }

    /// The repository root of the checkout this handle was discovered from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The main repository root.
    ///
    /// For a linked worktree, `git rev-parse --show-toplevel` returns the worktree itself, but
    /// KAP's store lives in the main repository. The common git directory is the same for both,
    /// so its parent is the answer.
    pub fn main_root(&self) -> Result<PathBuf> {
        let common = PathBuf::from(self.run(&["rev-parse", "--git-common-dir"])?.trim());
        let common = if common.is_absolute() {
            common
        } else {
            self.root.join(common)
        };

        match common.file_name().and_then(|name| name.to_str()) {
            Some(".git") => Ok(common
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone())),
            // A bare repository has no working tree of its own.
            _ => Ok(self.root.clone()),
        }
    }

    /// The commit `HEAD` points at.
    pub fn head_commit(&self) -> Result<String> {
        Ok(self.run(&["rev-parse", "HEAD"])?.trim().to_string())
    }

    /// Resolve any revision to a commit hash.
    pub fn resolve_commit(&self, rev: &str) -> Result<String> {
        let spec = format!("{rev}^{{commit}}");
        Ok(self
            .run(&["rev-parse", "--verify", &spec])?
            .trim()
            .to_string())
    }

    /// Whether a local branch exists.
    pub fn branch_exists(&self, branch: &str) -> Result<bool> {
        let spec = format!("refs/heads/{branch}");
        let output = self.output(&["rev-parse", "--verify", "--quiet", &spec])?;
        Ok(output.status.success())
    }

    /// Create a worktree at `path` on a new branch based on `base`.
    pub fn add_worktree(&self, path: &Path, branch: &str, base: &str) -> Result<()> {
        let path = path.to_string_lossy().to_string();
        self.run(&["worktree", "add", "-b", branch, &path, base])?;
        Ok(())
    }

    /// Remove a worktree, optionally discarding uncommitted changes.
    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<()> {
        let path = path.to_string_lossy().to_string();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(path.as_str());
        self.run(&args)?;
        Ok(())
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let output = self.output(args)?;
        if !output.status.success() {
            return Err(Error::Git {
                command: args.join(" "),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn output(&self, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .map_err(Error::GitMissing)
    }
}
