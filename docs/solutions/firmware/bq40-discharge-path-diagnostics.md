---
title: BQ40 discharge path diagnostics must separate OperationStatus, AFE state, and charger BAT
module: firmware
problem_type: hardware-diagnostics
component: BQ40Z50
tags:
  - bq40z50
  - afe-register
  - discharge-path
  - self-check
  - diag-snapshot
status: active
related_specs:
  - docs/specs/5cvrj-bq40-self-check-result-dialogs/SPEC.md
  - docs/specs/cqd8u-regulated-output-module/SPEC.md
  - docs/specs/p8k3d-mains-aegis-devd/SPEC.md
---

# BQ40 discharge path diagnostics must separate OperationStatus, AFE state, and charger BAT

## Context

Mains Aegis gates every output mode that needs `TPS55288` output on the battery discharge path. A valid `BQ40Z50` SBS response or a healthy pack voltage is not enough to prove that the pack can power the system. The discharge path is only proven when the BQ40 logical state, the AFE FET state, and the downstream BAT node are consistent.

The recurring trap is to read `OperationStatus()[DSG]` as "the discharge FET is physically on". It is useful, but it is not the most direct hardware-state observation. `ManufacturerAccess() 0x0058 AFE Register` returns BQ40 AFE hardware registers; in that response, `BB` is `AFE FET Status` and `FF` is `AFE Control`.

## Symptoms

- BQ40 communication is healthy and cells/pack voltage are plausible.
- `OperationStatus()[DSG]` may be set while the system still cannot run from the pack.
- BQ25792 reports `VBAT_PRESENT=false` or a low `VBAT_ADC` value even though BQ40 reports a normal pack voltage.
- Self-check stays blocked or recovery reports failure, but the raw evidence looks contradictory if `OperationStatus()[DSG]` is treated as the whole truth.

## Root Cause Pattern

The discharge path has multiple observation planes:

| Plane | What it proves | What it cannot prove alone |
| --- | --- | --- |
| BQ40 SBS pack/cell telemetry | BQ40 is alive and can measure the cell stack | The CHG/DSG power path is closed |
| `OperationStatus()` | BQ40 firmware state and logical FET state flags | The AFE gate driver output is actually asserted |
| `AFE Register[BB]` / `[FF]` | AFE-side FET status/control bits | Downstream charger BAT node continuity |
| BQ25792 `VBAT_PRESENT` / `VBAT_ADC` | The charger BAT pin sees pack-side voltage | Whether BQ40 decided to enable a FET |
| TPS requested/active/gate state | Whether regulated outputs were requested and actually active | Why the upstream battery path is unavailable |

For the observed failure pattern, the decisive contradiction is:

- BQ40 pack/cells are valid.
- BQ25792 BAT is absent or near ground.
- `OperationStatus()[DSG]` is not enough to close the case.
- `AFE FET Status` and `AFE Control` must be decoded before claiming that DSG is physically on.

When `AFE FET Status=0x21` and `AFE Control=0xC1`, the FET bit positions from the AFE register table and bit descriptions show `b1=DSG` and `b2=CHG`; both bits are clear in both bytes. The set bits are not proof of the discharge path being enabled. That points to BQ40/AFE not actually driving the discharge path, not to BQ25792 inventing a missing battery.

## Minimum Valuable Data

Collect these fields together. Missing one group makes the conclusion weaker.

### BQ40 identity and pack validity

- BQ40 address and communication state.
- Pack voltage, current, RSOC, remaining/full charge capacity.
- All cell voltages and min/max/delta.
- `BatteryStatus()` including `RCA`, `OCA`, `TCA`, and `FC/FD` if available.

Purpose: prove the gauge is alive and the cell stack itself is not absent.

### BQ40 logical state

- Raw `OperationStatus()` H4/block bytes and decoded `op_status`.
- `EMSHUT`, `PRES`, `XCHG`, `XDSG`.
- `op_chg_fet`, `op_dsg_fet`, `op_pchg_fet`.

Purpose: distinguish EMSHUT, logical charge/discharge inhibit, and ordinary FET-state flags. Keep `op_*` names to avoid implying physical gate state.

### BQ40 AFE hardware state

- Full raw `AFE Register` response if possible.
- At minimum:
  - `AFE Register[BB]` / `afe_fet_status`
  - `AFE Register[FF]` / `afe_fet_control`
  - `AFE Register[DD]` / `afe_latch_status`
  - `AFE Register[AA]` / interrupt status if exposed
- Decoded `afe_chg_fet` and `afe_dsg_fet` only from confirmed bit positions.

Purpose: decide whether the AFE actually asserts the charge/discharge FET outputs. Do not derive `afe_pchg_fet` unless its active polarity and mode are separately documented for the field being exposed.

Use the AFE register table and per-bit descriptions together when decoding. In the bq29330 public AFE table, `OUTPUT_CONTROL` bit 0 is `LTCLR`, bit 1 is `DSG`, and bit 2 is `CHG`; do not infer the FET mapping from a compressed prose sentence alone.

### BQ40 safety and configuration state

- `SafetyAlert`, `SafetyStatus`, `PFStatus`.
- `ManufacturingStatus`, especially `FET_EN`, `CHG_TEST`, `DSG_TEST`.
- `FET Options`.
- `DA Configuration`, `Power Config`, and `Protection Configuration`.
- CUV recovery settings when low-voltage recovery is involved.

Purpose: identify why the BQ40 firmware or AFE would refuse to assert FET outputs. Do not blame `OC` or another safety bit unless the bit is both active and causally relevant to the FET that is off.

### Charger-side BAT observation

- BQ25792 `VBAT_PRESENT`.
- `VBAT_ADC`, `VSYS_ADC`, `VBUS/VAC/IBUS/IBAT` ADCs.
- Charger status/fault registers and input path status.

Purpose: prove whether pack voltage reaches the charger BAT node. If AFE DSG is off, missing charger-side VBAT is expected. If AFE DSG is on and charger-side VBAT is still absent, the next suspect is physical path continuity, gate-drive network, MOSFET orientation, connector, or BAT sense routing.

### Runtime output gate

- Requested outputs, active outputs, recoverable outputs.
- `output_gate_reason`.
- TPS per-channel enable/status/fault only after the upstream battery path is proven.

Purpose: prevent impossible UI/runtime states. Any mode that needs TPS output must remain blocked unless the required TPS output is actually active and all upstream gates are proven.

## Diagnostic Matrix

| Evidence | Conclusion | Next data to get |
| --- | --- | --- |
| BQ40 absent | BMS unavailable; no output-required mode may run | SMBus/I2C electrical and address diagnostics |
| BQ40 pack/cells valid, BQ25792 BAT absent, AFE DSG off | BQ40/AFE has not released the discharge path | AFE status/control, safety/PF/manufacturing/config, recovery before/after |
| `OperationStatus()[DSG]=1`, AFE DSG off | Logical state and AFE state disagree; do not call this recovered | AFE latch/interrupt/control and safety/PF reason bits |
| AFE DSG on, BQ25792 BAT absent | BQ40 says it is driving; downstream path is open or not sensed | Measure DSG/CHG gate-source, MOSFET source/drain nodes, PACK/BAT/BQ25792 BAT |
| AFE DSG on, BQ25792 BAT present, TPS inactive | Battery path is available; output runtime/TPS gate is the suspect | TPS status/fault, requested/active outputs, thermal/protection gates |
| Safety/PF active | Protection may be suppressing FET outputs | Decode exact active bit and confirm it applies to charge, discharge, or both |
| EMSHUT active | Emergency shutdown is the primary state | Follow `bq40-emshut-recovery.md` before ordinary XDSG recovery |

## Recovery API Expectations

The BMS discharge authorization recovery API should report a completed firmware-side judgment, not just "command sent". A useful response includes:

- `accepted`
- `result`: `success`, `rejected`, `failed`, or `already_ready`
- `reason`
- `status_before` and `status_after`
- `op_*` logical FET fields
- `afe_chg_fet` and `afe_dsg_fet`
- charger-side `vbat_present` and `vbat_adc_mv`
- output requested/active/gate state

The API must not directly force TPS output. It may only request the existing recovery chain after firmware-side preconditions pass. Success requires the discharge path to be observable after recovery; if AFE DSG remains off, the result is a failure such as `afe_dsg_fet_off`. Raw CDC callers may observe a `pending` intermediate result while the firmware state machine is still running; devd/CLI and device LAN HTTP should wait for the terminal firmware result instead of treating `pending` as failure.

## Guardrails / Reuse Notes

- Do not use `OperationStatus()[DSG]` as the final proof of a physically conducting discharge path.
- Keep field names explicit:
  - `op_dsg_fet` means `OperationStatus()`.
  - `afe_dsg_fet` means AFE register bit decode.
  - `dsg_fet` may be owner-facing shorthand only if it is defined to prefer AFE evidence.
- Do not infer "battery absent" from BQ25792 alone while BQ40 pack/cell telemetry is valid.
- Do not infer "hardware fault" from charger-side BAT absence while AFE DSG is off.
- Do not treat `OC` as a discharge blocker without proving the BQ40/AFE state that actually turns DSG off.
- Do not expose Dashboard or any TPS-output-required mode unless the runtime gate proves the required output path is possible.
- If AFE and charger-side evidence disagree, prefer adding a targeted field or measurement over inventing a narrative.

## Self-review Checklist

Before closing a BQ40 discharge-path investigation:

- The explanation names each observation plane and does not collapse them into one field.
- Every root-cause claim cites the field that proves it.
- `OperationStatus()` and AFE status are separated in field names and UI wording.
- Charger BAT absence is classified as cause only when AFE says the path should be closed.
- Safety/PF bits are treated as candidate causes, not automatic explanations.
- The recovery result is based on `status_after`, not on whether a recovery command was accepted.
- Output mode transitions are checked against `requested_outputs`, `active_outputs`, and `output_gate_reason`.

## References

- `docs/manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md` section 14.1.44, `ManufacturerAccess() 0x0058 AFE Register`
- `docs/datasheets/BQ40Z50-R2/BQ40Z50-R2.md` sections 7.18 and 9.2.2.3.3
- TI bq40z50-R2 Technical Reference Manual, `sluubk0`
- TI bq29330 datasheet, `OUTPUT_CONTROL` register bit descriptions
- `docs/solutions/firmware/bq40-emshut-recovery.md`
