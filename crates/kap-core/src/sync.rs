use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::error::{Error, Result};
use crate::model::new_id;
use crate::repo::Repo;

const MACHINE_KEY: &str = "machine_id";
const STORE_PREFIX: &str = "refs/kap/store/";
const REMOTE_PREFIX: &str = "refs/kap/remote/";

/// What a push or pull moved.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    /// History rows that were written or imported.
    pub rows: usize,
    /// Rows that already existed and whose lifecycle moved forward.
    pub updated: usize,
    /// Objects that were transferred.
    pub objects: usize,
    /// Machine refs that were pushed or read.
    pub refs: usize,
}

impl Repo {
    /// This machine's stable id, created on first use.
    ///
    /// Each machine pushes to its own ref, so two people never fight over the same ref and a push
    /// never needs a force.
    pub fn machine_id(&self) -> Result<String> {
        let db = self.db()?;
        if let Some(id) = db.meta_get(MACHINE_KEY)? {
            return Ok(id);
        }

        let id = format!("machine-{}", new_id());
        db.meta_set(MACHINE_KEY, &id)?;
        Ok(id)
    }

    /// Publish this repository's history under a machine ref and push it.
    ///
    /// The export is a throwaway repository holding `rows.jsonl` and the referenced objects as one
    /// commit; git carries it, so there is no server to run and no extra protocol to trust.
    pub fn push(&self, remote: &str) -> Result<SyncReport> {
        // Two concurrent exports would race for the same machine ref and one would fail with a
        // bare git error. Serialise them instead.
        let _lock = self.lock_push()?;
        let machine = self.machine_id()?;
        let jsonl = self.db()?.export_jsonl()?;
        let rows = jsonl.lines().count();

        let staging = self
            .kap_dir()
            .join("tmp")
            .join(format!("export-{}", new_id()));
        fs::create_dir_all(staging.join("objects")).map_err(|error| Error::io(&staging, error))?;
        let rows_path = staging.join("rows.jsonl");
        fs::write(&rows_path, &jsonl).map_err(|error| Error::io(&rows_path, error))?;

        let mut objects = 0;
        for address in self.db()?.object_addresses()? {
            let Ok(source) = self.objects().path(&address) else {
                continue;
            };
            if !source.is_file() {
                continue;
            }

            let hex = address.trim_start_matches("blake3:");
            let target = staging.join("objects").join(&hex[..2]).join(&hex[2..]);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
            }
            fs::copy(&source, &target).map_err(|error| Error::io(&target, error))?;
            objects += 1;
        }

        let git = self.git()?;
        let reference = format!("{STORE_PREFIX}{machine}");

        git_in(&staging, &["init", "-q", "-b", "main"])?;

        // Chain the export onto the previous one, so publishing stays a fast-forward.
        let previous = git.ref_exists(&reference)?;
        let mut parent = None;
        if previous {
            git_in(
                &staging,
                &[
                    "fetch",
                    "-q",
                    &self.root().to_string_lossy(),
                    &format!("+{reference}:refs/kap/parent"),
                ],
            )?;
            let sha = git_in(&staging, &["rev-parse", "refs/kap/parent"])?
                .trim()
                .to_string();
            git_in(&staging, &["update-ref", "refs/heads/main", &sha])?;
            parent = Some(sha);
        }

        git_in(&staging, &["add", "-A"])?;
        let tree = git_in(&staging, &["write-tree"])?.trim().to_string();

        let unchanged = match &parent {
            Some(parent) => {
                let spec = format!("{parent}^{{tree}}");
                git_in(&staging, &["rev-parse", &spec])?.trim() == tree
            }
            None => false,
        };

        let commit = if unchanged {
            // Nothing new since the last export: publishing would be a no-op.
            let _ = fs::remove_dir_all(&staging);
            return Ok(SyncReport {
                rows: 0,
                updated: 0,
                objects: 0,
                refs: 1,
            });
        } else {
            let message = format!("KAP export from {machine}");
            git_in(
                &staging,
                &[
                    "-c",
                    "user.name=KAP",
                    "-c",
                    "user.email=kap@localhost",
                    "commit",
                    "-q",
                    "-m",
                    &message,
                ],
            )?;
            git_in(&staging, &["rev-parse", "HEAD"])?.trim().to_string()
        };

        git.fetch(&staging.to_string_lossy(), &format!("{commit}:{reference}"))?;
        git.push(remote, &format!("{reference}:{reference}"))?;

        let _ = fs::remove_dir_all(&staging);
        Ok(SyncReport {
            rows,
            updated: 0,
            objects,
            refs: 1,
        })
    }

    /// Bring in history pushed by other machines.
    ///
    /// Import is idempotent: rows are keyed by globally unique ids, so pulling twice changes
    /// nothing the second time.
    pub fn pull(&self, remote: &str) -> Result<SyncReport> {
        let git = self.git()?;
        if git.ls_remote(remote, "refs/kap/store/*")?.trim().is_empty() {
            return Ok(SyncReport::default());
        }

        git.fetch(remote, &format!("+{STORE_PREFIX}*:{REMOTE_PREFIX}*"))?;

        let mut report = SyncReport::default();
        for reference in git.refs_under(REMOTE_PREFIX)? {
            if let Ok(rows) = git.show_text(&format!("{reference}:rows.jsonl")) {
                let stats = self.db()?.import_jsonl(&rows)?;
                report.rows += stats.rows;
                report.updated += stats.updated;
            }

            for path in git.ls_tree(&reference)? {
                let Some(relative) = path.strip_prefix("objects/") else {
                    continue;
                };

                let target = self.kap_dir().join("objects").join(relative);
                if target.exists() {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
                }

                let bytes = git.show_bytes(&format!("{reference}:{path}"))?;
                let staging = target.with_extension(format!("tmp-{}", std::process::id()));
                fs::write(&staging, bytes).map_err(|error| Error::io(&staging, error))?;
                fs::rename(&staging, &target).map_err(|error| Error::io(&target, error))?;
                report.objects += 1;
            }

            report.refs += 1;
        }

        Ok(report)
    }
}

/// A lock file that is removed when the push ends, including on an early return.
struct PushLock {
    path: PathBuf,
}

impl Drop for PushLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Repo {
    fn lock_push(&self) -> Result<PushLock> {
        let path = self.kap_dir().join("tmp").join("push.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
        }

        for _ in 0..600 {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(PushLock { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(Error::io(&path, error)),
            }
        }

        Err(Error::SyncBusy { path })
    }
}

/// A lock left behind by a crashed push should not block forever.
fn is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .map(|age| age.as_secs() > 300)
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn git_in(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(Error::GitMissing)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(Error::Git {
            command: args.join(" "),
            stderr: if stderr.is_empty() { stdout } else { stderr },
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
