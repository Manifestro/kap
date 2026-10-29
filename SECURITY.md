# Security Policy

KAP runs `git`, executes commands inside attempts, and records their output. It is not a sandbox and
does not claim to isolate untrusted code — see "Scope" below.

## Supported versions

KAP is pre-alpha. Only the latest commit on `main` is supported, and fixes are not backported to
older commits or tags.

## Reporting a vulnerability

Please do not report security issues as public GitHub issues.

Use GitHub's private vulnerability reporting for this repository:

<https://github.com/Manifestro/kap/security/advisories/new>

If that is unavailable to you, contact the maintainers through the Manifestro organization on
GitHub (<https://github.com/Manifestro>) and ask for a private channel.

Please include:

- what an attacker can do, and under what conditions;
- the smallest reproduction you can manage;
- the KAP version (`kap --version`), OS, and git version.

## What to expect

- An acknowledgement within 7 days.
- An assessment of severity and a suggested fix timeline.
- Credit in the advisory once a fix is released, unless you ask to stay anonymous.

## Scope

KAP stores a lot of text on disk: commands, their output, diffs, and artifacts. Treat `.kap/` as
sensitive as the work it records. Things worth reporting:

- a way to make KAP execute an unintended command, for example through hook input or a crafted
  `.claude/settings.json`;
- a path-traversal or symlink issue that lets a crafted repository write outside `.kap/` or outside
  the attempt worktree;
- an object or snapshot whose content does not match its hash;
- a way to make `kap pull` import rows that were not authored by the machine they claim.

Out of scope:

- **Agents doing what agents do.** KAP records what an agent runs; it does not sandbox it. Running an
  agent with broad permissions is a decision made outside KAP.
- **Recording is not encryption.** `.kap/` is plain SQLite and plain files. Do not record secrets in
  it, and do not publish it.
- **Leaked tokens in recorded output.** If a command prints a secret, KAP stores it. Rotate the
  secret; there is nothing to fix in KAP.
