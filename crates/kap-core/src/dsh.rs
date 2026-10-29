use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::{now_rfc3339, ToolEvent};
use crate::repo::Repo;

/// How many events one import brought in.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DshImport {
    pub sessions: usize,
    pub events: usize,
}

/// The state of one session log, kept in the meta table so repeat imports are cheap.
///
/// `mtime` and `size` let an unchanged log be skipped without decompressing it; `lines` is how far
/// into the file the previous import got.
struct Cursor {
    mtime: i64,
    size: u64,
    lines: usize,
}

impl Cursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}", self.mtime, self.size, self.lines)
    }

    fn decode(text: &str) -> Option<Cursor> {
        let mut parts = text.split(':');
        Some(Cursor {
            mtime: parts.next()?.parse().ok()?,
            size: parts.next()?.parse().ok()?,
            lines: parts.next()?.parse().ok()?,
        })
    }
}

/// A tool call waiting for its result.
struct Pending {
    name: String,
    arguments: String,
    time: i64,
}

impl Repo {
    /// Where DSH keeps sessions for this workspace.
    ///
    /// DSH encodes the working directory into the directory name: `/a/b` becomes `--a-b--`.
    pub fn dsh_session_dir(&self) -> PathBuf {
        let home = std::env::var("DSH_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                PathBuf::from(home).join(".dsh")
            });

        let encoded = format!(
            "--{}--",
            self.root()
                .to_string_lossy()
                .trim_start_matches('/')
                .replace('/', "-")
        );
        home.join("sessions").join(encoded)
    }

    /// Session logs for this workspace, newest first.
    pub fn dsh_sessions(&self) -> Result<Vec<PathBuf>> {
        let dir = self.dsh_session_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };

        let mut logs: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let log = entry.path().join("session.v4.jsonl.zstd");
            if !log.is_file() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            logs.push((modified, log));
        }

        logs.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        Ok(logs.into_iter().map(|(_, path)| path).collect())
    }

    /// Import every session log that changed since it was last read.
    pub fn import_dsh_new(&self, attempt_id: &str) -> Result<DshImport> {
        let mut report = DshImport::default();
        for log in self.dsh_sessions()? {
            let imported = self.import_dsh_session(attempt_id, &log)?;
            if imported > 0 {
                report.sessions += 1;
                report.events += imported;
            }
        }
        Ok(report)
    }

    /// Import one session log, skipping the part already imported.
    pub fn import_dsh_session(&self, attempt_id: &str, log: &Path) -> Result<usize> {
        let metadata = fs::metadata(log).map_err(|error| Error::io(log, error))?;
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);

        let key = format!("dsh.cursor.{}", session_key(log));
        let db = self.db()?;
        let cursor = db.meta_get(&key)?.and_then(|text| Cursor::decode(&text));

        if let Some(cursor) = &cursor {
            if cursor.mtime == mtime && cursor.size == size {
                return Ok(0);
            }
        }
        let already = cursor.map(|cursor| cursor.lines).unwrap_or(0);

        let text = read_log(log)?;
        let session_id = session_id_of(log, &text);
        let run = self.run_for_session_as(attempt_id, &session_id, "dsh")?;

        let mut pending: HashMap<String, Pending> = HashMap::new();
        let mut recorded = 0usize;
        let mut lines = 0usize;

        for line in text.lines() {
            lines += 1;
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            match event["type"].as_str() {
                Some("tool/call") => {
                    if let Some(call_id) = event["data"]["callId"].as_str() {
                        pending.insert(
                            call_id.to_string(),
                            Pending {
                                name: event["data"]["name"].as_str().unwrap_or("tool").to_string(),
                                arguments: event["data"]["arguments"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_string(),
                                time: event["time"].as_i64().unwrap_or(0),
                            },
                        );
                    }
                }
                Some("tool/result") => {
                    let Some(call_id) = event["data"]["message"]["toolCallId"].as_str() else {
                        continue;
                    };
                    let Some(call) = pending.remove(call_id) else {
                        continue;
                    };

                    // Only the part of the file that is new gets written.
                    if lines <= already {
                        continue;
                    }

                    let output = result_text(&event["data"]["message"]["content"]);
                    let exit_code = exit_code_of(&output);
                    if self
                        .record_dsh_call(attempt_id, &run.id, call_id, &call, &output, exit_code)?
                    {
                        recorded += 1;
                    }
                }
                _ => {}
            }
        }

        let cursor = Cursor { mtime, size, lines };
        self.db()?.meta_set(&key, &cursor.encode())?;
        Ok(recorded)
    }

    /// Import new sessions automatically if this repository opted in.
    pub fn auto_import_dsh(&self) -> Result<()> {
        let db = self.db()?;
        if db.meta_get("dsh.auto")?.as_deref() != Some("true") {
            return Ok(());
        }
        let Some(attempt_id) = db.meta_get("dsh.attempt")? else {
            return Ok(());
        };
        drop(db);

        self.import_dsh_new(&attempt_id)?;
        Ok(())
    }

    /// Turn the repository's DSH import on or off.
    pub fn set_dsh_auto(&self, enabled: bool, attempt_id: Option<&str>) -> Result<()> {
        let db = self.db()?;
        db.meta_set("dsh.auto", if enabled { "true" } else { "false" })?;
        if let Some(attempt_id) = attempt_id {
            db.attempt(attempt_id)?;
            db.meta_set("dsh.attempt", attempt_id)?;
        }
        Ok(())
    }

    fn record_dsh_call(
        &self,
        attempt_id: &str,
        run_id: &str,
        call_id: &str,
        call: &Pending,
        output: &str,
        exit_code: Option<i32>,
    ) -> Result<bool> {
        let objects = self.objects();
        let is_bash = call.name == "bash";
        let command = if is_bash {
            serde_json::from_str::<serde_json::Value>(&call.arguments)
                .ok()
                .and_then(|value| value["command"].as_str().map(str::to_string))
                .unwrap_or_else(|| call.arguments.clone())
        } else {
            call.name.clone()
        };

        let mut hasher = blake3::Hasher::new();
        hasher.update(attempt_id.as_bytes());
        hasher.update(b"|dsh|");
        hasher.update(call_id.as_bytes());
        let hex = hasher.finalize().to_hex().to_string();

        let stdout = truncate(output.as_bytes().to_vec());
        let event = ToolEvent {
            id: format!("event-{}", &hex[..24]),
            attempt_id: attempt_id.to_string(),
            run_id: Some(run_id.to_string()),
            kind: if is_bash { "command" } else { "tool_call" }.to_string(),
            command: Some(command),
            exit_code,
            stdout_hash: if stdout.is_empty() {
                None
            } else {
                Some(objects.put(&stdout)?)
            },
            stderr_hash: None,
            observed: true,
            occurred_at: timestamp(call.time),
            agent_id: None,
            agent_type: None,
        };

        self.db()?.insert_tool_event_returning(&event)
    }
}

fn session_key(log: &Path) -> String {
    log.parent()
        .and_then(|dir| dir.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

fn session_id_of(log: &Path, text: &str) -> String {
    for line in text.lines() {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            if event["type"] == "session" {
                if let Some(id) = event["id"].as_str() {
                    return id.to_string();
                }
            }
        }
    }
    session_key(log)
}

fn read_log(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|error| Error::io(path, error))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| Error::io(path, error))?;

    let decoded = if path.to_string_lossy().ends_with(".zstd") {
        zstd::stream::decode_all(bytes.as_slice()).map_err(|error| Error::io(path, error))?
    } else {
        bytes
    };

    Ok(String::from_utf8_lossy(&decoded).into_owned())
}

fn result_text(content: &serde_json::Value) -> String {
    let mut out = String::new();
    if let Some(parts) = content.as_array() {
        for part in parts {
            if let Some(text) = part["text"].as_str() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
    }
    out
}

/// DSH appends its own marker to command output, e.g. `[exit code: 128]`.
fn exit_code_of(output: &str) -> Option<i32> {
    let start = output.rfind("[exit code: ")? + "[exit code: ".len();
    let rest = &output[start..];
    let end = rest.find(']')?;
    rest[..end].trim().parse().ok()
}

fn timestamp(millis: i64) -> String {
    if millis <= 0 {
        return now_rfc3339();
    }

    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .ok()
        .and_then(|time| {
            time.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(now_rfc3339)
}

fn truncate(mut bytes: Vec<u8>) -> Vec<u8> {
    const MAX: usize = 8 * 1024 * 1024;
    if bytes.len() > MAX {
        bytes.truncate(MAX);
        bytes.extend_from_slice(b"\n[kap: output truncated]\n");
    }
    bytes
}
