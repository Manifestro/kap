use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row};

use crate::error::{Error, Result};
use crate::model::{Attempt, AttemptState, Decision, Evidence, Review, Run, Snapshot, ToolEvent};

/// Every table and its columns, in an order safe for import: referenced rows come first.
const TABLES: &[(&str, &[&str])] = &[
    (
        "attempts",
        &[
            "id",
            "label",
            "task",
            "state",
            "branch",
            "worktree",
            "base_commit",
            "created_at",
            "closed_at",
        ],
    ),
    (
        "runs",
        &[
            "id",
            "attempt_id",
            "agent",
            "session_id",
            "status",
            "started_at",
            "finished_at",
        ],
    ),
    (
        "tool_events",
        &[
            "id",
            "attempt_id",
            "run_id",
            "kind",
            "command",
            "exit_code",
            "output_hash",
            "stderr_hash",
            "observed",
            "occurred_at",
        ],
    ),
    (
        "decisions",
        &[
            "id",
            "attempt_id",
            "summary",
            "rationale",
            "alternatives",
            "source",
            "created_at",
        ],
    ),
    (
        "evidence",
        &[
            "id",
            "attempt_id",
            "decision_id",
            "source",
            "command",
            "exit_code",
            "artifact_hash",
            "path",
            "observed",
            "complete",
            "recorded_at",
        ],
    ),
    (
        "snapshots",
        &["id", "attempt_id", "commit_id", "note", "created_at"],
    ),
    (
        "reviews",
        &[
            "id",
            "attempt_id",
            "verdict",
            "reviewer",
            "head_commit",
            "note",
            "created_at",
        ],
    ),
    ("meta", &["key", "value"]),
];

/// What an import brought in.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportStats {
    pub rows: usize,
}

/// Schema migrations, applied in order. Index `i` upgrades the database to version `i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_event_outputs.sql"),
    include_str!("../migrations/0003_run_sessions.sql"),
    include_str!("../migrations/0004_evidence_paths.sql"),
    include_str!("../migrations/0005_reviews.sql"),
    include_str!("../migrations/0006_snapshots.sql"),
    include_str!("../migrations/0007_meta.sql"),
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
                "INSERT INTO tool_events (id, attempt_id, run_id, kind, command, exit_code, \
                 output_hash, stderr_hash, observed, occurred_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    event.id,
                    event.attempt_id,
                    event.run_id,
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
                "SELECT id, attempt_id, run_id, kind, command, exit_code, output_hash, \
                 stderr_hash, observed, occurred_at FROM tool_events \
                 WHERE ?1 IS NULL OR attempt_id = ?1 ORDER BY occurred_at ASC, id ASC",
            )
            .map_err(|e| Error::db(&self.path, e))?;

        let rows = statement
            .query_map([attempt_id], |row| {
                Ok(ToolEvent {
                    id: row.get("id")?,
                    attempt_id: row.get("attempt_id")?,
                    run_id: row.get("run_id")?,
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

    /// Record an evidence row.
    pub fn insert_evidence(&self, evidence: &Evidence) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO evidence (id, attempt_id, decision_id, source, command, exit_code, \
                 artifact_hash, path, observed, complete, recorded_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    evidence.id,
                    evidence.attempt_id,
                    evidence.decision_id,
                    evidence.source,
                    evidence.command,
                    evidence.exit_code,
                    evidence.artifact_hash,
                    evidence.path,
                    evidence.observed as i64,
                    evidence.complete as i64,
                    evidence.recorded_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Evidence recorded for an attempt, oldest first.
    pub fn evidence(&self, attempt_id: &str) -> Result<Vec<Evidence>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, attempt_id, decision_id, source, command, exit_code, artifact_hash, \
                 path, observed, complete, recorded_at FROM evidence \
                 WHERE attempt_id = ?1 ORDER BY recorded_at ASC, id ASC",
            )
            .map_err(|e| Error::db(&self.path, e))?;

        let rows = statement
            .query_map([attempt_id], |row| {
                Ok(Evidence {
                    id: row.get("id")?,
                    attempt_id: row.get("attempt_id")?,
                    decision_id: row.get("decision_id")?,
                    source: row.get("source")?,
                    command: row.get("command")?,
                    exit_code: row.get("exit_code")?,
                    artifact_hash: row.get("artifact_hash")?,
                    path: row.get("path")?,
                    observed: row.get::<_, i64>("observed")? != 0,
                    complete: row.get::<_, i64>("complete")? != 0,
                    recorded_at: row.get("recorded_at")?,
                })
            })
            .map_err(|e| Error::db(&self.path, e))?;

        let mut evidence = Vec::new();
        for row in rows {
            evidence.push(row.map_err(|e| Error::db(&self.path, e))?);
        }
        Ok(evidence)
    }

    /// Record a run.
    pub fn insert_run(&self, run: &Run) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO runs (id, attempt_id, agent, session_id, status, started_at, \
                 finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    run.id,
                    run.attempt_id,
                    run.agent,
                    run.session_id,
                    run.status,
                    run.started_at,
                    run.finished_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// The run belonging to a harness session.
    pub fn run_by_session(&self, session_id: &str) -> Result<Option<Run>> {
        let run = self
            .conn
            .query_row(
                "SELECT id, attempt_id, agent, session_id, status, started_at, finished_at \
                 FROM runs WHERE session_id = ?1",
                [session_id],
                |row| {
                    Ok(Run {
                        id: row.get("id")?,
                        attempt_id: row.get("attempt_id")?,
                        agent: row.get("agent")?,
                        session_id: row.get("session_id")?,
                        status: row.get("status")?,
                        started_at: row.get("started_at")?,
                        finished_at: row.get("finished_at")?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(run)
    }

    /// Mark a session's run as finished.
    pub fn finish_run(&self, session_id: &str, status: &str, finished_at: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE runs SET status = ?2, finished_at = ?3 WHERE session_id = ?1",
                rusqlite::params![session_id, status, finished_at],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// The hash most recently recorded for a path in this attempt.
    pub fn last_evidence_hash(&self, attempt_id: &str, path: &str) -> Result<Option<String>> {
        let hash = self
            .conn
            .query_row(
                "SELECT artifact_hash FROM evidence \
                 WHERE attempt_id = ?1 AND path = ?2 AND artifact_hash IS NOT NULL \
                 ORDER BY recorded_at DESC, id DESC LIMIT 1",
                rusqlite::params![attempt_id, path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(hash)
    }

    /// Record a review verdict.
    pub fn insert_review(&self, review: &Review) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO reviews (id, attempt_id, verdict, reviewer, head_commit, note, \
                 created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    review.id,
                    review.attempt_id,
                    review.verdict,
                    review.reviewer,
                    review.head_commit,
                    review.note,
                    review.created_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// The most recent review of an attempt.
    pub fn latest_review(&self, attempt_id: &str) -> Result<Option<Review>> {
        let review = self
            .conn
            .query_row(
                "SELECT id, attempt_id, verdict, reviewer, head_commit, note, created_at \
                 FROM reviews WHERE attempt_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 1",
                [attempt_id],
                |row| {
                    Ok(Review {
                        id: row.get("id")?,
                        attempt_id: row.get("attempt_id")?,
                        verdict: row.get("verdict")?,
                        reviewer: row.get("reviewer")?,
                        head_commit: row.get("head_commit")?,
                        note: row.get("note")?,
                        created_at: row.get("created_at")?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(review)
    }

    /// Record a snapshot.
    pub fn insert_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO snapshots (id, attempt_id, commit_id, note, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    snapshot.id,
                    snapshot.attempt_id,
                    snapshot.commit,
                    snapshot.note,
                    snapshot.created_at,
                ],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Snapshots of an attempt, oldest first.
    pub fn snapshots(&self, attempt_id: &str) -> Result<Vec<Snapshot>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, attempt_id, commit_id, note, created_at FROM snapshots \
                 WHERE attempt_id = ?1 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| Error::db(&self.path, e))?;

        let rows = statement
            .query_map([attempt_id], |row| {
                Ok(Snapshot {
                    id: row.get("id")?,
                    attempt_id: row.get("attempt_id")?,
                    commit: row.get("commit_id")?,
                    note: row.get("note")?,
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(|e| Error::db(&self.path, e))?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(row.map_err(|e| Error::db(&self.path, e))?);
        }
        Ok(snapshots)
    }

    /// One snapshot by id.
    pub fn snapshot(&self, id: &str) -> Result<Snapshot> {
        self.conn
            .query_row(
                "SELECT id, attempt_id, commit_id, note, created_at FROM snapshots WHERE id = ?1",
                [id],
                |row| {
                    Ok(Snapshot {
                        id: row.get("id")?,
                        attempt_id: row.get("attempt_id")?,
                        commit: row.get("commit_id")?,
                        note: row.get("note")?,
                        created_at: row.get("created_at")?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::db(&self.path, e))?
            .ok_or_else(|| Error::SnapshotNotFound { id: id.to_string() })
    }

    /// Read a value from the meta table.
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| Error::db(&self.path, e))
    }

    /// Write a value into the meta table.
    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )
            .map_err(|e| Error::db(&self.path, e))?;
        Ok(())
    }

    /// Every row as one JSON object per line, tagged with its table.
    pub fn export_jsonl(&self) -> Result<String> {
        let mut out = String::new();

        for (table, columns) in TABLES {
            let pairs = columns
                .iter()
                .map(|column| format!("'{column}', {column}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!("SELECT json_object('table', '{table}', {pairs}) FROM {table}");

            let mut statement = self
                .conn
                .prepare(&sql)
                .map_err(|e| Error::db(&self.path, e))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| Error::db(&self.path, e))?;

            for row in rows {
                out.push_str(&row.map_err(|e| Error::db(&self.path, e))?);
                out.push('\n');
            }
        }

        Ok(out)
    }

    /// Import rows from [`Db::export_jsonl`] output, skipping rows that already exist.
    ///
    /// Rows are keyed by their ids, which are globally unique, so importing is idempotent and two
    /// machines can exchange history in either order.
    pub fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats> {
        let mut stats = ImportStats::default();

        for line in jsonl.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let row: serde_json::Value =
                serde_json::from_str(line).map_err(|source| Error::HookPayload { source })?;
            let table = row
                .get("table")
                .and_then(|value| value.as_str())
                .ok_or_else(|| Error::SyncRow {
                    reason: "row has no table".to_string(),
                })?;

            let columns = TABLES
                .iter()
                .find(|(name, _)| *name == table)
                .map(|(_, columns)| *columns)
                .ok_or_else(|| Error::SyncRow {
                    reason: format!("unknown table `{table}`"),
                })?;

            let placeholders = (1..=columns.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "INSERT OR IGNORE INTO {table} ({}) VALUES ({placeholders})",
                columns.join(", ")
            );

            let values: Vec<Box<dyn rusqlite::ToSql>> = columns
                .iter()
                .map(|column| boxed_value(&row[*column]))
                .collect();
            let params: Vec<&dyn rusqlite::ToSql> = values
                .iter()
                .map(|value| value.as_ref() as &dyn rusqlite::ToSql)
                .collect();

            let inserted = self
                .conn
                .execute(&sql, params.as_slice())
                .map_err(|e| Error::db(&self.path, e))?;
            stats.rows += inserted;
        }

        Ok(stats)
    }

    /// Object addresses referenced anywhere in the history.
    pub fn object_addresses(&self) -> Result<Vec<String>> {
        let mut addresses = Vec::new();
        let mut statement = self
            .conn
            .prepare(
                "SELECT output_hash FROM tool_events WHERE output_hash IS NOT NULL \
                 UNION SELECT stderr_hash FROM tool_events WHERE stderr_hash IS NOT NULL \
                 UNION SELECT artifact_hash FROM evidence WHERE artifact_hash IS NOT NULL",
            )
            .map_err(|e| Error::db(&self.path, e))?;

        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| Error::db(&self.path, e))?;
        for row in rows {
            addresses.push(row.map_err(|e| Error::db(&self.path, e))?);
        }
        Ok(addresses)
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

fn boxed_value(value: &serde_json::Value) -> Box<dyn rusqlite::ToSql> {
    match value {
        serde_json::Value::String(text) => Box::new(text.clone()),
        serde_json::Value::Number(number) => match number.as_i64() {
            Some(number) => Box::new(number),
            None => Box::new(number.as_f64()),
        },
        _ => Box::new(Option::<String>::None),
    }
}
