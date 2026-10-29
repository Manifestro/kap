# Contributing to KAP

Thanks for considering a contribution. KAP is an early-stage project, so the most valuable
contributions right now are design discussions and clearly scoped patches — not large rewrites.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Please report
security issues through [SECURITY.md](SECURITY.md), never in a public issue.

## Before you start

- **Open an issue first** for anything larger than a bug fix or a small improvement: new features,
  changes to the data model, or anything that touches the public interfaces described in the
  [README](README.md). The design is still moving, and a short discussion can save a long review.
- **Keep pull requests focused.** One logical change per PR is easier to review and to revert.

## Development setup

KAP is written in Rust and targets the current stable toolchain.

1. Install [rustup](https://rustup.rs/).
2. Install the toolchain components used by CI:

   ```sh
   rustup component add rustfmt clippy
   ```

3. Clone the repository and verify the workspace builds:

   ```sh
   git clone https://github.com/Manifestro/kap.git
   cd kap
   cargo build
   cargo test
   ```

## Code style

- Run `cargo fmt` before committing. CI rejects unformatted code.
- Run `cargo clippy --all-targets --all-features` and treat warnings as errors.
- Public items get documentation comments. Explain *why* in comments; the code already says *what*.
- Prefer clear, boring code over clever code. This project is meant to be read.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<scope>): <short summary>

<body: what changed and why, wrapped at 72 columns>
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `build`. Keep the summary in the
imperative mood, lowercase, and without a trailing period.

## Pull requests

1. Fork the repository and create a branch from `main`:
   `git switch -c feat/short-description`.
2. Make your change, with tests where the behaviour is testable.
3. Update documentation and the [README](README.md) if the change affects how people use KAP.
4. Make sure `cargo fmt --check`, `cargo clippy`, and `cargo test` pass.
5. Open the PR with a short description: the problem, the approach, and anything a reviewer should
   look at closely. Link the issue it resolves.

A maintainer will review as soon as they can. Reviews may ask for changes to keep the design
coherent — that is about the project, not about you.

## License of contributions

By submitting a pull request you agree that your contribution is licensed under the
[MIT License](LICENSE), the same terms as the rest of the project.

## Security

Do not open a public issue for a vulnerability. [SECURITY.md](SECURITY.md) explains the private
reporting channel and what is in scope.

## Questions

Open a discussion or an issue. There is no separate mailing list or chat yet.
