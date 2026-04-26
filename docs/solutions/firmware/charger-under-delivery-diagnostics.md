---
title: Charger under-delivery diagnostics
module: firmware
problem_type: observability
component: BQ25792 charger policy
tags:
  - charger
  - bq25792
  - manual-charge
  - usb-pd
status: active
related_specs:
  - docs/specs/eu2b8-bq25792-charge-policy/SPEC.md
  - docs/specs/zp4cg-manual-charge-dashboard/SPEC.md
---

# Charger under-delivery diagnostics

## Context

The charger policy can correctly program a target `ICHG` while the BQ25792 still delivers less current than requested. In that case the target value, the applied register value, the BMS current, and the charger ADC current must be kept separate.

## Symptoms

- Manual charge is set to `1A`, and charger logs show `policy_target_ichg_ma=Some(1000)` plus `ichg_ma=Some(1000)`.
- The USB-C PD sink may renegotiate to a higher contract, such as a PPS contract around `17.4V / 1.3A`.
- `IBAT_ADC` and BMS current remain near the previous `500mA` class current.
- `IINDPM_STAT` or `VINDPM_STAT` is asserted, meaning the charger is reducing charge current because the input path is in regulation.

## Root Cause

`ICHG` is only the requested fast-charge current limit. It is not a guarantee that the battery will receive that current. BQ25792 input DPM, external/current-limit path state, source behavior, and power-path conditions can all reduce the delivered battery current below the programmed target.

The failure mode is easy to miss if diagnostics only report over-limit mismatches. Under-delivery needs a separate signal that records both target and actual current with the limiter state.

## Resolution

- Keep manual charge target propagation separate from actual delivered current in logs and UI.
- Add a stable under-delivery diagnostic when target `ICHG` exceeds `IBAT_ADC` or positive BMS current by a clear margin for several polls.
- Include the limiter context in the diagnostic: `IINDPM/VINDPM`, PD contract, programmed input current limit, `REG03`, `REG06`, `REG10`, and `REG14`.
- Classify input-DPM under-delivery distinctly, for example `reason=charge_under_target_input_dpm`.

## Guardrails / Reuse Notes

- Do not treat a low actual current as proof that manual charge preferences failed to apply. First verify target propagation, register writes, and PD demand.
- Do not display target `ICHG` as actual battery current. Prefer `IBAT_ADC`; use BMS current as corroborating telemetry.
- Do not “fix” under-delivery by raising limits blindly. Confirm whether `IINDPM/VINDPM`, external ILIM, PD source behavior, or the power path is the limiting factor.
- Keep the diagnostic rate-limited and require a short hold period so startup renegotiation transients do not flood the monitor.

## References

- `firmware/src/output/mod.rs`
- `firmware/src/output/pure.rs`
- `firmware/src/bq25792.rs`
- `docs/datasheets/BQ25792/BQ25792.md`
