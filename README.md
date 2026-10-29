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

Pre-alpha. The recording half works: an attempt gets its own branch and worktree, commands and
decisions are recorded as they happen, and two attempts can be compared before anything is merged.

- [x] Repository, license, and contribution guide
- [x] Architecture and core concepts documented
- [x] Cargo workspace, core crate, and CLI skeleton
- [x] SQLite schema for attempts, runs, tool events, decisions, and evidence
- [x] Worktree-based isolation, `kap init`, `kap attempt start`, `kap attempt list`
- [x] Tool-event capture: `kap exec` records commands, exit codes, and both output streams
- [x] Decisions: `kap decision add` records rationale and rejected alternatives
- [x] Closing an attempt and comparing two attempts with `kap diff`
- [ ] Evidence attached to decisions, with its completeness marked
- [ ] Snapshots and restore
- [ ] Adapter interface and a first adapter (Claude Code or Codex)
- [ ] Review-gated merge
- [ ] `kap push` / `kap pull` over a separate git ref namespace

## Getting started

KAP is built from source. You need [Rust](https://rustup.rs/) and `git`.

```sh
git clone https://github.com/Manifestro/kap.git
cd kap
cargo build --release
```

Then, inside a git repository that has at least one commit:

```sh
cp target/release/kap /usr/local/bin/     # or put it anywhere on PATH
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
kap log                                   # the timeline of both
```

Command output is stored by BLAKE3 hash in `.kap/objects/`, so identical output is kept once and a
hash proves two records saw the same bytes. A failed command is recorded too — it is evidence.

When the attempt is done, close it and compare it with another attempt:

```sh
kap attempt close                         # freezes the attempt; a closed attempt records nothing more
kap diff attempt-… attempt-…              # files changed between two attempts, not their bases
```


## Repository layout

```text
crates/kap-core   # storage, worktrees, and the attempt lifecycle
crates/kap-cli    # the `kap` binary
docs/             # architecture and design notes
```

## Contributing

KAP is written in Rust. Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the
development setup and the pull request process. By participating you agree to follow the project's
code of conduct (to be added).

## License

[MIT](LICENSE) © Manifestro
