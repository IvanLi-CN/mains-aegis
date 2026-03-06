# Recovery Safety Policy

## Safety defaults

- Diagnose mode enforces `--recover never`.
- Recover mode must be explicit (`run.sh recover ...`).
- Canonical mode uses 7-bit `0x0B` by default.
- Supported no-pack bring-up uses `--force-min-charge true` to apply `VREG=16.8V / ICHG=200mA / IINDPM=500mA`.

## When recovery is allowed

- ROM signature observed (`0x9002`) and `--recover if-rom`.
- Repo-supported workflow stops when ROM signature is absent; do not upgrade to `--recover force`.

## When to stop

- Unexpected canonical access to `0x16` in logs.
- Repeated `transport_lost` without any valid samples.
- Power path is not stable (no minimal wake current path).
- Recover report shows `flash_attempted=true` but `flash_done=false`; treat this as a failed ROM exit, not a partial success.

## Device-operation guardrails

- Allowed: `mcu-agentd flash/monitor/reset` (G4)
- Allowed: `diagnose --mode dual-diag --probe-mode mac-only` as a diagnostic-only escalation before any ROM write decision
- Denied: direct `espflash` / `cargo espflash` (G0)
- Denied: selector enumeration/switching in automated flows (G1/G2/G3)
