---
name: mains-aegis-devd-flow
description: "Default in-repo flow for Mains Aegis host-tools/devd development, validation, diagnostics, and hardware read/session-read checks."
---

# mains-aegis-devd flow

Use this developer skill by default when Codex is working inside this repository on host-tools source, firmware artifacts, USB CDC control, devd APIs, IPC, HTTP bridge, Web/devd integration, diagnostics, field investigation, or hardware read/session-read checks.

Use `$mains-aegis-user-operations` instead only when the owner explicitly asks for end-user/released host-tools operation, installation validation, or that skill by name.

## Required boundaries

- This is the default in-repo Codex route. Source-built `tools/mains-aegis-host` CLI/devd may be used for development, validation, diagnostics, field investigation, and hardware read/session-read checks. Prefer the `mains-aegis` CLI lifecycle surface over directly invoking `mains-aegis-devd`; the daemon binary is a packaged sibling/internal process and a low-level debug fallback.
- Prefer released `mains-aegis` / `mains-aegis-devd` only for explicit end-user/released host-tools workflows.
- Use repository `Justfile` recipes for source-built development, validation, firmware artifact generation, and devd-backed flash flows. Run `just --list` first when unsure; do not bypass `just firmware-*` with raw Cargo/esptool commands unless updating the recipes themselves.
- Do not directly run `espflash`, `cargo espflash`, or `cargo-espflash` from the agent shell. The devd flash backend may invoke `espflash` internally after an explicit HTTP/API request or test dry-run.
- Do not auto-connect because of scan results, page probes, history, or the first/last seen device. Do not auto-switch or try alternate serial ports. Scanning may list candidates; binding and connecting remain user-visible operations.
- Read/session-read operations are allowed by default under this skill: scan/list, connect/disconnect, identity/status/diag-snapshot, and monitor start/stop/log reading.
- Persistent binding changes, settings writes, reset, flash, and real host power actions require explicit owner authorization. Use mock/dry-run validation otherwise.
- defmt logs are `verified` only when device firmware identity matches the selected artifact manifest by exact `build_id`, build profile, and feature set.

## Standard development flow

1. Generate or select a Firmware Catalog manifest.
   - Local builds use `just firmware-release`.
   - Release ELF-only validation uses `just firmware-build`.
   - Flash dry-runs use `just flash-current-dry-run <device>`.
   - Real flash uses `just flash-current-real <device> flash` only after explicit owner authorization.
   - GitHub Releases and Web bundled fallback must use the same schema.
2. Start devd IPC for development/debugging:
   - `just devd-serve`
   - Normal CLI read/session-read commands auto-start the repo-local IPC daemon when needed; `just devd-serve` is only for foreground logs or persistent debug sessions.
3. Start the explicit local HTTP bridge only when Web/API validation needs HTTP:
   - `just devd-http`
   - Point CLI commands at the same `--ipc` endpoint when Web and CLI must observe the same bridge state.
4. Develop Web UI through Vite.
   - The Web dev server proxies `/api` to the explicit `mains-aegis daemon http` endpoint, default `http://127.0.0.1:30080`.
5. Read/session-read device lifecycle:
   - `scan/list -> connect selected or bound device -> identity/status/diag-snapshot -> monitor logs -> disconnect`.
6. State-changing device lifecycle:
   - `bind/unbind`, `settings`, `artifact select`, `reset`, `flash`, and real host power actions require explicit owner authorization.
7. Validation without hardware:
   - Use `flash dry_run=true` plus unit tests with synthetic in-memory device state.
   - Verify `log_decode.status` is `verified` for matching manifests and `unverified` for mismatches.

## Documentation requirements

- Update `docs/specs/p8k3d-mains-aegis-devd/SPEC.md` when changing devd contracts.
- Update `docs/specs/7jqrq-mains-aegis-cli-devd-alignment/SPEC.md` when changing CLI/devd/IPC/bridge/release alignment.
- Update `docs/firmware-catalog.md` when changing manifest semantics.
- Update `AGENTS.md` if device operation permissions or denials change.
