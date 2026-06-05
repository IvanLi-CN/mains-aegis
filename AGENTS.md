# Repository Guidelines

## Project Purpose

`mains-aegis` is a docs-first hardware design repository. Most contributions are Markdown updates plus offline-renderable vendor documentation (datasheets/manuals/reference designs).

## Device Operation Discipline (Agent Guardrails)

To avoid operating the wrong device in multi-device / multi-port environments, the Agent must follow:

- No direct `espflash`: do not directly invoke `espflash` / `cargo espflash` / `cargo-espflash`.
- No legacy `mcu-agentd` device path: do not invoke `mcu-agentd` for Mains Aegis hardware operations, including `selector`, `flash`, `monitor`, `erase`, `reset`, or `logs`.
- No port enumeration: never enumerate `/dev/*` or other serial-device paths to discover candidate ports.
- No port switching: never switch ports “to try”.
- Required workflow: use released `mains-aegis` / `mains-aegis-devd` host tools for Mains Aegis device work.
- `mains-aegis-devd` may scan/list serial candidates for owner-visible binding, but must not auto-connect, auto-switch, or try alternate ports. Real flash/reset/monitor still requires a known bound device and owner authorization; mock/dry-run validation is allowed. `mains-aegis-devd serve` is IPC-only; HTTP/Web access requires explicit `mains-aegis-devd bridge-http`.
- `mains-aegis-devd` flash may invoke its internal `espflash` backend; Agents must not invoke `espflash` directly from the shell.
- Decision summary required: for every device-related operation (including denials), output a minimal, copy-pastable decision summary: `Operation type` (`read-only` / `state-changing` / `write`), `Command`, `Decision` (`allow|deny`), `Rationale` (which gate G0–G5), and `Next step`.

Gates (G0–G5) for the `Rationale` field:

- G0 (no direct espflash): deny any direct `espflash` / `cargo espflash` / `cargo-espflash`.
- G1 (no legacy mcu-agentd path): deny `mcu-agentd` hardware operations for Mains Aegis.
- G2 (no port enumeration): deny any port enumeration.
- G3 (no port switching): deny any port switching.
- G4 (no automatic port switching): deny any attempt to “try another port”.
- G5 (devd required): allow released `mains-aegis` / `mains-aegis-devd` mock/dry-run operations and owner-visible scan/list/bind flows; real flash/reset/monitor still requires explicit bound-device context and owner authorization.

## Project Structure & Module Organization

- `docs/`: project docs and indexes (start at `docs/README.md`).
- `docs/datasheets/<PART>/`: Markdown conversions of datasheets with local `images/` for offline viewing.
- `docs/manuals/<DOC>/`, `docs/reference-designs/<DOC>/`: same pattern for manuals and reference designs.
- `downloads/`: scratch space for raw PDFs/ZIPs (ignored; do not commit).

## Build, Test, and Development Commands

There is no build system or test runner yet. Useful local commands:

- Search content: `rg "BQ40Z50" docs`
- Preview docs via a local server: `python -m http.server -d docs 8000`
- Start IPC daemon for development: `cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- serve`
- Start local HTTP bridge for development: `cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- bridge-http --allow-dev-cors`
- Generate firmware catalog entry: `python3 tools/firmware-artifact/build-catalog-entry.py --elf <firmware-elf> --out firmware/target/mains-aegis-artifacts`
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
