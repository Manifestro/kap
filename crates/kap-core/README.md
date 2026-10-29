# kap-core

The engine behind [KAP](https://github.com/Manifestro/kap) — an open-source versioning layer for
work done by AI agents.

KAP records what an agent did, why it did it, and what backs the claim up: attempts in isolated git
worktrees, tool events, decisions, evidence, snapshots, reviews, and history shared through git.

This crate is the library. It does not talk to a model and does not drive an agent's loop — it
records and restores.

## Install

```sh
cargo add kap-core
```

The command-line tool lives in `kap-cli`:

```sh
cargo install kap-cli
```

## License

MIT © Manifestro
