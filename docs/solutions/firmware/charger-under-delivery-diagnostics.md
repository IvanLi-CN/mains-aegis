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
- During low-voltage recovery, `policy.status=RECOV` and `policy.recovery_stage=bq40_pchg|bq25792_precharge`; the intentionally low delivered current must not be diagnosed as a failed `500mA` charge request.

## Root Cause

`ICHG` is only the requested fast-charge current limit. It is not a guarantee that the battery will receive that current. BQ25792 input DPM, external/current-limit path state, source behavior, and power-path conditions can all reduce the delivered battery current below the programmed target.

The failure mode is easy to miss if diagnostics only report over-limit mismatches. Under-delivery needs a separate signal that records both target and actual current with the limiter state.

In this project another concrete failure mode was a BQ25792 register byte-order mismatch. The 16-bit configuration words such as `REG01`, `REG03`, and `REG06` are transferred MSB first. Writing `REG03=1000mA` little-endian produces a readback like `0x6400`, which decodes to `0mA` even though software may log the attempted target as `1000mA`.

## Resolution

- Keep manual charge target propagation separate from actual delivered current in logs and UI.
- Add a stable under-delivery diagnostic when target `ICHG` exceeds positive `IBAT_ADC` or positive BMS current by a clear margin for several polls; negative battery current is discharge and must count as zero delivered charge current.
- Include the limiter context in the diagnostic: `IINDPM/VINDPM`, PD contract, programmed input current limit, `REG03`, `REG06`, `REG10`, and `REG14`.
- Decode `REG03/REG06` readback from the same bytes the device stores, not from the software value returned by the setter. If readback shows byte-swapped values such as `0x6400` or `0x2c00`, fix the register transfer order before chasing external power limits.
- Decode and log BQ25792 `REG08` alongside low-voltage recovery. The expected Mains Aegis baseline is `VBAT_LOWV=71.4%` and `IPRECHG=120mA`; `power-diag` reports this as `charger.vbat_lowv_pct_x10=714` and `charger.iprechg_ma=120`.
- Classify input-DPM under-delivery distinctly, for example `reason=charge_under_target_input_dpm`.
- When the input source is `dcin`, treat `TPS55288` total output current as the first stop criterion. If `out_a_iout_ma + out_b_iout_ma > 100mA`, stop charging immediately, surface `pressure_reason=tps_output_current` plus `limit_reason=pressure_tps_output_current|cooldown_retry_wait`, and carry the measured `tps_total_iout_ma` with the fixed threshold `100mA` through status, power-diag, and power events so CLI/Web can explain the stop.
- Keep `UPS VIN / INA3221 CH3` drop and BQ25792 `VINDPM/IINDPM/POORSRC` as secondary pressure signals. They still matter for observability, but they no longer outrank the `TPS` stop rule for `dcin`.
- Split input-limit programming by source. `dcin` must program `IINDPM=1000mA` and `VINDPM=measured_input_voltage*96%` using current `vin_vbus_mv` first and stable `vin_baseline_mv` as fallback. `usb_c` must continue to use the negotiated PD current limit and the existing contract-based `VINDPM` policy.
- For Wi-Fi/LAN-only observation paths, allow host-side `power-diag` derivation from `/api/v1/status` when `/api/v1/power-diag` is unavailable, but keep the derived payload source-tagged so the operator can tell it came from `lan_derived`.
- Mirror the diagnostic and manual `START/STOP` events to the plain serial monitor when the field workflow does not decode defmt, and rate-limit sustained under-delivery output so live monitoring remains readable.

## Guardrails / Reuse Notes

- Do not treat a low actual current as proof that manual charge preferences failed to apply. First verify target propagation, register writes, and PD demand.
- Do not display target `ICHG` as actual battery current. Prefer `IBAT_ADC`; use BMS current as corroborating telemetry.
- Do not treat `BQ25792 termination_done` as full-charge evidence while `policy.status=RECOV` or `cell_min_mv < 3000`; that state is a low-voltage recovery window, not top-off completion.
- Do not “fix” under-delivery by raising limits blindly. Confirm whether `IINDPM/VINDPM`, external ILIM, PD source behavior, or the power path is the limiting factor.
- Do not attribute a `dcin` stop to `vin_drop` when `TPS` output current already crossed the `100mA` threshold in the same window. The owner-facing root cause must stay `tps_output_current`.
- Keep the diagnostic rate-limited and require a short hold period so startup renegotiation transients do not flood the monitor.

## References

- `firmware/src/output/mod.rs`
- `firmware/src/output/pure.rs`
- `firmware/src/bq25792.rs`
- `docs/datasheets/BQ25792/BQ25792.md`
