---
title: Source-limited backup takeover
module: firmware
problem_type: power-path-control
component: UPS runtime mode switching
tags:
  - ups
  - runtime-mode
  - source-limited
  - backup
  - assist
status: active
related_specs:
  - docs/specs/xjpvj-runtime-mode-switching/SPEC.md
---

# Source-limited backup takeover

## Context

The UPS has two different failure classes on the input side:

- `input_absent`: upstream input is physically absent or confirmed below the offline threshold.
- `source_limited`: upstream input is still present, but it is current-limited, browned out, or otherwise unable to carry the active load.

Both classes require the UPS to protect the output. They should not be collapsed into the same internal condition.

## Problem

Hardware assist can protect the output before firmware reacts, but it is not a complete mixed-supply strategy on this board.

The observed `12V / 3A source + 3900mA load` sign-off evidence completed successfully, but the assist phase still showed a load-side voltage sag around the `10.5V` level. That is consistent with a hardware path that only starts to share current after a significant voltage difference, possibly through an ideal-diode MOSFET body-diode path before active conduction.

If firmware waits for `VIN` to become physically absent, the output can remain in a deep sag for too long while the upstream supply is merely limited.

## Resolution

Treat `BACKUP` as “UPS has taken over the load”, not only “VIN is gone”.

Keep the owner-facing mode stable:

- `mode=backup`

Expose the cause separately:

- `backup_reason=input_absent`
- `backup_reason=source_limited`

For source-limited takeover, use only MCU-visible input/output signals:

- `vin_vbus_mv`
- `vin_drop_mv`
- `vin_iin_ma`
- `tps_total_iout_ma`
- fresh `tps_total_iout` sample sequencing

Do not use LoadLynx load-side voltage as a firmware trigger. LoadLynx voltage is HIL acceptance evidence only.

## Control Rule

Enter source-limited backup when all of these hold:

- DC input assist is allowed.
- Input is still confirmed online.
- `tps_total_iout_ma` exceeds the configured source-limited enter threshold.
- Either:
  - `VIN` is below `rated_vout_mv - source_limited_recover_margin_mv`; or
  - `vin_drop_mv` exceeds the source-limited drop threshold and `vin_iin_ma` is near the DC input current limit.
- The condition holds for `source_limited_required_samples` fresh samples.

Exit source-limited backup only after hysteresis:

- `VIN` is above the low-VIN threshold.
- `vin_drop_mv` has recovered to half the enter threshold.
- `tps_total_iout_ma` is below the source-limited exit threshold.
- The condition holds for `source_limited_required_samples` fresh samples.

## Reusable Defaults

Use conservative defaults so ordinary assist behavior is not replaced by backup:

- `source_limited_vin_drop_pct=4`
- `source_limited_enter_delta_ma=1900`
- `source_limited_exit_delta_ma=0`
- `source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

With the current rated enter base, the default source-limited enter threshold is `2000mA`. This targets overloaded source cases such as `12V / 3A source + 3900mA load`, not normal 1A-class backup-only validation.

## Observability

Always expose source-limited takeover in both status surfaces:

- `status.input.backup_reason`
- `diag-snapshot.input.backup_reason`

Charger token policy should remain non-charging:

- `backup_reason=input_absent`: `NOAC`
- `backup_reason=source_limited`: `LOAD` plus a source-limited backup notice

This wording prevents operators from misreading a limited-but-present upstream supply as a physical unplug event.

## Validation

For HIL validation, prove three 12V cases separately:

- normal load VIN cut: `12V / 3A source`, `1000mA load`, then cut VIN.
- overload with VIN online: `12V / 3A source`, `3900mA load`, source-limited backup must occur.
- overload then VIN cut: after source-limited backup at `3900mA`, cut VIN and remain in backup without a long deep sag.

Each report must satisfy sample-rate, max-gap, scene-complete, required voltage-series, and freshness gates before it can be used as sign-off evidence.

## Formal 12V Contract

Use `mains-aegis power-validation run --suite-contract source-limited-12v` for
this behavior. The contract is intentionally separate from the legacy
dual-voltage suite so online source limitation is not hidden inside a later
physical source cut.

It produces three reports:

- `backup_only / 1000mA`: prove `input_absent` after a normal VIN cut.
- `source_limited_online / 3900mA`: prove `source_limited` while VIN remains
  online.
- `source_limited_cut / 3900mA`: only cut VIN while the final pre-cut sample
  still proves online takeover, then prove continuous backup and `input_absent`
  reason truth.

For the overloaded online phase, capture the time to source-limited latch,
the rated TPS target observation, the minimum LoadLynx voltage after latch,
and low-voltage durations. Treat load voltage below `11000mV` after latch, or
a pre-latch low-voltage interval longer than one second, as a failed stability
criterion. These are HIL acceptance signals only; firmware must continue to
trigger from UPS-local VIN/current telemetry.

The USB-C low-output charging exception belongs only to confirmed
`input_absent` backup. It must never turn a `source_limited` backup into a
charging state, because the upstream DC source is still present but unsafe.
