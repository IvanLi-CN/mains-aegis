# Repository Guidelines

## Project Purpose

`mains-aegis` is a docs-first hardware design repository. Most contributions are Markdown updates plus offline-renderable vendor documentation (datasheets/manuals/reference designs).

## Project Structure & Module Organization

- `docs/`: project docs and indexes (start at `docs/README.md`).
- `docs/datasheets/<PART>/`: Markdown conversions of datasheets with local `images/` for offline viewing.
- `docs/manuals/<DOC>/`, `docs/reference-designs/<DOC>/`: same pattern for manuals and reference designs.
- `downloads/`: scratch space for raw PDFs/ZIPs (ignored; do not commit).

## Build, Test, and Development Commands

There is no build system or test runner yet. Useful local commands:

- Search content: `rg "BQ40Z50" docs`
- Preview docs via a local server: `python -m http.server -d docs 8000`
- Review changes before PR: `git status` / `git diff`

## Coding Style & Naming Conventions

- Keep docs “offline-first”: prefer relative links (e.g., `docs/datasheets/BQ25792/`) and local images under `images/` (no hotlinked diagrams).
- Match existing language in the area you edit (design docs are mostly Chinese; vendor-extraction READMEs are typically English).
- For new vendor drops, follow the existing layout: `docs/{datasheets,manuals,reference-designs}/<NAME>/` with `README.md`, `<NAME>.md`, and `images/`.

## Testing Guidelines

No automated tests. Before opening a PR, manually verify that:

- New content is linked from the relevant index (`docs/README.md`, `docs/datasheets/README.md`, etc.).
- Markdown renders without external image dependencies (quick check: `rg -n '!\[.*\]\(https?://' docs`).

## Commit & Pull Request Guidelines

- Follow the repo’s Conventional Commit style from history: `docs(scope): short summary` (example: `docs(bms): record precharge resistor`).
- PRs should be small and descriptive: include what changed, why it changed, and source URLs for any added vendor documents.
