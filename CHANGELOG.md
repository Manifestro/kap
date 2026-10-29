# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-10-29

### Added

- `kap install` adds the Claude Code hooks to `~/.claude/settings.json` once per machine, instead of
  a per-worktree `kap hook setup`. It preserves existing settings and other hooks, and
  `kap uninstall` removes only what it added.
- `kap install --settings <path>` writes the hooks into any Claude-compatible settings file, so
  another CLI that fires the same events can be attached without KAP knowing anything about it.
- Recording can be switched off without uninstalling: `KAP_DISABLED=1` for one session, `kap
  disable` and `kap enable` for a repository.
- `kap doctor` checks that recording is actually wired — the hooks exist in a settings file, the
  binary they call still exists, recording is not disabled, and the current directory is one where
  hooks record. It exits non-zero when something is missing.
- `kap status` says whether the hooks are installed and whether recording is disabled here.

### Changed

- A run no longer records which harness produced it. KAP talks to hooks, not to Claude Code, so
  any CLI that emits the same events is equally supported. Migration 0009 drops the column.

### Fixed

- `kap init` outside a repository reported `.` instead of the resolved directory.
- Starting an attempt in a repository with no commits leaked git's "ambiguous argument 'HEAD'";
  it now says that an initial commit is needed.

## [0.1.1] - 2026-10-29

### Fixed

- Merging failed when the checkout held any untracked file, including files unrelated to the
  attempt. Only tracked changes block a merge now; git still refuses when an untracked file would
  be overwritten.
- `kap attempt clean` kept every branch forever. A branch that is merged into the current one is now
  deleted with the worktree; an unmerged branch is kept.
- `kap status` counted only committed changes. It now reports uncommitted and untracked files too.
- `kap status` in the main checkout now says that hooks record nothing there.
- `kap merge` no longer leaves a half-merged tree: a conflicted merge is aborted.
- Three stray files from a test run (`m1bc.txt`, `pc2.txt`, `push2.txt`) were committed by mistake
  and are removed.

- Hooks could fail the agent's turn: malformed or empty input, a missing session id, or a locked
  database produced a non-zero exit that Claude Code surfaced. Hooks now always exit 0 and record
  the failure in `.kap/logs/hooks.log`.
- A tool call delivery could be recorded twice; event ids are derived from the tool call's id, and
  duplicates are ignored by the database.
- SQLite had no busy timeout, so a hook could fail with "database is locked" while another command
  was writing. Writers now wait up to five seconds.
- A malformed object address panicked; it is now an error.
- Command output was buffered whole. Streams are capped at 8 MiB and marked when truncated, and a
  snapshot skips files over 16 MiB.
- A file edit was recorded twice — once by the hook under an absolute path, once by the stop
  snapshot under a relative one. Paths are now relative to the worktree, and evidence is
  deduplicated by content.
- `kap push` with nothing new to export failed with a bare git error; it now reports that there is
  nothing to push.
- Git failures that git explains on stdout — an empty commit, for example — were reported with an
  empty reason.
- Objects were written in place, so a concurrent pull could read a half-written object. They are now
  written beside the target and renamed into place.
- Two pushes from the same machine raced for the same ref and one failed. Pushes are now serialised
  with a lock file, and a lock left by a crashed push is ignored after five minutes.

## [0.1.0] - 2026-10-29

The first working version: an attempt can be opened, recorded, reviewed, merged, snapshotted, and
shared with another machine.

### Added

- `kap init` creates `.kap/` — a SQLite record of work plus content-addressed objects — and keeps it
  out of `git status`.
- `kap attempt start | list | close | clean`: every attempt gets its own branch and git worktree.
- `kap exec` runs a command inside an attempt and records the command, its exit code, and both
  output streams; output is stored by BLAKE3 hash, so identical output is written once.
- `kap decision add | list` records what was decided, why, and what was rejected.
- `kap evidence attach | list` stores an artifact and marks the record observed; a record without an
  artifact is kept as a claim and marked not observed.
- `kap log` shows the timeline of an attempt, or of the whole repository.
- `kap status` shows where you are, what is open, and what the current attempt has recorded.
- `kap diff` compares two attempts by their branches, not their bases.
- `kap review` records a verdict pinned to the commit it looked at; `kap merge` refuses without an
  approval, refuses a stale approval, refuses a dirty tree, and aborts a conflicted merge.
- `kap snapshot create | list | restore` captures the whole worktree, tracked and untracked files
  included, without moving the branch; restoring opens a new attempt instead of overwriting one.
- `kap push` and `kap pull` move history and objects through the git remote, one ref per machine,
  with idempotent import and monotonic lifecycle updates.
- Claude Code adapter through hooks: `kap hook setup` wires `SessionStart`, `PostToolUse`,
  `PostToolUseFailure`, and `Stop`, so KAP listens without owning the agent's loop.
- Append-only SQLite schema with versioned migrations, and a CI job that runs formatting, lints, and
  tests on every push and pull request.

[Unreleased]: https://github.com/Manifestro/kap/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/Manifestro/kap/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Manifestro/kap/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Manifestro/kap/releases/tag/v0.1.0
