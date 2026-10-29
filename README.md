# KAP

**An open-source versioning layer for work done by AI agents — working today with Claude Code.**

Git records *what* changed. KAP records *why*.

[![CI](https://github.com/Manifestro/kap/actions/workflows/ci.yml/badge.svg)](https://github.com/Manifestro/kap/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/kap-cli.svg)](https://crates.io/crates/kap-cli)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> **Status: pre-alpha.** The loop works end to end with Claude Code: attempts, recorded commands and
> decisions, evidence, snapshots, review, and merge. The data model and the CLI are still settling,
> and there are no releases yet — but everything described below runs today.

## Today, and what is planned

KAP is aimed at agents working on the same problem together. That is the goal; this is what the
current version actually does.

| | Today | Planned |
| --- | --- | --- |
| **Harnesses** | Claude Code | Codex and other agents |
| **Agents per repository** | Several attempts at once, each in its own worktree, each working alone | Agents cooperating on one task |
| **Review** | `kap review` — whoever runs it approves; nothing distinguishes a person from an agent | Agents reviewing another attempt as a matter of course |
| **Subagents** | Claude Code subagents are recorded with their identity, but they belong to one run | Cross-run collaboration |
| **Evidence** | Captured automatically where the harness allows; claims marked as claims | Contracts that tie evidence to a decision |

## The problem

Teams already run coding agents. What they do not have is a reliable memory of what those agents
did, why they did it, and whether the result actually holds up.

- **Context is lost.** An agent makes a decision, the session ends, and the reasoning disappears
  with it. The next agent — or the next person — starts from zero.
- **Agents collide.** Several agents editing the same tree step on each other, and the resulting
  conflicts are resolved by whoever writes last, not by whoever is right.
- **Work is not reproducible.** A result exists, but the path that produced it — the attempts, the
  failed hypotheses, the evidence — cannot be replayed or audited.

KAP is built around the claim that the *reasoning trail* is the primary artifact, and the code is
one of its outputs.

## How KAP relates to Git

KAP does not replace Git. It layers a record of intent and evidence on top of the artifacts Git
already versions.

| | Git | KAP |
| --- | --- | --- |
| Records | Code snapshots and diffs | Actions, decisions, rationale, and supporting evidence |
| Answers | "What changed, and when?" | "Why does this look like this, and how do we know it works?" |
| Multi-agent model | Branches that merge (and conflict) | Isolated workspaces with explicit coordination |
| Reproducibility | A commit hash | A decision trail plus the state and evidence behind it |

## How it works

KAP is a versioning layer, not an agent runtime. It does not host a model, does not drive an agent's
loop, and does not require a particular harness. Claude Code, Codex, DeepSeek, and anything else
connect through adapters — and the recorded history stays useful with none of them running.

```text
Claude Code ──┐
Codex ────────┼── Adapters ── KAP Core ── .kap/
DeepSeek ─────┘                  │
                                 └── CLI
```

KAP is responsible for **isolation** (each agent gets its own branch and worktree), **history**
(commands, file changes, and their results), **decisions** (rationale and rejected alternatives),
**snapshots** (restorable states), **collaboration** (results exchanged without conflicts), and
**merge** (deliberate combination of results).

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) has the full design.

## Core concepts

These are provisional definitions — the vocabulary will settle as the design does.

- **Workspace** — an isolated unit of work, backed by a git worktree and a branch.
- **Agent** — an external actor plus the adapter that connects it.
- **Attempt** — one bounded run of an agent toward a goal. Attempts are the unit of comparison,
  snapshotting, and merging.
- **Action** — a single observable step: a command, an edit, a tool call, with its result.
- **Decision** — a choice an agent commits to, with the reasoning and the alternatives it rejected.
- **Evidence** — observed support for or against a decision: real test output, measured results,
  artifacts. An agent's claim is not evidence until something verifies it.
- **Snapshot** — a restorable state of an attempt at a point in time.

The platform's job is to keep these linked: every decision traceable to the actions that preceded
it and the evidence that backs it.

## Status

Pre-alpha. The loop is closed: an attempt gets its own branch and worktree, what happens inside is
recorded, the state can be snapshotted and restored, and the result is merged only through a review.

- [x] Repository, license, and contribution guide
- [x] Architecture and core concepts documented
- [x] Cargo workspace, core crate, and CLI skeleton
- [x] SQLite schema for attempts, runs, tool events, decisions, and evidence
- [x] Worktree-based isolation, `kap init`, `kap attempt start`, `kap attempt list`
- [x] Tool-event capture: `kap exec` records commands, exit codes, and both output streams
- [x] Decisions: `kap decision add` records rationale and rejected alternatives
- [x] Closing an attempt and comparing two attempts with `kap diff`
- [x] Evidence attached to decisions, marked observed or merely reported
- [x] Snapshots and restore: `kap snapshot create|list|restore`, plus `kap attempt clean`
- [x] Claude Code adapter through hooks: session briefing, tool events, and a stop-time snapshot
- [x] Review-gated merge: `kap review` records a verdict, `kap merge` obeys it
- [ ] Codex adapter and a documented adapter interface for other agents
- [x] `kap push` / `kap pull` over per-machine git refs, with idempotent import

## Getting started

Prebuilt binaries for macOS (Apple silicon and Intel) and Linux x86_64 are attached to every
[release](https://github.com/Manifestro/kap/releases). Or build from source — you need
[Rust](https://rustup.rs/) and `git`:

```sh
git clone https://github.com/Manifestro/kap.git
cd kap
cargo install --path crates/kap-cli   # installs `kap` into ~/.cargo/bin
```

The crates are published as [`kap-cli`](https://crates.io/crates/kap-cli) and
[`kap-core`](https://crates.io/crates/kap-core):

```sh
cargo install kap-cli
```

> The `kap` package on crates.io is an unrelated project. Installing this one means `kap-cli`; it
> installs the `kap` binary.

Then, inside a git repository that has at least one commit:

```sh
kap init                                  # creates .kap/ and ignores it in .gitignore
kap attempt start fix-parser \
  --task "make the parser accept trailing commas"
```

`kap attempt start` creates a branch `kap/attempt-…` and a worktree under `.kap/worktrees/`, then
prints where to work. From inside that worktree, the attempt is detected automatically:

```sh
cd .kap/worktrees/attempt-…
kap exec -- cargo test                    # recorded: command, exit code, stdout, stderr
kap decision add \
  --summary "Normalize trailing commas in the lexer" \
  --rationale "the parser then needs no special case" \
  --alternatives "handle them in the parser"
kap evidence attach \
  --source test-result --file results.txt \
  --command "cargo test" --exit-code 0 \
  --decision decision-…                   # an artifact is hashed and stored
kap log                                   # the timeline so far
```

Command output is stored by BLAKE3 hash in `.kap/objects/`, so identical output is kept once and a
hash proves two records saw the same bytes. A failed command is recorded too — it is evidence.

Evidence is never taken on trust. With an artifact, KAP stores and hashes the bytes and marks the
record **observed**. Without one — an agent saying "it works" — the record is kept as a claim and
marked **not observed**.

## Install once, then forget it

```sh
kap install
```

`kap install` adds KAP's hooks to `~/.claude/settings.json`, so every Claude Code session on the
machine reports to KAP without per-project setup. It only adds its own entries — existing settings
and other hooks are left untouched — and `kap uninstall` removes exactly what it added.

Recording still happens only inside an attempt worktree (`kap attempt start`); outside one the hooks
do nothing.

Any other CLI that fires the same hook events can be attached the same way, by pointing at its
settings file:

```sh
kap install --settings ~/.other-cli/settings.json
```

Turn it off when you do not want a session recorded:

| Scope | How |
| --- | --- |
| One session | `KAP_DISABLED=1 claude` |
| One repository | `kap disable` — and `kap enable` brings it back |
| Whole machine | `kap uninstall` |

## What the hooks record

KAP does not wrap the agent and does not own its loop. It listens through
[Claude Code hooks](https://code.claude.com/docs/en/hooks), which fire inside the session the agent
is already running.

| Hook | What KAP does with it |
| --- | --- |
| `SessionStart` | Injects a briefing — earlier attempts, recorded decisions — into the agent's context. |
| `PostToolUse` | Records commands with their output, and file edits with the file's hash. |
| `PostToolUseFailure` | Records the failure and its message. No exit code is invented. |
| `Stop` | Closes the run and snapshots changed files as evidence, so edits made through a shell are covered too. |

The hooks call the absolute path of the `kap` binary, so they work without `kap` on `PATH`. If you
prefer per-project setup, `kap hook setup` writes the same hooks into a checkout's
`.claude/settings.json`, which you can commit and share.

When the attempt is done, compare it and decide:

```sh
kap diff attempt-… attempt-…              # files changed between two attempts, not their bases
kap review attempt-…                      # changes vs its base, decisions, evidence, verdict
kap review attempt-… --approve --note "…" # the review gate: approve this exact commit
kap merge attempt-…                       # merges it and closes the attempt
```

Merging is gated on a review, and the review is pinned to a commit: if the attempt moves after it
was approved, the approval no longer applies and the merge is refused. `kap attempt close` freezes
an attempt you do not intend to merge.

Snapshots capture the whole worktree — tracked and untracked files — without moving the attempt's
branch, so taking one never invalidates a review. Restoring never overwrites the original: it opens
a new attempt from the snapshot, and both stay in the history.

```sh
kap snapshot create --note "before the refactor"
kap snapshot list
kap snapshot restore snapshot-…            # a new attempt, original untouched
kap attempt close && kap attempt clean     # drop the worktree, keep the branch
```

History travels through the git remote you already have:

```sh
kap push                                  # publishes under refs/kap/store/<machine-id>
kap pull                                  # imports every machine ref it finds
```

Each machine pushes to its own ref and imports rows by id, so there is no server to run, no lock to
take, and no force-push. `kap pull` is idempotent: pulling twice imports nothing the second time.
What travels is the history and the objects it references — never the SQLite file, which could not
be merged between two machines.

Most of the history is append-only, but closing an attempt is an update. Lifecycle moves forward
monotonically across machines: a closed attempt stays closed, and a stale export cannot reopen one.


## Limits

These are known and deliberate for now, not oversights:

- **A crash loses the command in flight.** An event is written after its command finishes, so
  killing `kap` mid-command loses that event — and the command itself keeps running as an orphan.
- **`.kap/` is the only copy of the history.** SQLite is crash-consistent, but there is no repair
  tool and no automatic backup: a corrupted `kap.db` loses decisions, evidence, and events. Branches
  and objects survive; `kap push` is the backup that exists.
- **Recorded output is stored verbatim.** If a command prints a secret, KAP stores it in `.kap/` and
  sends it to the remote on `kap push`. There is no redaction.
- **Hooks see tool calls, not reasoning.** Claude Code hooks capture tool calls and their output; they
  do not capture prompts, thinking, or model messages. A tool's arguments are recorded for shell
  commands and file edits only.
- **Review has no identity.** `kap review` records the git user name, so an agent can approve its own
  work with the same command a person uses. The gate exists; the separation between the worker and
  the reviewer does not.
- **Codex is not supported yet.**

## Repository layout

```text
crates/kap-core   # storage, worktrees, hooks, sync, and the attempt lifecycle
crates/kap-cli    # the `kap` binary
docs/             # architecture and design notes
```

## Contributing

KAP is written in Rust. Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the
development setup and the pull request process. By participating you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Security issues go through
[SECURITY.md](SECURITY.md), never a public issue.

## License

[MIT](LICENSE) © Manifestro
