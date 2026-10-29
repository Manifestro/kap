use std::process::Command;

use crate::error::{Error, Result};
use crate::model::{new_id, now_rfc3339, AttemptState, ToolEvent};
use crate::repo::Repo;

/// The result of a command run inside an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Repo {
    /// Run `command` inside an attempt's worktree and record it as a tool event.
    ///
    /// Output is recorded whether the command succeeded or not: a failed run is evidence too.
    pub fn exec(&self, attempt_id: &str, command: &[String]) -> Result<ExecOutput> {
        let (program, args) = command.split_first().ok_or(Error::EmptyCommand)?;

        let attempt = self.db()?.attempt(attempt_id)?;
        if attempt.state != AttemptState::Open {
            return Err(Error::AttemptClosed { id: attempt.id });
        }

        let output = Command::new(program)
            .args(args)
            .current_dir(&attempt.worktree)
            .output()
            .map_err(|e| Error::io(&attempt.worktree, e))?;

        let exit_code = output.status.code().unwrap_or(127);
        self.record_tool_event(
            attempt_id,
            command,
            exit_code,
            &output.stdout,
            &output.stderr,
        )?;

        Ok(ExecOutput {
            exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Record a command and its output as a tool event, storing the payloads as objects.
    pub fn record_tool_event(
        &self,
        attempt_id: &str,
        command: &[String],
        exit_code: i32,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<ToolEvent> {
        let objects = self.objects();

        let event = ToolEvent {
            id: format!("event-{}", new_id()),
            attempt_id: attempt_id.to_string(),
            run_id: None,
            kind: "command".to_string(),
            command: Some(command.join(" ")),
            exit_code: Some(exit_code),
            stdout_hash: Some(objects.put(stdout)?),
            stderr_hash: Some(objects.put(stderr)?),
            observed: true,
            occurred_at: now_rfc3339(),
        };

        self.db()?.insert_tool_event(&event)?;
        Ok(event)
    }

    /// Tool events, oldest first, optionally limited to one attempt.
    pub fn tool_events(&self, attempt_id: Option<&str>) -> Result<Vec<ToolEvent>> {
        self.db()?.tool_events(attempt_id)
    }
}
