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

    /// Whether the repository has any commit yet.
    pub fn has_head(&self) -> Result<bool> {
        let output = self.output(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
        Ok(output.status.success())
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

    /// Run git inside another working tree of the same repository.
    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map_err(Error::GitMissing)?;

        if !output.status.success() {
            return Err(Error::Git {
                command: args.join(" "),
                stderr: failure_text(&output),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// The commit a local branch points at.
    pub fn branch_head(&self, branch: &str) -> Result<String> {
        let spec = format!("refs/heads/{branch}^{{commit}}");
        Ok(self
            .run(&["rev-parse", "--verify", &spec])?
            .trim()
            .to_string())
    }

    /// The branch currently checked out.
    pub fn current_branch(&self) -> Result<String> {
        Ok(self
            .run(&["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_string())
    }

    /// Merge a branch into the current one, always with a merge commit.
    pub fn merge_no_ff(&self, branch: &str) -> Result<()> {
        let message = format!("Merge {branch}");
        self.run(&["merge", "--no-ff", "-m", &message, branch])?;
        Ok(())
    }

    /// Undo a merge that stopped on a conflict.
    pub fn merge_abort(&self) -> Result<()> {
        self.run(&["merge", "--abort"])?;
        Ok(())
    }

    /// The configured git user name, when there is one.
    pub fn user_name(&self) -> Option<String> {
        self.run(&["config", "user.name"])
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }

    /// Run git inside a working tree with extra environment variables.
    pub fn run_in_with_env(
        &self,
        dir: &Path,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<String> {
        let mut command = Command::new("git");
        command.args(args).current_dir(dir);
        for (key, value) in env {
            command.env(key, value);
        }

        let output = command.output().map_err(Error::GitMissing)?;
        if !output.status.success() {
            return Err(Error::Git {
                command: args.join(" "),
                stderr: failure_text(&output),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Point a ref at a commit.
    pub fn update_ref(&self, reference: &str, commit: &str) -> Result<()> {
        self.run(&["update-ref", reference, commit])?;
        Ok(())
    }

    /// Remote refs matching a pattern, one `sha\tref` line each.
    pub fn ls_remote(&self, remote: &str, pattern: &str) -> Result<String> {
        self.run(&["ls-remote", "--refs", remote, pattern])
    }

    /// Fetch a refspec from a remote.
    pub fn fetch(&self, remote: &str, refspec: &str) -> Result<()> {
        self.run(&["fetch", "--no-tags", remote, refspec])?;
        Ok(())
    }

    /// Push a refspec to a remote.
    pub fn push(&self, remote: &str, refspec: &str) -> Result<()> {
        self.run(&["push", remote, refspec])?;
        Ok(())
    }

    /// Whether a ref exists.
    pub fn ref_exists(&self, reference: &str) -> Result<bool> {
        let output = self.output(&["rev-parse", "--verify", "--quiet", reference])?;
        Ok(output.status.success())
    }

    /// Ref names under a prefix.
    pub fn refs_under(&self, prefix: &str) -> Result<Vec<String>> {
        let out = self.run(&["for-each-ref", "--format=%(refname)", prefix])?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Paths in a revision's tree.
    pub fn ls_tree(&self, rev: &str) -> Result<Vec<String>> {
        let out = self.run(&["ls-tree", "-r", "--name-only", rev])?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// A file in a revision, as text.
    pub fn show_text(&self, spec: &str) -> Result<String> {
        self.run(&["show", spec])
    }

    /// A file in a revision, as bytes.
    pub fn show_bytes(&self, spec: &str) -> Result<Vec<u8>> {
        let output = self.output(&["show", spec])?;
        if !output.status.success() {
            return Err(Error::Git {
                command: format!("show {spec}"),
                stderr: failure_text(&output),
            });
        }
        Ok(output.stdout)
    }

    /// Changed tracked paths, ignoring untracked files.
    ///
    /// A merge does not care about untracked files in general — only about those it would
    /// overwrite, and git itself refuses in that case. Blocking on every stray file made merging
    /// fail for reasons unrelated to the attempt.
    pub fn tracked_changes(&self, dir: &Path) -> Result<String> {
        self.run_in(dir, &["status", "--porcelain", "--untracked-files=no"])
    }

    /// Whether `ancestor` is reachable from `descendant`.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let args = ["merge-base", "--is-ancestor", ancestor, descendant];
        let output = self.output(&args)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(Error::Git {
                command: args.join(" "),
                stderr: failure_text(&output),
            }),
        }
    }

    /// Delete a branch, refusing if it is not merged into the current one.
    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        self.run(&["branch", "-d", branch])?;
        Ok(())
    }

    /// Changed paths in a working tree, including untracked files.
    pub fn status_porcelain(&self, dir: &Path) -> Result<String> {
        self.run_in(dir, &["status", "--porcelain"])
    }

    /// A machine-readable diff between two revisions: one `added\tremoved\tpath` line per file.
    pub fn diff_numstat(&self, from: &str, to: &str) -> Result<String> {
        self.run(&["diff", "--numstat", from, to])
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
                stderr: failure_text(&output),
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

/// Git explains some failures on stdout rather than stderr — `commit` with nothing to commit, for
/// example. Report whichever stream actually carries the reason.
fn failure_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}
