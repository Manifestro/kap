# Releasing KAP

KAP is pre-1.0: minor versions may break the CLI and the on-disk format, and the changelog says so
when they do.

## Cutting a release

1. **Pick the version.** Update `version` in the `[workspace.package]` section of `Cargo.toml`.
   `cargo build` writes the same value into `kap --version`.
2. **Update `CHANGELOG.md`.** Move entries from `[Unreleased]` into a new `[X.Y.Z] - YYYY-MM-DD`
   section, and update the compare links at the bottom.
3. **Check the tree.** `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
   warnings`, `cargo test --all` — the same three checks CI runs.
4. **Commit and tag.**

   ```sh
   git commit -am "chore: release v0.1.0"
   git tag -a v0.1.0 -m "KAP v0.1.0"
   git push origin main
   git push origin v0.1.0
   ```

5. **Watch the workflow.** Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds
   `kap` for Linux x86_64, macOS arm64, and macOS x86_64, packages each with `README.md`,
   `LICENSE`, and `CHANGELOG.md`, and attaches the archives plus SHA-256 sums to a GitHub release.
6. **Verify the release page.** Every archive should be present with its `.sha256`, and the release
   notes should be generated.

## If the workflow fails

The build job is per target, so one platform can fail without blocking the others; the release job
still waits for all three. Fix the cause, delete the tag, and re-tag:

```sh
git tag -d v0.1.0
git push origin :refs/tags/v0.1.0
```

A release is never edited by hand: the workflow owns the assets, so the next tag is the next
release.

## Installing a release

```sh
curl -L https://github.com/Manifestro/kap/releases/latest/download/kap-aarch64-apple-darwin.tar.gz \
  | tar -xz -C ~/.local/bin
```

Or from source: `cargo install --git https://github.com/Manifestro/kap --locked`.

## Publishing to crates.io

The package names are `kap-core` and `kap-cli`; both are free. The `kap` package name belongs to an
unrelated project, so it is never used — the binary is still called `kap`.

Publishing is manual and needs a token (`cargo login`), because the upload is an identity-bound
action:

```sh
cargo publish -p kap-core     # first: kap-cli depends on it by version
cargo publish -p kap-cli
```

`cargo publish` strips path dependencies, so `kap-core` must exist on crates.io before `kap-cli`
can be published. The working tree must be clean.

If something is wrong with a published version, yank it rather than deleting anything:

```sh
cargo yank --version 0.1.0 kap-cli
```

## What is not automated

- crates.io publishing is manual (token, order, and no way to undo an upload).
- There is no Homebrew formula and no other package registry.
