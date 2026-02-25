# Recovery Safety Policy

## Safety defaults

- Diagnose mode enforces `--recover never`.
- Recover mode must be explicit (`run.sh recover ...`).
- Canonical mode uses 7-bit `0x0B` by default.

## When recovery is allowed

- ROM signature observed (`0x9002`) and `--recover if-rom`.
- Explicit force path is requested (`--recover force`) for controlled diagnosis.

## When to stop

- Unexpected canonical access to `0x16` in logs.
- Repeated `transport_lost` without any valid samples.
- Power path is not stable (no minimal wake current path).

## Device-operation guardrails

- Allowed: `mcu-agentd flash/monitor/reset` (G4)
- Denied: direct `espflash` / `cargo espflash` (G0)
- Denied: selector enumeration/switching in automated flows (G1/G2/G3)
