---
name: mains-aegis-devd-flow
description: "Develop and validate Mains Aegis host-tools, CLI, devd, IPC, HTTP bridge, and Web/devd integration."
---

# mains-aegis-devd flow

Use this developer skill when changing host-tools source, firmware artifacts, USB CDC control, devd APIs, IPC, HTTP bridge, or Web/devd integration for this repository.

## Required boundaries

- Prefer released `mains-aegis` / `mains-aegis-devd` for owner-facing device workflows; use `tools/mains-aegis-host` only for development and validation.
- Do not directly run `espflash`, `cargo espflash`, or `cargo-espflash` from the agent shell. The devd flash backend may invoke `espflash` internally after an explicit HTTP/API request or test dry-run.
- Do not auto-connect, auto-switch, or try alternate serial ports. Scanning may list candidates; binding and connecting are separate user-visible operations.
- No real hardware flash/reset/monitor is allowed without a known bound device and owner authorization. Use mock/dry-run validation otherwise.
- defmt logs are `verified` only when device firmware identity matches the selected artifact manifest by exact `build_id`, build profile, and feature set.

## Standard development flow

1. Generate or select a Firmware Catalog manifest.
   - Local builds use `tools/firmware-artifact/build-catalog-entry.py`.
   - GitHub Releases and Web bundled fallback must use the same schema.
2. Start devd IPC for development:
   - `cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- serve`
3. Start the explicit local HTTP bridge only when Web/API validation needs HTTP:
   - `cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- bridge-http --allow-dev-cors`
   - Add `--web-root web/dist` only for production/static handoff.
   - Point CLI commands at the same `--ipc` endpoint when Web and CLI must observe the same bridge state.
4. Develop Web UI through Vite.
   - The Web dev server proxies `/api` to the explicit `bridge-http` endpoint, default `http://127.0.0.1:30080`.
5. Device lifecycle:
   - `scan -> bind -> connect -> identity -> artifact/select -> monitor/reset/flash`.
6. Validation without hardware:
   - Use mock device and `flash dry_run=true`.
   - Verify `log_decode.status` is `verified` for matching manifests and `unverified` for mismatches.

## Documentation requirements

- Update `docs/specs/p8k3d-mains-aegis-devd/SPEC.md` when changing devd contracts.
- Update `docs/specs/7jqrq-mains-aegis-cli-devd-alignment/SPEC.md` when changing CLI/devd/IPC/bridge/release alignment.
- Update `docs/firmware-catalog.md` when changing manifest semantics.
- Update `AGENTS.md` if device operation permissions or denials change.
