# Development

## Hooks

This repo uses `lefthook` for local quality gates:

1. Install hooks:

   - `lefthook install`

2. Install Bun dev tools (commitlint + dprint):

   - `bun install`

## Lint

- Rust:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test --all`

- Markdown (staged files):
  - `bunx --no-install dprint fmt <files...>`

## Commit messages

Commit messages must follow Conventional Commits and be English-only.

Examples:

- `feat(driver): add vout slew rate setter`
- `fix(i2c): handle nack on read`
- `docs: update register map notes`
