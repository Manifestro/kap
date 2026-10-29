use std::fs;
use std::path::Path;
use std::process::Command;

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

        git_in(&staging, &["init", "-q"])?;
        git_in(&staging, &["add", "-A"])?;
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
                &format!("KAP export from {machine}"),
            ],
        )?;
        let commit = git_in(&staging, &["rev-parse", "HEAD"])?.trim().to_string();

        let git = self.git()?;
        let reference = format!("{STORE_PREFIX}{machine}");
        git.fetch(&staging.to_string_lossy(), &format!("{commit}:{reference}"))?;
        git.push(remote, &format!("{reference}:{reference}"))?;

        let _ = fs::remove_dir_all(&staging);
        Ok(SyncReport {
            rows,
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
                report.rows += self.db()?.import_jsonl(&rows)?.rows;
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
                fs::write(&target, bytes).map_err(|error| Error::io(&target, error))?;
                report.objects += 1;
            }

            report.refs += 1;
        }

        Ok(report)
    }
}

fn git_in(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(Error::GitMissing)?;

    if !output.status.success() {
        return Err(Error::Git {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
