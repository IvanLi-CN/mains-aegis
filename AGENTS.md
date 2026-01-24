# Repository Guidelines

## Project Purpose

`mains-aegis` is a docs-first hardware design repository. Most contributions are Markdown updates plus offline-renderable vendor documentation (datasheets/manuals/reference designs).

## Device Operation Discipline (Agent Guardrails)

To avoid operating the wrong device in multi-device / multi-port environments, the Agent must follow:

- No direct `espflash`: do not directly invoke `espflash` / `cargo espflash` / `cargo-espflash`. (Note: `mcu-agentd` may use an `espflash` backend internally; that is allowed when using `mcu-agentd`.)
- Single target port only: the only allowed target port must come from `mcu-agentd` selector state (user runs `mcu-agentd selector set <MCU_ID> <PORT>`; Agent may only read `mcu-agentd selector get <MCU_ID>`). The Agent must not enumerate candidate ports; if no unique target is set, deny device operations.
- No automatic port switching: never switch ports “to try”.
- State-changing allowlist (no confirmation): after verifying the selected target port via `mcu-agentd selector get <MCU_ID>`, the Agent may run only the allowlisted state-changing command `mcu-agentd monitor <MCU_ID> --reset` without additional confirmation prompts.
- Write allowlist (no confirmation): flashing is permitted only via `mcu-agentd flash <MCU_ID>`, and only after verifying the selected target port via `mcu-agentd selector get <MCU_ID>`. No additional confirmation prompts.
- Decision summary required: for every device-related operation (including denials), output a minimal, copy-pastable decision summary: `Operation type` (`read-only` / `state-changing` / `write`), `Target port`, `Command`, `Decision` (`allow|deny`), `Rationale` (which gate G0–G4), and `Next step`.

Gates (G0–G4) for the `Rationale` field:

- G0 (no direct espflash): deny any direct `espflash` / `cargo espflash` / `cargo-espflash`.
- G1 (selector-only port): target port must come from `mcu-agentd selector get <MCU_ID>` (agent may read only).
- G2 (unique port required): if selector does not yield a unique port, deny device ops and ask the user to run `mcu-agentd selector set <MCU_ID> <PORT>`.
- G3 (no enumeration/switching): deny any port enumeration and never switch ports “to try”.
- G4 (allowlist-only ops): allow only `mcu-agentd monitor <MCU_ID> --reset` and `mcu-agentd flash <MCU_ID>` after passing G1–G3.

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
