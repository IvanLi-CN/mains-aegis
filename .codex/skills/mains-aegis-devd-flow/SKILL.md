---
name: mains-aegis-devd-flow
description: "Operate Mains Aegis devices through mains-aegis-devd."
---

# mains-aegis-devd flow

Use this skill when working on local device operations, firmware artifacts, USB CDC control, devd APIs, or Web/devd integration for this repository.

## Required boundaries

- Prefer `tools/mains-aegis-devd` over `mcu-agentd` for Mains Aegis device workflows.
- Do not directly run `espflash`, `cargo espflash`, or `cargo-espflash` from the agent shell. The devd flash backend may invoke `espflash` internally after an explicit HTTP/API request or test dry-run.
- Do not auto-connect, auto-switch, or try alternate serial ports. Scanning may list candidates; binding and connecting are separate user-visible operations.
- No real hardware flash/reset/monitor is allowed without a known bound device and owner authorization. Use mock/dry-run validation otherwise.
- defmt logs are `verified` only when device firmware identity matches the selected artifact manifest by exact `build_id`, build profile, and feature set.

## Standard development flow

1. Generate or select a Firmware Catalog manifest.
   - Local builds use `tools/firmware-artifact/build-catalog-entry.py`.
   - GitHub Releases and Web bundled fallback must use the same schema.
2. Start devd:
   - `cargo run --manifest-path tools/mains-aegis-devd/Cargo.toml -- serve`
   - Add `--web-root web/dist` only for production/static handoff.
3. Develop Web UI through Vite.
   - The Web dev server proxies `/api` to `http://127.0.0.1:30080`.
4. Device lifecycle:
   - `scan -> bind -> connect -> identity -> artifact/select -> monitor/reset/flash`.
5. Validation without hardware:
   - Use mock device and `flash dry_run=true`.
   - Verify `log_decode.status` is `verified` for matching manifests and `unverified` for mismatches.

## Documentation requirements

- Update `docs/specs/p8k3d-mains-aegis-devd/SPEC.md` when changing devd contracts.
- Update `docs/firmware-catalog.md` when changing manifest semantics.
- Update `AGENTS.md` if device operation permissions or denials change.
