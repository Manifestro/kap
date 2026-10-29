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

## Core concepts

These are provisional definitions — the vocabulary will settle as the design does.

- **Workspace** — an isolated environment where a single agent works. Changes made in one
  workspace do not leak into another.
- **Agent** — anything that can act inside a workspace: an external coding agent such as Claude
  Code or Codex, or a custom agent built on the platform's interfaces.
- **Action** — a single observable step an agent takes: a command, an edit, a test run, a request
  for review.
- **Decision** — a choice an agent commits to, together with the reasoning behind it.
- **Evidence** — what supports or undermines a decision: test output, experiment results,
  benchmarks, citations, reviewer objections.
- **Session** — the bounded span of an agent's work, with its own context and history.

The platform's job is to keep these linked: every decision traceable to the actions that preceded
it and the evidence that backs it.

## Status

Pre-alpha, design phase. What exists today:

- [x] Repository, license, and contribution guide
- [ ] Data model for actions, decisions, and evidence
- [ ] Workspace isolation and lifecycle
- [ ] Agent adapter interface (Claude Code, Codex, custom agents)
- [ ] Decision and evidence log
- [ ] Replay and reproducibility tooling
- [ ] Conflict detection between concurrent agents

## Getting started

Not available yet. Once the first components land, this section will cover installation and a
minimal end-to-end example.

## Contributing

KAP is written in Rust. Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the
development setup and the pull request process. By participating you agree to follow the project's
code of conduct (to be added).

## License

[MIT](LICENSE) © Manifestro
