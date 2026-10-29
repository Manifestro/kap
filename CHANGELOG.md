# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
