# KAP

**An open-source multi-agent platform for research and development.**

KAP lets autonomous AI agents — Claude Code, Codex, and custom agents of your own — work on the
same problem together: propose hypotheses, run experiments, review each other's work, and defend
their conclusions inside isolated workspaces.

Git records *what* changed. KAP records *why*.

> **Status: pre-alpha.** KAP is at the design stage. There is nothing to install or run yet. The
> concepts, data model, and interfaces below are working drafts and will change.

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

Pre-alpha, design phase. What exists today:

- [x] Repository, license, and contribution guide
- [x] Architecture and core concepts documented
- [ ] Cargo workspace and core crate
- [ ] SQLite schema for events, decisions, and evidence
- [ ] Worktree-based isolation and attempt lifecycle
- [ ] `kap` CLI skeleton
- [ ] Adapter interface and a first adapter (Claude Code or Codex)
- [ ] Snapshots, diff between attempts, and restore
- [ ] Merge and conflict handling

## Getting started

Not available yet. Once the first components land, this section will cover installation and a
minimal end-to-end example.

## Contributing

KAP is written in Rust. Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the
development setup and the pull request process. By participating you agree to follow the project's
code of conduct (to be added).

## License

[MIT](LICENSE) © Manifestro
