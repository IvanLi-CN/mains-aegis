# Code quality & CI

This repo uses a small set of quality gates to keep changes consistent and reviewable:

- Local Git hooks (`lefthook`) to catch issues before pushing
- GitHub Actions checks for formatting, firmware build, PR title lint, and dependency review

## Prerequisites

- `bun` (JS runtime + package manager)
- `lefthook` (git hooks manager)
- Rust toolchain(s):
  - `stable` toolchain (for `rustfmt`)
  - `esp` toolchain (for firmware builds; installed via `espup`)

## Install JS dev tools (bun)

From repo root:

```bash
bun install --frozen-lockfile
```

## Install Git hooks (lefthook)

From repo root:

```bash
lefthook install
```

Manual run for debugging:

```bash
lefthook run pre-commit
bun run commitlint -- --edit .git/COMMIT_EDITMSG
lefthook run pre-push
```

## Commit / PR title rules

- Conventional Commits
- No CJK characters in subject/body
- Subject must not start with an uppercase letter

## Troubleshooting

- Hook fails with `rustfmt is required`: install `rustfmt` (e.g. `rustup component add rustfmt` on stable).
- `bun install --frozen-lockfile` fails: update `bun` to match `package.json` `engines.bun` constraint.
- Dependency review check fails with "not supported": enable Dependency graph (and GitHub Advanced Security for private repos) in repository settings.
