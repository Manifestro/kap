# kap-cli

The `kap` command-line tool from [KAP](https://github.com/Manifestro/kap) — an open-source
versioning layer for work done by AI agents.

```sh
cargo install kap-cli
kap --help
```

> **Not the `kap` crate.** That package is an unrelated project. This one is `kap-cli`, and the
> binary it installs is `kap`.

## What it does

```sh
kap init                       # create .kap/ in a repository
kap attempt start my-task      # a branch and a worktree for one attempt
kap exec -- cargo test         # recorded: command, exit code, output
kap decision add --summary …   # what was decided, why, and what was rejected
kap review --approve           # the gate before anything is merged
kap merge                      # merge it, and close the attempt
```

## License

MIT © Manifestro
