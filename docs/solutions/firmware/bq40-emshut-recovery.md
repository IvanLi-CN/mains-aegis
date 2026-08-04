---
title: BQ40 EMSHUT recovery must be decoded before XDSG
module: firmware
problem_type: hardware-recovery
component: BQ40Z50
tags:
  - bq40z50
  - emshut
  - self-check
  - diag-snapshot
status: active
related_specs:
  - docs/specs/bq40-self-check-result-dialogs/SPEC.md
  - docs/specs/mains-aegis-devd/SPEC.md
---

# BQ40 EMSHUT recovery must be decoded before XDSG

## Context

Mains Aegis uses BQ40Z50 `OperationStatus()` to decide whether the battery can discharge and whether self-check may enter Dashboard. A pack that was shut down through `SHUTDN#` can report both `EMSHUT=1` and `XDSG=1`.

## Symptoms

- The front panel remains on self-check or goes to a black-but-backlit state after USB is attached.
- `diag-snapshot.bms.issue_detail` looks like a normal `xdsg_blocked` case.
- BQ40 pack voltage and SBS reads are valid, but `discharge_ready=false`, `DSG=false`, and removing input power immediately turns the UPS off.
- BQ25792 may report low `vbat_adc_mv` or `vbat_present=false` because the pack discharge FET is actually open, even though BQ40 pack voltage is healthy.

## Root Cause

`EMSHUT` is a more specific state than ordinary `XDSG`. If firmware checks `XDSG` first, it collapses emergency shutdown into a generic discharge block. That hides the real recovery path and makes the UI/host evidence ambiguous.

BQ40Z50 can exit EMSHUT through documented mechanisms, including a valid SMBus communication path when `Power Config[EMSHUT_EXIT_COMM]` is enabled. A recovery loop that only waits for ordinary discharge readiness can miss this exit trigger.

## Resolution

- Decode `OperationStatus()[EMSHUT]` before `XDSG`; use `issue_detail=emshut_active`.
- Expose the raw H4/block `OperationStatus()` payload in diagnostics:
  - `op_status_raw_len`
  - `op_status_raw_bytes`
  - decoded `emshut`, `pres`, `xdsg`, `dsg_fet`
- When BQ40 data flash is readable, expose EMSHUT-related configuration:
  - `da_configuration`
  - `power_config`
  - `emshut_en`
  - `emshut_pexit_dis`
  - `emshut_exit_comm`
  - `emshut_exit_vpack`
- During `DischargeAuthorization` recovery, run a short early communication-exit exercise by sending valid BQ40 SBS commands before falling back to the slower activation path.

## Guardrails / Reuse Notes

- Do not infer battery absence from BQ25792 `VBAT_PRESENT=false` while BQ40 can still report a healthy pack voltage; an open BQ40 discharge FET can make charger-side VBAT appear absent.
- Do not treat `OperationStatus()[PRES]` as a battery-present signal; it is the system-present / pin state signal.
- Keep the short communication-exit exercise scoped to discharge authorization recovery. Regular offline activation still needs the less intrusive no-charge observe window.
- Validate fixes with both host diagnostics and hardware:
  - before recovery, expect `emshut=true`, `xdsg=true`, `dsg_fet=false`
  - after recovery, expect `emshut=false`, `xdsg=false`, `dsg_fet=true`, `discharge_ready=true`

## References

- `docs/manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md`
- `docs/specs/bq40-self-check-result-dialogs/SPEC.md`
- `docs/specs/mains-aegis-devd/SPEC.md`
