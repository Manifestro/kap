# KAP Architecture

> **Status: draft.** This document records decisions made so far and the questions still open.
> Sections marked *(open)* are not settled; do not build on them without discussing first.

## 1. What KAP is

KAP is an **independent versioning layer for agent work**. It is not an execution environment for
agents and not a competing harness.

The analogy is Git: Git does not force developers to use a particular editor. In the same way, KAP
does not force agents to use a particular harness. Claude Code, Codex, DeepSeek, and any other agent
connect through adapters, and the recorded history remains useful without any of them running.

```text
Claude Code ──┐
Codex ────────┼── Adapters ── KAP Core ── .kap/
DeepSeek ─────┘                  │
                                 └── CLI
```

Two consequences follow, and both are requirements rather than accidents:

1. **KAP works without a running agent.** Browsing history, comparing attempts, restoring snapshots,
   and merging results must all work offline, without Claude, Codex, or DeepSeek installed.
2. **Execution belongs to the harness.** An adapter may start an agent inside an isolated
   workspace, but the agent's loop, model calls, and reasoning stay outside KAP.

KAP does not manage an agent's thinking. It manages its context, its events, and its history of
decisions.

## 2. Responsibilities

| Area | KAP's job |
| --- | --- |
| **Isolation** | Give every agent its own branch and space for changes. |
| **History** | Record actions, commands, file changes, and their results. |
| **Decisions** | Record explanations, considered alternatives, and justifications. |
| **Snapshots** | Restore the state of a given piece of agent work. |
| **Collaboration** | Exchange results between agents without conflicts. |
| **Merge** | Compare and deliberately combine results, behind a review gate. |
| **Sync** | Move attempts, decisions, and evidence between machines without loss. |

## 3. Non-goals

- **No LLM of its own.** KAP does not host, route, or call models.
- **No agent loop.** KAP does not drive an agent's reasoning or decide what it does next.
- **No hidden reasoning.** KAP records observable events and explanations agents explicitly
  provide. It cannot and does not capture private chain-of-thought.

KAP records what is observable and makes the difference between "observable" and "claimed" explicit
— see [Evidence](#5-evidence).

## 4. Storage

**Decision: SQLite, with content-addressed blobs on disk.**

A relational store was chosen over a purely file-based event log because the questions KAP must
answer are relational: which evidence belongs to which decision, which attempt produced which file
change, what changed between two attempts of the same task.

Layout:

```text
.kap/
├── kap.db              # SQLite: the record of events, decisions, evidence, metadata
├── objects/            # content-addressed payloads, named by hash (blake3)
│   └── ab/cd/abcdef…   # command output, diffs, artifacts
└── worktrees/          # git worktrees, one per active attempt
```

Principles:

- **The database is the log of record.** Every recorded event lands in SQLite in a single
  transaction with the references it needs.
- **Payloads are addressed by hash.** Large output (stdout, diffs, artifacts) is stored in
  `objects/` under `blake3:<hex>` and referenced from the database. Identical payloads are stored
  once; a hash is stable evidence that two records saw the same bytes.
- **Nothing is rewritten.** History rows are append-only. Corrections are new rows, not updates, so
  a later claim cannot quietly change what was observed earlier.
- **Read-side never needs adapters.** The database and objects are the whole story.

### 4.1 Sync and portability

**Decision: hybrid — local storage first, explicit synchronization on demand.**

- `.kap/` is the local database of history, decisions, evidence, and attempts.
- `kap push` and `kap pull` move KAP objects between machines.
- **Git is the first transport.** KAP objects travel as a separate ref namespace, apart from the
  source branches, so history syncs without polluting the code history.
- **KAP Remote** — a dedicated server — is a later option, not a requirement.
- **Content addressing (BLAKE3) does the deduplication.** The same object is stored once and
  verified on receipt; a corrupted or tampered object fails the hash check.
- **Never synced:** personal settings, secrets, and temporary workspaces.
- **Offline first.** Everything must work with no network at all; sync is never a precondition for
  recording or reading history.

## 5. Evidence

**Principle: evidence ≠ agent claims.**

If an agent says "the tests passed", that is a claim. Evidence exists only once KAP has the result
of an actual test run or a verifiable artifact.

Sources:

| Source | What it yields |
| --- | --- |
| **Tool events** | Commands executed, exit codes, stdout/stderr, tool-call results. |
| **File changes** | Diffs, snapshots, hashes of changed files. |
| **Test results** | Test, benchmark, and metric output from a real run. |
| **Agent reports** | Explanations, hypotheses, conclusions, and references to evidence. |
| **External artifacts** | Training logs, checkpoints, plots, datasets, reports. |

Each evidence record carries its provenance:

```json
{
  "id": "ev-001",
  "attempt_id": "attempt-014",
  "source": "tool_execution",
  "command": "pytest tests/",
  "exit_code": 0,
  "artifact_hash": "blake3:...",
  "timestamp": "2026-10-29T12:00:00Z"
}
```

An exit code of `0` confirms that a command finished successfully. It does not, by itself, prove
that the program is correct — and KAP does not pretend otherwise. Every record is marked with
whether it came from direct observation or from an agent's report.

When an external agent provides no telemetry, KAP marks the evidence as **incomplete**. It does not
invent the events it could not see.

The resulting division of labour: agents propose solutions, KAP records observable facts, and a
person or a reviewing process decides whether those facts support the decision.

## 6. Entities

The vocabulary is still settling. Current working definitions:

- **Workspace** — an isolated unit of work, backed by a git worktree and a branch.
- **Agent** — an external actor plus the adapter that connects it: Claude Code, Codex, DeepSeek, or
  anything else that can emit events.
- **Attempt** — one attempt to solve a task. An attempt may span several runs and sessions; it is
  the unit of comparison, snapshotting, and merging. Two agents attacking the same task produce two
  attempts. An attempt is closed explicitly — see [§8](#8-adapters).
- **Run** — one execution of an agent harness inside an attempt. A run can end and another can
  begin without closing the attempt.
- **Tool event** — one observable action inside a run: an executed command, a file edit, a tool
  call, along with its result.
- **File change** — a diff or content hash recorded against an attempt.
- **Decision** — a choice an agent commits to, with its reasoning and the alternatives it rejected.
- **Evidence** — observed support for or against a decision (see above).
- **Snapshot** — a restorable state of an attempt at a point in time.

## 7. Isolation

**Decision: git worktrees.**

Each attempt gets its own worktree under `.kap/worktrees/` on its own branch. Worktrees were chosen
over containers because they are native to the ecosystem KAP tracks, cheap to create, and require no
daemon. Isolation here means *changes do not leak between attempts*, not *untrusted code cannot run*.

*(open)* Whether untrusted agents should additionally be confined (containers, sandboxes) is a
separate question, and likely a later one.

## 8. Adapters

An adapter connects one harness to KAP. Its job is narrow: translate what the harness does into
events, and translate KAP's instructions into something the harness understands.

### Lifecycle

An adapter runs a fixed cycle around every attempt:

1. **Context in.** KAP hands the agent the task's history: previous attempts, decisions, and
   evidence. The agent starts informed instead of cold.
2. **Work.** The agent acts inside an isolated workspace — its own worktree and branch.
3. **Decisions.** The adapter records actions as they happen and asks the agent to explain the
   decisions it makes — the rationale and the alternatives it rejected.
4. **Evidence.** KAP captures commands, file changes, and check results automatically. What the
   adapter cannot observe is marked unobserved.
5. **Completion.** The agent returns a result. KAP closes the attempt and preserves its history for
   the agents that follow.

KAP never asks an agent to expose its reasoning process; it asks for the explanation of a decision,
which the agent can produce deliberately.

### Modes

- **Wrapped mode** — the adapter launches the agent inside a prepared worktree and observes tool
  calls and file changes as they happen. This gives the most complete telemetry.
- **Reported mode** — the agent runs on its own and reports what it did by calling the `kap` CLI
  (or an MCP bridge). This works with any harness, at the cost of completeness: KAP records what it
  is told, and marks the rest as unobserved.

An adapter must never be required for reading history.

## 9. CLI surface *(draft)*

```text
kap init                     # create .kap/ in a repository
kap attempt start            # open a new attempt (worktree + branch)
kap attempt close            # close an attempt and freeze its history
kap attempt list             # list attempts and their state
kap log                      # timeline of events, decisions, evidence
kap decision add             # record a decision with rationale and alternatives
kap evidence attach          # attach an artifact as evidence
kap snapshot                 # snapshot the current state of an attempt
kap diff <a> <b>             # compare two attempts
kap review <attempt>         # inspect diff + evidence before merging
kap merge <attempt>          # merge an attempt, only after review
kap restore <snapshot>       # restore a recorded state
kap push / kap pull          # sync KAP objects with a remote
```

Names, flags, and grouping will change. The list exists to keep scope honest.

## 10. Open questions

1. **How the adapter asks for a decision.** Prompt injection, a tool call, or a post-run
   interrogation — and how much KAP should insist on an explanation before closing an attempt.
2. **Merge conflicts.** The review gate is decided; what review does when two attempts touch the
   same file is not.
3. **Multi-user use.** Whether several people share one `.kap/`, and how identity and authorship
   are attributed across them.
4. **Object retention.** Snapshots and artifacts grow without bound; when they may be pruned
   without breaking reproducibility.
