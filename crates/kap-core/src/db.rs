use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row};

use crate::error::{Error, Result};
use crate::model::{Attempt, AttemptState, Decision, ToolEvent};

/// Schema migrations, applied in order. Index `i` upgrades the database to version `i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_event_outputs.sql"),
];

const ATTEMPT_COLUMNS: &str =
    "id, label, task, state, branch, worktree, base_commit, created_at, closed_at";

/// The SQLite record of work.
///
/// The database is the log of record: every recorded event lands here in a transaction, and rows
/// are never rewritten. Large payloads live in `.kap/objects/` and are referenced by hash.
pub struct Db {
    path: PathBuf,
    conn: Connection,
}

impl Db {
    /// Open the database at `path`, creating it and applying pending migrations if needed.
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path).map_err(|e| Error::db(path, e))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| Error::db(path, e))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| Error::db(path, e))?;

        let db = Db {
            path: path.to_path_buf(),
            conn,
        };
        db.migrate()?;
        Ok(db)
    }

    /// Record a new attempt.
    pub fn insert_attempt(&self, attempt: &Attempt) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO attempts (id, label, task, state, branch, worktree, base_commit, \
                 created_at, closed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    attempt.id,
                    attempt.label,
                    attempt.task,
                    attempt.state.as_str(),
                    attempt.branch,
                    attempt.worktree.to_string_lossy(),
                    attempt.base_commit,
                    attempt.created_at,
                    attempt.closed_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Look up one attempt by id.
    pub fn attempt(&self, id: &str) -> Result<Attempt> {
        let sql = format!("SELECT {ATTEMPT_COLUMNS} FROM attempts WHERE id = ?1");
        self.conn
            .query_row(&sql, [id], attempt_from_row)
            .optional()
            .map_err(|e| Error::db(&self.path, e))?
            .ok_or_else(|| Error::AttemptNotFound { id: id.to_string() })
    }

    /// All attempts, newest first.
    pub fn attempts(&self) -> Result<Vec<Attempt>> {
        let sql =
            format!("SELECT {ATTEMPT_COLUMNS} FROM attempts ORDER BY created_at DESC, id DESC");
        let mut statement = self
            .conn
            .prepare(&sql)
            .map_err(|e| Error::db(&self.path, e))?;
        let rows = statement
            .query_map([], attempt_from_row)
            .map_err(|e| Error::db(&self.path, e))?;

        let mut attempts = Vec::new();
        for row in rows {
            attempts.push(row.map_err(|e| Error::db(&self.path, e))?);
        }
        Ok(attempts)
    }

    /// Mark an attempt closed. Only the lifecycle fields change; the recorded history is untouched.
    pub fn close_attempt(&self, id: &str, closed_at: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE attempts SET state = 'closed', closed_at = ?2 WHERE id = ?1",
                rusqlite::params![id, closed_at],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Record a tool event.
    pub fn insert_tool_event(&self, event: &ToolEvent) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO tool_events (id, attempt_id, kind, command, exit_code, output_hash, \
                 stderr_hash, observed, occurred_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    event.id,
                    event.attempt_id,
                    event.kind,
                    event.command,
                    event.exit_code,
                    event.stdout_hash,
                    event.stderr_hash,
                    event.observed as i64,
                    event.occurred_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Tool events, oldest first, optionally limited to one attempt.
    pub fn tool_events(&self, attempt_id: Option<&str>) -> Result<Vec<ToolEvent>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, attempt_id, kind, command, exit_code, output_hash, stderr_hash, \
                 observed, occurred_at FROM tool_events \
                 WHERE ?1 IS NULL OR attempt_id = ?1 ORDER BY occurred_at ASC, id ASC",
            )
            .map_err(|e| Error::db(&self.path, e))?;

        let rows = statement
            .query_map([attempt_id], |row| {
                Ok(ToolEvent {
                    id: row.get("id")?,
                    attempt_id: row.get("attempt_id")?,
                    kind: row.get("kind")?,
                    command: row.get("command")?,
                    exit_code: row.get("exit_code")?,
                    stdout_hash: row.get("output_hash")?,
                    stderr_hash: row.get("stderr_hash")?,
                    observed: row.get::<_, i64>("observed")? != 0,
                    occurred_at: row.get("occurred_at")?,
                })
            })
            .map_err(|e| Error::db(&self.path, e))?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(|e| Error::db(&self.path, e))?);
        }
        Ok(events)
    }

    /// Record a decision.
    pub fn insert_decision(&self, decision: &Decision) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO decisions (id, attempt_id, summary, rationale, alternatives, source, \
                 created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    decision.id,
                    decision.attempt_id,
                    decision.summary,
                    decision.rationale,
                    decision.alternatives,
                    decision.source,
                    decision.created_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Decisions recorded for an attempt, oldest first.
    pub fn decisions(&self, attempt_id: &str) -> Result<Vec<Decision>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, attempt_id, summary, rationale, alternatives, source, created_at \
                 FROM decisions WHERE attempt_id = ?1 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| Error::db(&self.path, e))?;

        let rows = statement
            .query_map([attempt_id], |row| {
                Ok(Decision {
                    id: row.get("id")?,
                    attempt_id: row.get("attempt_id")?,
                    summary: row.get("summary")?,
                    rationale: row.get("rationale")?,
                    alternatives: row.get("alternatives")?,
                    source: row.get("source")?,
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(|e| Error::db(&self.path, e))?;

        let mut decisions = Vec::new();
        for row in rows {
            decisions.push(row.map_err(|e| Error::db(&self.path, e))?);
        }
        Ok(decisions)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| Error::db(&self.path, e))?;

        for (index, sql) in MIGRATIONS.iter().enumerate() {
            let target = index as i64 + 1;
            if version < target {
                self.conn
                    .execute_batch(sql)
                    .map_err(|e| Error::db(&self.path, e))?;
                self.conn
                    .pragma_update(None, "user_version", target)
                    .map_err(|e| Error::db(&self.path, e))?;
            }
        }
        Ok(())
    }
}

fn attempt_from_row(row: &Row<'_>) -> rusqlite::Result<Attempt> {
    let state: String = row.get("state")?;
    let state = AttemptState::from_db(&state).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown attempt state",
            )),
        )
    })?;

    Ok(Attempt {
        id: row.get("id")?,
        label: row.get("label")?,
        task: row.get("task")?,
        state,
        branch: row.get("branch")?,
        worktree: PathBuf::from(row.get::<_, String>("worktree")?),
        base_commit: row.get("base_commit")?,
        created_at: row.get("created_at")?,
        closed_at: row.get("closed_at")?,
    })
}
