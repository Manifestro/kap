use std::fs;
use std::path::{Path, PathBuf};

use crate::db::Db;
use crate::error::{Error, Result};
use crate::git::Git;

/// Name of the directory KAP keeps its state in.
pub const KAP_DIR: &str = ".kap";

const DB_FILE: &str = "kap.db";
const IGNORE_ENTRY: &str = ".kap/";

/// A git repository with KAP initialized in it.
#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Create the `.kap/` store in the repository that contains `path`.
    ///
    /// Fails if KAP is already initialized there, so an existing history is never clobbered by
    /// accident.
    pub fn init(path: &Path) -> Result<Repo> {
        let git = Git::discover(path)?;
        let repo = Repo {
            root: git.root().to_path_buf(),
        };
        let kap = repo.kap_dir();

        if kap.join(DB_FILE).exists() {
            return Err(Error::AlreadyInitialized { path: kap });
        }

        for dir in [kap.clone(), kap.join("objects"), kap.join("worktrees")] {
            fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        }

        Db::open(&kap.join(DB_FILE))?;
        repo.ensure_ignored()?;
        Ok(repo)
    }

    /// Open the KAP repository that contains `path`.
    pub fn open(path: &Path) -> Result<Repo> {
        let git = Git::discover(path)?;
        let repo = Repo {
            root: git.root().to_path_buf(),
        };

        if !repo.db_path().exists() {
            return Err(Error::NotInitialized {
                path: repo.kap_dir(),
            });
        }
        Ok(repo)
    }

    /// The repository root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The `.kap/` directory.
    pub fn kap_dir(&self) -> PathBuf {
        self.root.join(KAP_DIR)
    }

    /// The history database file.
    pub fn db_path(&self) -> PathBuf {
        self.kap_dir().join(DB_FILE)
    }

    /// Open the history database.
    pub fn db(&self) -> Result<Db> {
        Db::open(&self.db_path())
    }

    /// The git repository KAP records into.
    pub fn git(&self) -> Result<Git> {
        Git::discover(&self.root)
    }

    /// Make sure `.kap/` is ignored, so the store never shows up as untracked work.
    fn ensure_ignored(&self) -> Result<()> {
        let path = self.root.join(".gitignore");
        let existing = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error::io(&path, e)),
        };

        let already_ignored = existing
            .lines()
            .any(|line| matches!(line.trim(), ".kap" | ".kap/" | "/.kap" | "/.kap/"));
        if already_ignored {
            return Ok(());
        }

        let mut text = existing;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(IGNORE_ENTRY);
        text.push('\n');

        fs::write(&path, text).map_err(|e| Error::io(&path, e))
    }
}
