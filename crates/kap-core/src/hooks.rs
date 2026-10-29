use serde::Deserialize;

use crate::error::{Error, Result};
use crate::model::{new_id, now_rfc3339, Attempt, Evidence, Run, ToolEvent};
use crate::repo::Repo;

/// Largest payload stored for one output stream. Anything larger is cut and marked, so a runaway
/// command cannot exhaust memory or fill the disk mid-session.
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;

/// Largest file a working-tree snapshot will read.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// One tool call's worth of Claude Code hook input.
///
/// Only the fields KAP uses are modelled and every one of them is optional: a newer Claude Code
/// must not be able to break recording by changing or dropping a field.
#[derive(Debug, Clone, Deserialize)]
pub struct HookInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub hook_event_name: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl HookInput {
    /// Parse hook input.
    pub fn parse(json: &str) -> Result<HookInput> {
        serde_json::from_str(json).map_err(|source| Error::HookPayload { source })
    }

    /// A session id that is always usable, even when the harness did not send one.
    pub fn session(&self) -> &str {
        if self.session_id.is_empty() {
            "unknown-session"
        } else {
            &self.session_id
        }
    }
}

/// The agent name recorded for Claude Code hook sessions.
pub const CLAUDE_CODE: &str = "claude-code";

impl Repo {
    /// The run for a harness session, created on first sight.
    ///
    /// A run outlives individual hook events: every tool call in a session belongs to it, even if
    /// the session started before the hooks were installed.
    pub fn run_for_session(&self, attempt_id: &str, session_id: &str) -> Result<Run> {
        let db = self.db()?;
        if let Some(run) = db.run_by_session(session_id)? {
            return Ok(run);
        }

        let run = Run {
            id: format!("run-{}", new_id()),
            attempt_id: attempt_id.to_string(),
            agent: CLAUDE_CODE.to_string(),
            session_id: Some(session_id.to_string()),
            status: "running".to_string(),
            started_at: now_rfc3339(),
            finished_at: None,
        };
        db.insert_run(&run)?;
        Ok(run)
    }

    /// Finish the run for a session, if one was started.
    pub fn finish_run_for_session(&self, session_id: &str, status: &str) -> Result<()> {
        self.db()?.finish_run(session_id, status, &now_rfc3339())
    }

    /// Record what a hook observed.
    ///
    /// Returns `None` when the event carries nothing KAP can attest to. Commands keep their output
    /// as objects; file edits additionally produce evidence, because a file's content is something
    /// KAP can hash and check later.
    pub fn record_hook_event(
        &self,
        attempt: &Attempt,
        run_id: &str,
        input: &HookInput,
        failed: bool,
    ) -> Result<Option<ToolEvent>> {
        let Some(tool) = input.tool_name.as_deref() else {
            return Ok(None);
        };

        let objects = self.objects();
        let (kind, mut command, exit_code, stdout, mut stderr) = describe(tool, input, failed);

        if let Some(error) = &input.error {
            if !stderr.is_empty() {
                stderr.push(b'\n');
            }
            stderr.extend_from_slice(error.as_bytes());
        }

        let mut artifact_hash = None;
        let mut path = None;
        if is_file_write(tool) {
            if let Some(file) = input
                .tool_input
                .get("file_path")
                .and_then(|value| value.as_str())
            {
                let relative = relative_to(&attempt.worktree, file);
                command = format!("{tool} {relative}");
                path = Some(relative);
                if let Ok(metadata) = std::fs::metadata(file) {
                    if metadata.len() <= MAX_FILE_BYTES {
                        if let Ok(bytes) = std::fs::read(file) {
                            artifact_hash = Some(objects.put(&bytes)?);
                        }
                    }
                }
            }
        }

        let stdout = truncate(stdout);
        let stderr = truncate(stderr);

        let event = ToolEvent {
            id: event_id(attempt, input),
            attempt_id: attempt.id.clone(),
            run_id: Some(run_id.to_string()),
            kind: kind.to_string(),
            command: Some(command.clone()),
            exit_code,
            stdout_hash: match artifact_hash.clone() {
                Some(hash) => Some(hash),
                None if stdout.is_empty() => None,
                None => Some(objects.put(&stdout)?),
            },
            stderr_hash: if stderr.is_empty() {
                None
            } else {
                Some(objects.put(&stderr)?)
            },
            observed: true,
            occurred_at: now_rfc3339(),
        };

        let db = self.db()?;
        db.insert_tool_event(&event)?;

        if let Some(hash) = artifact_hash {
            db.insert_evidence(&Evidence {
                id: format!("evidence-{}", new_id()),
                attempt_id: attempt.id.clone(),
                decision_id: None,
                source: "file_change".to_string(),
                command: Some(command),
                exit_code: None,
                artifact_hash: Some(hash),
                path,
                observed: true,
                complete: true,
                recorded_at: now_rfc3339(),
            })?;
        }

        Ok(Some(event))
    }

    /// A briefing for an agent starting a session in this attempt.
    ///
    /// The point is that the next agent does not start cold: it sees what was already tried and
    /// what was already decided.
    pub fn session_context(&self, attempt: &Attempt) -> Result<String> {
        let db = self.db()?;
        let decisions = db.decisions(&attempt.id)?;
        let attempts = db.attempts()?;

        let mut out = String::new();
        out.push_str(&format!(
            "KAP attempt {} is open in this worktree, on branch {}. ",
            attempt.id, attempt.branch
        ));
        if let Some(task) = &attempt.task {
            out.push_str(&format!("The task it serves: {task}. "));
        }
        out.push('\n');

        let earlier: Vec<_> = attempts
            .iter()
            .filter(|other| other.id != attempt.id)
            .take(3)
            .collect();
        if !earlier.is_empty() {
            out.push_str("\nEarlier attempts in this repository:\n");
            for other in earlier {
                out.push_str(&format!(
                    "- {} ({}) — {}\n",
                    other.id, other.state, other.label
                ));
            }
        }

        if !decisions.is_empty() {
            out.push_str("\nDecisions already recorded for this attempt:\n");
            for decision in &decisions {
                out.push_str(&format!("- {}", decision.summary));
                if let Some(rationale) = &decision.rationale {
                    out.push_str(&format!(" — {rationale}"));
                }
                out.push('\n');
            }
        }

        out.push_str(
            "\nRecord a choice with `kap decision add --summary ... --rationale ...`, attach a \
             file as evidence with `kap evidence attach --file ... --decision ...`, and run \
             commands through `kap exec -- <command>` so their output is kept.",
        );
        Ok(out)
    }
}

/// A tool call's id, derived from the call itself when the harness identifies it.
///
/// Claude Code delivers a tool call once, but a retry or a duplicated hook must not double the
/// history: the same call maps to the same row, and the insert ignores it the second time.
fn event_id(attempt: &Attempt, input: &HookInput) -> String {
    match input.tool_use_id.as_deref() {
        Some(tool_use_id) if !tool_use_id.is_empty() => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(attempt.id.as_bytes());
            hasher.update(b"|");
            hasher.update(input.session().as_bytes());
            hasher.update(b"|");
            hasher.update(tool_use_id.as_bytes());
            let hex = hasher.finalize().to_hex().to_string();
            format!("event-{}", &hex[..24])
        }
        _ => format!("event-{}", new_id()),
    }
}

/// Paths are stored relative to the attempt's worktree: they survive a move to another machine and
/// an edit recorded by a hook deduplicates against the same file seen by a later snapshot.
fn relative_to(worktree: &std::path::Path, file: &str) -> String {
    std::path::Path::new(file)
        .strip_prefix(worktree)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file.to_string())
}

fn truncate(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() > MAX_STREAM_BYTES {
        bytes.truncate(MAX_STREAM_BYTES);
        bytes.extend_from_slice(b"\n[kap: output truncated]\n");
    }
    bytes
}

fn is_file_write(tool: &str) -> bool {
    matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit")
}

/// Turn a tool call into the pieces KAP stores.
fn describe(
    tool: &str,
    input: &HookInput,
    failed: bool,
) -> (&'static str, String, Option<i32>, Vec<u8>, Vec<u8>) {
    match tool {
        "Bash" => {
            let command = input
                .tool_input
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();

            let stdout = output_stream(&input.tool_response, "stdout");
            let stderr = output_stream(&input.tool_response, "stderr");

            // Claude Code routes a failing command to PostToolUseFailure, so a success event is a
            // zero exit. A failure event carries no exit code, and KAP does not invent one.
            let exit_code = if failed { None } else { Some(0) };
            ("command", command, exit_code, stdout, stderr)
        }
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let path = input
                .tool_input
                .get("file_path")
                .and_then(|value| value.as_str())
                .unwrap_or("<unknown path>");
            (
                "file_change",
                format!("{tool} {path}"),
                None,
                Vec::new(),
                Vec::new(),
            )
        }
        other => ("tool_call", other.to_string(), None, Vec::new(), Vec::new()),
    }
}

fn output_stream(response: &serde_json::Value, field: &str) -> Vec<u8> {
    response
        .get(field)
        .and_then(|value| value.as_str())
        .map(|text| text.as_bytes().to_vec())
        .unwrap_or_default()
}
