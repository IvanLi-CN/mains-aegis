---
name: mains-aegis-user-operations
description: "Explicit end-user route for operating Mains Aegis devices through released mains-aegis host tools."
---

# Mains Aegis user operations

Use this skill only when the owner explicitly asks for end-user/released host-tools operation, installation validation, or this skill by name.

For Codex work inside the `mains-aegis` repository, default to `$mains-aegis-devd-flow` for development, validation, diagnostics, field investigation, and hardware read/session-read checks.

## Required boundaries

- Require released `mains-aegis` and `mains-aegis-devd` binaries to be installed and available on `PATH`.
- If the released tools are missing, stop and instruct installation from the latest `host-tools-v*` release archive. Do not fall back to `cargo run`, local source builds, or direct `espflash`.
- Use `mains-aegis` CLI for commands; normal CLI operations auto-start or reuse the singleton IPC daemon.
- Use `mains-aegis daemon http` when Web/API access is explicitly needed; CLI commands can point at its `--ipc` endpoint to share the same daemon state.
- Non-loopback `daemon http` requires `--allow-lan-bridge` and `--auth-token-file`; API clients must send `Authorization: Bearer <token>`, while browser EventSource requests use `service_token=<token>` or the legacy `bridge_token=<token>`.
- Real flash/reset/monitor requires a known bound device and owner authorization. Mock/dry-run validation is allowed.
- CLI flash and host power commands default to dry-run; real flash and real host power actions require explicit `--real` plus owner authorization.

## Standard user flow

1. Verify tools:
   - `mains-aegis --version`
   - `mains-aegis-devd --version`
2. Inspect devices:
   - `mains-aegis devices list`
   - `mains-aegis devices scan`
3. Owner-visible device lifecycle:
   - `mains-aegis device <id> bind --alias <name>`
   - `mains-aegis device <id> connect`
   - `mains-aegis device <id> identity`
4. Firmware operation:
   - Load/select artifact.
   - Run flash dry-run before any real flash.
   - Only run real flash with `--real` after owner authorization.

## Hard blocker

If `mains-aegis` or `mains-aegis-devd` is unavailable, report: released host tools are not installed; install the latest `host-tools-v*` release package for the platform and retry.
