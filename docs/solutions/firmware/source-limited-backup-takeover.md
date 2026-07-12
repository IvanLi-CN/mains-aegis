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

- `input_absent`: upstream input is physically absent, confirmed below the offline threshold, or has collapsed from an established online baseline so it can no longer supply the load.
- `source_limited`: upstream input is still present, but it is current-limited, browned out, or otherwise unable to carry the active load.

Both classes require the UPS to protect the output. They should not be collapsed into the same internal condition.

## Problem

Hardware assist can protect the output before firmware reacts, but it is not a complete mixed-supply strategy on this board.

The observed `12V / 3A source + 3900mA load` sign-off evidence completed successfully, but the assist phase still showed a load-side voltage sag around the `10.5V` level. That is consistent with a hardware path that only starts to share current after a significant voltage difference, possibly through an ideal-diode MOSFET body-diode path before active conduction.

If firmware waits for `VIN` to become physically absent, the output can remain in a deep sag for too long while the upstream supply is merely limited.

The physical source-cut case needs the same protection. A source can remain above the
presence threshold while its voltage and available current have already collapsed. In
that interval, treating it as a viable online source delays UPS takeover without
providing useful input power.

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

Use defaults that bound detection delay while retaining the fresh-telemetry
guard:

- `source_limited_vin_drop_pct=1`
- `source_limited_enter_delta_ma=1000`
- `source_limited_exit_delta_ma=0`
- `source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

With the current rated enter base, the default source-limited enter threshold is `1100mA`. A fresh TPS sample still requires the full output-current and input-current evidence. When that aggregate TPS sample is stale, a fast path may use only `VIN baseline/drop` plus `vin_iin_ma` to lock takeover; it must never replace the normal fresh-sample path.

## Observability

Always expose source-limited takeover in both status surfaces:

- `status.input.backup_reason`
- `diag-snapshot.input.backup_reason`

Charger token policy should remain non-charging:

- `backup_reason=input_absent`: `NOAC`
- `backup_reason=source_limited`: `LOAD` plus a source-limited backup notice

This wording prevents operators from misreading a limited-but-present upstream supply as a physical unplug event.

## Input-collapse Rule

For a source that was previously online, enter `backup_reason=input_absent` before the
fixed offline threshold when all of these are true:

- `mains_present=true`.
- a DCIN `VIN baseline` is available.
- `VIN <= 85%` of that baseline.
- `vin_iin_ma` is below the configured source-limited entry threshold.

This is a supply-loss classifier, not an electronic-load trigger. It uses only UPS-local
telemetry and requires a prior baseline, so an initially unknown or weak input does not
become backup merely because TPS is enabled.

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

## HIL-confirmed behavior

The `source-limited-12v` contract passed on a 12V build with a `12V / 3A`
source and `3900mA` electronic load. The online scene latched source-limited
backup with no measured interval below `11000mV`; the following VIN cut stayed
in backup and changed the reason to `input_absent`.

This is an improvement over the older assist-path observation, where the same
overload class could remain around `10.5V` at the load while VIN was still
online. It does not prove that the hardware is a mixed-supply topology; it
proves the MCU can limit the duration of that hardware-only fallback.

A repeat of the complete contract after replacing the upstream supply feeding
IsolaPurr also passed all three scenes. With IsolaPurr still configured for
manual `12V / 3A`, a `3900mA` CC load latched source-limited in `0.400s` and
`0.406s`; the respective post-latch minimum load voltages were `11743mV` and
`11731mV`, with no interval below `11000mV`. The retained evidence is
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260712T0759Z/`.
The test runner preserved IsolaPurr `tps_cdc_rise_mv=300` before and after the
run.

The dedicated 19V contract also passed. With IsolaPurr at manual `19V / 3A`
and LoadLynx at `3900mA` CC, online source limitation latched in `0.097s`; the
following source-cut case latched in `0.203s`, stayed in backup, and changed
the cause to `input_absent`. Both post-latch minima were `18732mV`, above the
19V acceptance floor of `18000mV`. The retained evidence is
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-20260712T1020Z/`.

The 19V input drop was `168mV` at `2760mA` input current and `1368mA` TPS
output current. That is just below the percentage-derived drop threshold after
normal ADC and wiring error. Keep a bounded `60mV` VIN-drop tolerance only in
the source-limited qualifier; retain the independent TPS, input-current, and
consecutive-sample gates so the tolerance cannot turn a normal online source
into backup.

## 19V Input-Collapse Evidence

The dedicated `19V / 3A source + 1000mA load` VIN-cut evidence is retained under
`docs/specs/xjpvj-runtime-mode-switching/evidence/input-collapse-19v-backup-only-r7-20260712T1320Z/`.
It uses the 19V `main-vout-19v` build with the input-collapse rule above.

The final run is `valid_for_signoff` at `4.967Hz` with a maximum sample gap of
`0.401s`, all required voltage series, and no acceptance failures. Its first recorded
backup sample had `VIN=9.696V` and `vin_iin=108mA`, while `mains_present` was still
online; `backup_reason=input_absent` was published immediately rather than waiting for
the old sub-3V rule.

The first control-only pass retained `standby_drop_mv=1200`, leaving a `17.8V` hot-standby
target and a minimum load sample of `17.742V`. Reducing only `standby_drop_mv` to `800`
raised hot standby to `18.2V`; the final signed-off run reached a minimum of `18.155V`
and had no sample below `18.0V`. This validates a smaller hot-standby differential for
this bench, not a claim that all analog-path switching transient has disappeared.

The runner read `tps_cdc_rise_mv=300` before and after each run. It did not change that
source compensation setting; IsolaPurr was left at manual `19000mV / 3000mA` with output
disabled after completion.

## Final 19V Revalidation

The final three-scene 19V suite is retained at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-final-r7-20260712T1441Z/`.
It used the `main-vout-19v` build `0c98fe9d-dirty-fee0c84b3135d707`, manual
`19000mV / 3000mA` source, and `3900mA` CC overload scenes. The suite verifier returned
`signoff_valid=true`.

The online overload latched in `0.201s`; the overload-then-cut scene latched in `0.401s`,
stayed in backup through the physical cut, and changed its reason to `input_absent`. Both
post-latch minima were `18744mV`, with no interval below the 19V `18000mV` floor.

One preceding r6 run produced a post-latch `17589mV` sample for `0.303s` in the cut scene.
Retain such failed evidence instead of averaging it away or relaxing the contract. A subsequent
full rerun that passes every scene is the sign-off candidate; the failed run remains the boundary
for future TPS transient work.

## Retained Diagnostic Evidence

Keep complete rerun evidence with the implementation, even when a collection
gate prevents sign-off. The 12V three-scene rerun is archived at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260712T0300Z/`.
It includes the suite overview, each scene's raw result, full time series, and
interactive voltage chart.

In that rerun, `backup_only` and `source_limited_cut` were sign-off valid. The
online overload scene observed the intended source-limited takeover and passed
its functional assertions, but a `0.507s` sample gap exceeded the `0.5s`
collection contract. Treat the complete suite as diagnostic evidence, not a
replacement sign-off. Retaining it allows later changes to distinguish output
control regressions from telemetry-completeness regressions.

## Bench and Telemetry Lessons

- A source control command must operate the physical banana/TPS output gate.
  For the verified IsolaPurr path that is `power runtime output --enabled`, not
  an automatic-output or USB-C-path configuration command.
- Start every scene only after UPS status confirms an online non-backup state.
  Otherwise a previous source-limited latch invalidates the next scene's entry
  timing.
- Do not include explicitly stale UPS status frames in formal telemetry.
- Measure source-limited entry from actual LoadLynx CC telemetry, not from the
  host command subprocess start time.
- UPS and load collectors are asynchronous. Treat the source-limited decision
  sample as pre-latch status evidence and begin post-latch voltage acceptance
  on the subsequent aligned sample.
