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

- `pre_tps_vin_mv` (`vin_vbus_mv` remains a compatibility alias)
- `vin_drop_mv`
- `vin_iin_ma`
- `tps_total_iout_ma`
- fresh `tps_total_iout` sample sequencing

Do not use LoadLynx load-side voltage as a firmware trigger. LoadLynx voltage is HIL acceptance evidence only.

## Control Rule

Enter source-limited backup when all of these hold:

- DC input assist is allowed.
- Input is still confirmed online.
- TPS output is meaningful. The input-handoff path may start at `500mA`; the
  normal TPS path still uses the configured source-limited enter threshold.
- Either:
  - `VIN` is below `rated_vout_mv - source_limited_recover_margin_mv`; or
  - `vin_drop_mv` exceeds the source-limited drop threshold and `vin_iin_ma` is near the DC input current limit.
- The first qualifying fresh sample immediately preboosts the TPS target to
  rated output. A second fresh sample confirms takeover when TPS is still
  carrying at least `500mA`; preboost-induced VIN recovery must not erase the
  first sample's evidence.

Exit source-limited backup only after hysteresis:

- `VIN` is above the low-VIN threshold.
- `vin_drop_mv` has recovered to half the enter threshold.
- `tps_total_iout_ma` is below the source-limited exit threshold.
- The condition holds for `source_limited_required_samples` fresh samples.

## Reusable Defaults

Use defaults that bound detection delay while retaining the fresh-telemetry
guard:

- `source_limited_vin_drop_pct=1`
- `source_limited_enter_delta_ma=2500`
- `source_limited_exit_delta_ma=0`
- `source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

With the current rated enter base, the default source-limited enter threshold is `2600mA`.
Never count the same TPS sample sequence twice. The fast input-handoff path may
preboost at a lower TPS contribution only when the current fresh sample also
shows low VIN and input current at the source-limited threshold.

For 12V output, keep standby support voltage aligned with the soft input floor
instead of leaving a wide gap:

- `standby_drop_mv=700`, which yields `11.3V standby`
- if instability remains, raise the soft input floor first; do not lower the
  `11.3V` standby target as the first reaction

## Pre-TPS undervoltage gate

Measure upstream voltage at INA3221 CH3 `VIN_UNSAFE`, before the TPS2490 input
MOS. Publish it as `pre_tps_vin_mv`; do not describe this value as a post-TPS
measurement.

Use an MCU-controlled hysteretic input gate:

- store the gate in EEPROM-backed `advanced_power` fields:
  `input_uvlo_cutoff_mv`, `input_uvlo_recover_mv`,
  `input_uvlo_required_samples`;
- keep profile defaults conservative:
  - `12V`: `11.3V` cutoff, `11.5V` recover, `3` samples;
  - `19V`: `10V` cutoff, `11V` recover, `3` samples;
- while inside the hysteresis window, retain the current gate state;
- any missing sample resets the consecutive-sample streak.

This protects the UPS from a weak or collapsing upstream supply even though
the physical connector is still energized. TPS2490 power-good remains useful
confirmation, but it is not a substitute for the pre-TPS ADC and MCU gate.

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

For current HIL validation, prove four 12V cases separately:

- normal load VIN cut: `12V / 3A source`, `1000mA load`, then cut VIN.
- in-budget online guard: `12V / 3A source`, `2500mA load`, VIN remains online and Backup must not occur.
- overload with VIN online: `12V / 3A source`, `3900mA load`, source-limited backup must occur.
- overload then VIN cut: after source-limited backup at `3900mA`, cut VIN and remain in backup without a long deep sag.

Each report must satisfy sample-rate, max-gap, scene-complete, required voltage-series, and freshness gates before it can be used as sign-off evidence.

## Formal 12V Contract

Use `mains-aegis power-validation run --suite-contract source-limited-12v` for
this behavior. The contract is intentionally separate from the legacy
dual-voltage suite so online source limitation is not hidden inside a later
physical source cut.

It produces four reports:

- `backup_only / 1000mA`: prove `input_absent` after a normal VIN cut.
- `source_in_budget / 2500mA`: prove that a load within the verified source budget
  remains non-backup while VIN stays online.
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

Formal LoadLynx evidence keeps only one measured total-current field. Do not
invent, preserve, or display local/remote current components in owner-facing
reports.

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

## Standby Target Changes Require Source-Limited Recalibration

Raising the 19V standby target from `17.8V` to `18.2V` improved the ordinary VIN-cut floor, but
also changed how the hardware shared a `3900mA` load. The bench then reported about `2017mA`
VIN input and `2324mA` TPS output while the battery supplied the remainder. A detector calibrated
only around the previous near-3A input-current point therefore remained in standby even though the
source could not carry the load alone.

Treat standby voltage and source-limited admission as one control surface. For the verified 18.2V
standby point, a bounded `80mV` VIN-drop tolerance is acceptable only together with at least
`2000mA` VIN input, meaningful TPS output load, and consecutive fresh samples. This converts the
implicit battery contribution into an explicit `source_limited` Backup decision without allowing
normal 1000mA operation to latch Backup.

The composed suite is retained at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-tuned-final-20260713T0020Z/`.
It passed the voltage and reason checks available at generation time, but is diagnostic-only after
adding the missing hold-power contract. In its ordinary-load hold, `137/158` samples exceeded
`2W` TPS output; the overload holds had `73/80` and `71/79` samples above `2W`. Those samples prove
that the phase labels and acceptance logic were incomplete even though the ordinary cut transition
stayed above `18143mV` and both overload scenes stayed at or above `18744mV` after latch.

Define hold as source-only normal operation, not merely a wall-clock interval after applying load.
Every fresh hold sample must remain at or below `2W` TPS output. The first sample above that limit
starts `transition_source_limited`; a confirmed source-limited latch starts `backup_online`. Never
average post-latch TPS power into hold metrics, and never let successful voltage checks override a
hold-power violation.

## Optimize Against Path State, Not Voltage Alone

The 19V standby path has a discontinuity between `820mV` and `840mV` drop. A normal 1000mA load
produced about `20.389W` TPS output at 820mV, while a short 840mV probe stayed near `1.092W`.
Do not choose the closest passing step: temperature, previous path state, and measurement error can
cross that boundary. The retained product setting is `900mV`, leaving 60mV above the observed edge.

Parameter margin alone was insufficient. With the earlier 2A source-limited input threshold, a
normal load could reach about `1.1A` VIN input near the VIN-drop tolerance and be falsely latched
into Backup. The corrected threshold is `2.3A`: below the observed 3900mA limited-source range of
`2.394–2.411A`, but well above the normal-load range. VIN drop, meaningful TPS output, and
consecutive fresh samples remain mandatory.

The final suite at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-optimized-cut-r3-20260713T0155Z/`
passes the recomputed 2W hold gate. Its three hold maxima are `1089mW`, `1089mW`, and `1016mW`;
the normal VIN-cut minimum is `18049mV`, and both overload post-latch minima are `18744mV`.

The 12V follow-up exposed a second false-positive path. The diagnostic suite at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-c22bf968-20260713T0320Z/`
showed that a normal `1000mA` online hold could still be mis-latched into
`backup_reason=source_limited`, driving `12.607W` TPS output in what should
have remained ordinary standby. The source-limited cut continuity bug was
already fixed at that point; the remaining problem was the admission logic.

Do not let a single transient high `VIN IIN` sample seed a later TPS-only
source-limited latch. Fast enter may still react immediately when the current
sample itself shows `VIN drop + high VIN IIN`, but a later low-current sample
must not inherit that one spike and complete the consecutive counter by itself.
TPS-only source-limited admission now requires either:

- the current sample still meeting the source-limited `VIN IIN` threshold; or
- a sustained online-current history that has already reached the same
  threshold window.

The verified 12V rerun at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-c22bf968-20260713T0335Z/`
confirms the fix: `backup_only`, `source_limited_online`, and
`source_limited_cut` are all `valid_for_signoff`, and the ordinary `1000mA`
hold stays below the `2W` TPS gate with a maximum of `391mW`.

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

## 12V Final Validation Pattern

The repaired-hardware current result is archived at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-ce343924-uvlo-preboost-final-20260714T1206Z/`.
All four scenes are sign-off valid. The 2500mA guard produced no Backup samples.
The 3900mA online and cut scenes both held at least `11790mV` after latch; their
entry delays were `0.601s` and `1.002s`, and the cut scene remained continuously
in Backup before changing reason to `input_absent`.

The subsequent `93aadc61` EEPROM-backed UVLO sweep is archived at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-93aadc61-uvlo-sweep-20260714T1636Z/`.
Candidate A (`11.3V / 11.5V`) and candidate C (`11.5V / 11.7V`) both passed the
four-scene 12V contract, while candidate B (`11.4V / 11.6V`) falsely entered
Backup during the `2500mA` in-budget scene. The practical rule is therefore:
pick the lowest cutoff that still passes the full suite, not the highest cutoff
that seems to hand off earlier in one overload scene. In this sweep, candidate A
remained the recommendation because it matched candidate C's `11790mV`
post-latch floor without increasing false positives or moving Backup earlier
than necessary.

## 19V EEPROM UVLO Sweep Pattern

The repaired 19V path required a new sweep instead of blindly reusing the old
`10V / 11V` software UVLO defaults. With `standby_drop_mv=900` fixed at an
`18.1V` standby target, the retained sweep used:

- candidate A: `18.1V / 18.3V`
- candidate B: `18.2V / 18.4V`
- candidate C: `18.3V / 18.5V`

All three candidates were written to EEPROM, read back immediately, reset, and
read back again before running the three-scene `source-limited-19v` contract.
The retained sign-off evidence is:

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-6bc1a374-uvlo18100-20260715T0310Z/`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-6bc1a374-uvlo18200-20260715T0317Z/`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-6bc1a374-uvlo18300-r3-20260715T0332Z/`

Candidate A passed, but still allowed a visible pre-latch low-voltage interval:
`0.198s` in the online overload scene and `0.400s` in the overload-then-cut
scene. Candidate C also passed, but its online overload entry slowed back down
to `0.999s` even though it removed pre-latch low-voltage time. Candidate B was
the balanced point:

- online overload latch in `0.201s`
- overload-then-cut latch in `0.599s`
- no pre-latch interval below the 19V `18000mV` floor
- same `18768mV` post-latch floor as the other passing points

The practical 19V rule is therefore different from the 12V rule:

- do not always prefer the lowest passing cutoff when it still leaves a visible
  pre-latch low-voltage interval;
- do not keep raising cutoff once the post-latch floor has stopped improving
  and online takeover begins to slow down again.

For this repaired 19V bench, the retained recommendation is:

- `standby_drop_mv=900`
- `input_uvlo_cutoff_mv=18200`
- `input_uvlo_recover_mv=18400`
- `input_uvlo_required_samples=3`
- `source_limited_enter_delta_ma=1000`

The earlier `source-limited-19v-6bc1a374-uvlo18300-r2-20260715T0327Z/` rerun is
retained as diagnostic-only evidence because the parameter behavior passed but
collection failed with `load_collector_error` and a `0.902s` sample gap. Keep
that distinction explicit: telemetry failures must not be rewritten as control
failures, and control passes must not be promoted to sign-off when the
collection contract was broken.

The earlier 62179e3c report below remains the pre-repair baseline. Do not use
its PCB voltage drop or 2900mA guard result as current hardware truth.

The final 12V evidence is archived at
`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-62179e3c-final-r6-20260714T0010Z/`.
It contains four signed-off scenes: 1000mA input-absent cut, 2900mA in-budget online,
3900mA source-limited online, and 3900mA source-limited cut. The suite used USB for all
three devices, retained `tps_cdc_rise_mv=300`, and reported only `load_i_total_ma`.

The validation runner must not treat a control command as a high-rate source telemetry stream.
IsolaPurr `power show` can temporarily fail while its output is disabled because its device
info endpoint is unavailable. Keep its config and `tps_cdc_rise_mv` reads as explicit action
evidence, and use the UPS USB VIN ADC for continuous source-path voltage. If the scene control
task misses a scheduling interval, recover only real UPS frames already received by the collector,
using their timestamps; never synthesize or interpolate a voltage sample.

The firmware-side boundary is equally important: once a collapsed VIN has caused
`input_absent` Backup, do not clear Backup merely because `mains_present` has not yet settled.
Conversely, a `source_limited` latch must retain that reason during the intermediate DCIN window
and convert to `input_absent` only after explicit input absence. This prevents both long output
sags and misleading reason transitions.

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
