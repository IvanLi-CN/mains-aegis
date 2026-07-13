# Runtime-Mode Power Path Validation

This page is the current truth source for `#xjpvj` runtime-mode Power Path Validation on the approved `12V / 3A` and `19V / 3A` benches.

**Power Path Validation** / **电源路径验证** is the owner-facing name. `HIL`
only remains in some historical file and directory names.

It serves three purposes:

- the active operator test contract
- the current sign-off evidence summary
- the reusable runner/tooling guardrails for future reruns

## Current Goal

Under `12V / 3A` and `19V / 3A` input, validate the current runtime-mode implementation against these product goals:

- direct-through-first while the wall source still has real headroom
- `assist_low` only when online under-delivery evidence exists
- `assist_rated` only when sustained online under-delivery is proven after `assist_low`
- `backup` only when input is actually cut
- quantified output-voltage behavior across hold, cut, restore, and unload

## Bench Contract

Accepted bench topology:

- IsolaPurr `fixture-source-device` as the controllable DC input source
- `2 mm banana -> DC5025 -> UPS DCIN`
- LoadLynx `fixture-load-device` on UPS `OUT`
- UPS owner-facing evidence from:
  - `mains-aegis` CLI `status` over devd IPC / UPS USB CDC
  - `mains-aegis` CLI `diag-snapshot` over devd IPC / UPS USB CDC
- IsolaPurr source control/telemetry from the stable IsolaPurr CLI transport
  selected for the bench; LAN HTTP via `--isolapurr-url` is acceptable for the
  source because the UPS and LoadLynx transport restrictions do not apply to
  IsolaPurr

Accepted active input baselines:

- `12V / 3A`
- `19V / 3A`

## Formal Power Path Validation Suite

Current formal suite truth is fixed to four scenes:

- `12V assist_path`
- `12V backup_only`
- `19V assist_path`
- `19V backup_only`

Scene contracts:

- `assist_path`
  - source manual output at the profile voltage with `3000mA` current limit
  - CC load `3900mA`
  - expected path: `standby -> assist_low -> backup -> assist_low -> standby`
- `backup_only`
  - source manual output at the profile voltage with `3000mA` current limit
  - CC load `1000mA`
  - expected path: `standby -> backup -> standby`

Load protection is fixed for all suite scenes:

- `UVP=3000mV`
- `OCP=4000mA`
- `OPP=80000mW`

Hard power-off gate for any `12V <-> 19V` artifact switch:

1. disable the LoadLynx output
2. prove the chosen IsolaPurr target is reachable before the first `port_c`
   write of this switch / flash path
3. cut IsolaPurr `port_c`
4. confirm the UPS is no longer fed by the external source
   - required UPS-side cut truth:
     - `input.vin_vbus_mv <= 2999`
     - `input.mains_present == false`
     - `mode=backup` or `input.assist_power_stage=backup`
   - `input.input_vbus_mv` or steady USB `5V` presence is not a failure here
   - do not treat IsolaPurr ack by itself as sufficient proof
5. only then select/flash the next artifact
6. after boot, keep `port_c` off, verify USB + IPC capability truth, then
   restore the new source voltage before the next scene
   - `DCIN` must remain unpowered until UPS `output_profile` and
     `rated_vout_mv` are confirmed for that scene
   - this same order also applies to non-flash profile changes; “not flashing”
     is not a reason to skip the load-off and source-cut steps

This gate is mandatory for both:

- artifact select + flash
- any other firmware switch that changes the output-voltage profile

## Capability Gate Before DCIN Power

Before any formal Power Path Validation scene is allowed to energize `DCIN`, the runner must prove the UPS hardware profile first.

Mandatory sequence:

1. disable LoadLynx output
2. prove the chosen IsolaPurr target is reachable before the first `port_c`
   write of the scene
3. force IsolaPurr `port_c` off
4. prove the UPS has actually detached from external `DCIN`
   - required UPS-side cut truth:
     - `input.vin_vbus_mv <= 2999`
     - `input.mains_present == false`
     - `mode=backup` or `input.assist_power_stage=backup`
   - USB-C host power / communication may remain attached and must not be
     treated as a blocker for `12V <-> 19V` firmware switching
5. read UPS `identity`
6. read UPS `settings`
7. verify the actual hardware capability matches the intended scene profile
8. only then program the IsolaPurr source voltage/current limit
9. read back the programmed IsolaPurr source configuration while `port_c` is still off
10. only then re-enable `port_c`

This is a per-scene gate, not only a per-profile-switch gate:

- every formal scene must execute this sequence again before entering `pre`
- the runner must not inherit capability or source-configuration trust from the
  previous scene, even if the previous scene used the same `12V` or `19V`
  profile

Required capability truth:

- `identity.hardware_capabilities.output_profile`
- `identity.hardware_capabilities.rated_vout_mv`
- `settings.advanced_power_capabilities.rated_vout_mv`

UPS capability confirmation is USB + IPC combined truth:

- USB identity is the required source for `output_profile`
- USB identity and CLI/devd IPC settings together must agree on `rated_vout_mv`
- `DCIN` must not have source power before that `output_profile` /
  `rated_vout_mv` agreement is confirmed
- until that agreement is proven, the runner must not energize `DCIN`

Each observation surface has one formal job:

- CLI/devd IPC `status`
  - prove the source cut really changed live UPS runtime truth
  - confirm `input.vin_vbus_mv`, `input.mains_present`, `mode`, and
    `input.assist_power_stage`
- USB `identity`
  - prove the attached UPS hardware capability currently reports the intended
    `output_profile`
  - provide one side of the `rated_vout_mv` agreement
- CLI/devd IPC `settings`
  - prove the owner-facing live settings surface reports the same
    `rated_vout_mv`

None of those surfaces can substitute for the others:

- do not use cached devd device listings as a replacement for CLI/devd IPC
  `status`
- do not use settings alone to infer `output_profile`
- do not use USB `5V` presence or `input.input_vbus_mv` to waive the external
  `DCIN` cut proof

The runner must reject the scene before `port_c` is re-enabled if any of the following is true:

- the UPS-side cut truth is not satisfied after `port_c` is forced off
- the chosen IsolaPurr source target is unreachable before any `port_c` write
- the identity capability block is missing
- the settings capability block is missing
- the actual UPS profile is `19V` but the scene is `12V`
- the actual UPS profile is `12V` but the scene is `19V`
- the requested source voltage does not match the validated UPS rated output profile
- the source configuration readback does not match the requested voltage/current limit
- `port_c` becomes enabled before the source configuration gate completes

When source reachability fails, the runner must:

- keep `port_c` untouched
- return an explicit gate-failure report
- avoid crashing inside the safe-prepare step
- record the IsolaPurr CLI source-status probe result and the requested source
  configuration

This is a hard safety gate, not an operator hint.

## Run Validity Contract

One Power Path Validation run is considered effective only when:

- `summary.all.acceptance.run_validity == valid_for_signoff`

Anything else is:

- `invalid_diagnostic_only`

Minimum acceptance for one complete run:

- full expected scene structure exists
  - non-backup scene: `pre -> hold -> post`
  - backup scene: `pre -> hold -> backup -> restore -> post`
- full-scene sampling quality passes
  - `effective_sample_rate_hz >= 2.0`
  - `max_sample_gap_s <= 0.5`
- required voltage series are present
  - source output voltage
  - UPS `DCIN` voltage
  - UPS INA `VOUT`
  - load actual voltage
- if the scene includes a source cut / backup section
  - formal UPS truth must come from direct UPS `status`, not from a cached devd
    devices listing projection
  - once `port_c_enabled=false`, the UPS evidence must show a real cut response
    through at least one of:
    - `mains_present=false`
    - `mode=backup`
    - `assist_power_stage=backup`
  - UPS `DCIN` voltage must also move with the cut
- `failed_acceptance_checks` is empty

Any one failure above vetoes the whole run.

Realtime freshness fields remain required diagnostics:

- `load_status_max_age_s`
- `source_status_max_age_s`
- `ups_status_max_age_s`
- `diag_snapshot_max_age_s`

They are still recorded and reviewed, but they no longer independently veto an
otherwise complete formal run once continuous sampling and source-cut semantics
already pass.

## Required Evidence Surfaces

Each effective scene must retain synchronized evidence from:

- IsolaPurr source telemetry
- UPS CLI/devd IPC `status`
- UPS CLI/devd IPC `diag-snapshot`
- LoadLynx USB telemetry

The scene is not acceptable if any of the four surfaces goes stale or absent beyond the run-validity contract.
The scene is also not acceptable if the source is cut but the UPS-side runtime
truth remains logically frozen.

## Current Sign-Off Evidence

Current accepted formal sign-off suite:

- suite summary:
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-summary.json`
- suite overview:
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-overview.html`
- Rust-composed suite summary:
  - `tools/hil/reports/composed-current-four-scenes/suite-summary.json`
- Rust-composed suite overview:
  - `tools/hil/reports/composed-current-four-scenes/suite-overview.html`

Current status of that suite under the updated contract:

- all four scenes are acceptable as formal sign-off evidence
- each scene has `run_validity=valid_for_signoff`
- each scene has `signoff_valid=true`
- the raw scene directories can be recombined by
  `mains-aegis power-validation compose`
- the composed suite preserves links to the raw scene directories and is accepted
  by the same Rust sign-off verifier used by `power-validation report`
- `12V assist_path`
  - report directory: `12v-assist_path-3900ma`
  - load target: `3900mA`
  - effective sample rate: `5.103Hz`
  - max sample gap: `0.227s`
- `12V backup_only`
  - report directory: `12v-backup_only-1000ma`
  - load target: `1000mA`
  - effective sample rate: `5.110Hz`
  - max sample gap: `0.224s`
- `19V assist_path`
  - report directory: `19v-assist_path-3900ma`
  - load target: `3900mA`
  - effective sample rate: `5.057Hz`
  - max sample gap: `0.268s`
- `19V backup_only`
  - report directory: `19v-backup_only-1000ma`
  - load target: `1000mA`
  - effective sample rate: `5.093Hz`
  - max sample gap: `0.250s`

Current `advanced_power` snapshot used by the accepted sign-off report:

- `standby_drop_mv=1200`
- `assist_low_drop_mv=600`
- `assist_enter_delta_ma=0`
- `assist_exit_delta_ma=0`
- `assist_required_samples=2`
- `assist_ramp_step_mv=100`
- `assist_ramp_interval_ms=200`
- `rated_enter_delta_ma=0`
- `rated_exit_delta_ma=0`
- `vin_drop_threshold_pct=4`
- `required_samples=2`

Current dual-voltage suite execution status:

- current suite orchestrator is the Rust host command:
  - `mains-aegis power-validation run`
  - `just power-validation run`
- current suite verifier is the Rust host command:
  - `mains-aegis power-validation report --write-overview <suite-dir>`
  - it must verify suite summary, every scene `results.json`, raw
    `timeseries.jsonl` row counts, required voltage-series presence, sampling
    thresholds, and chart existence before an overview is considered valid
- current suite composer is the Rust host command:
  - `mains-aegis power-validation compose --suite-id <id> --output-dir <dir> <scene-dir>...`
  - it is used when raw scene result directories already exist and need a new
    combined summary/overview
  - it does not copy, edit, or synthesize raw samples
- current adapter protocol reference is the Rust host command:
  - `mains-aegis power-validation adapter-protocol`
- Python files under `tools/hil/` are migration references, chart helpers, or
  focused diagnostics, not the owner-facing suite entry
- four-scene execution is gated by the live safety checks above
- current accepted historical suite id:
  - `formal-12v-19v-four-scenes-cli-r4`
- current accepted raw scene suite id:
  - `formal-12v-19v-four-scenes-current-20260629T024800Z`
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-summary.json`
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-overview.html`
  - accepted by `mains-aegis power-validation report --write-overview`
  - all four scenes are `valid_for_signoff`
  - transport is UPS `cli+ipc+usb`, LoadLynx `cli+ipc+usb`, IsolaPurr
    `cli+ipc+usb`
- current accepted Rust-composed suite id:
  - `composed-current-four-scenes`
  - `tools/hil/reports/composed-current-four-scenes/suite-summary.json`
  - `tools/hil/reports/composed-current-four-scenes/suite-overview.html`
  - generated by `mains-aegis power-validation compose` from the current raw
    scene directories
- historical accepted Rust suite retained for comparison:
  - `power-validation-rust-four-scenes-url-r7`
  - transport is UPS `cli+ipc+usb`, LoadLynx `cli+ipc+usb`, IsolaPurr
    `cli+url`
- IsolaPurr source telemetry should use the stable CLI transport selected for
  the bench; if USB/devd returns `device did not respond to IsolaPurr info`,
  use the explicit IsolaPurr URL transport instead of treating that USB path as
  mandatory

Historical single-scene report retained for comparison:

- `tools/hil/reports/20260624T150204Z-formal-12v-3900-corrected-rerun-r16-lanmonitor/results.json`

## Current Behavioral Truth

What the current accepted sign-off suite proves:

- the current tooling can maintain continuous source / UPS / load sampling density
  across `12V` and `19V`
- the current suite overview was regenerated from the verified suite summary and
  corresponds to four `valid_for_signoff` reports with matching raw
  `timeseries.jsonl`
- the runner enforces UPS capability confirmation before `DCIN` is energized
- the runner can safely switch between `12V` and `19V` artifacts only after
  load disable, source cut, UPS-side cut proof, fresh identity/settings reads,
  and source readback

What the current sign-off suite does not prove:

- it does not by itself freeze all future `advanced_power` defaults forever
- it does not remove the need for future reruns if bench topology or host-tool versions change

## Current Tooling Lessons

### 1. Formal truth comes from the report, not only from the chart

The HTML chart is evidence presentation. Acceptance truth comes from:

- `results.json`
- `summary.json`
- raw `timeseries.jsonl`

If chart continuity and report continuity disagree, trust the report and raw scene data first, then fix the renderer.

### 1.5. Formal UPS status must not come from devd devices listing cache

For formal scene capture:

- UPS runtime truth must come from direct UPS `status`
- UPS diagnostics truth must come from direct devd `diag-snapshot`
- `devd /api/v1/devices` listing data may be useful for discovery or seeding, but
  it is not acceptable as the primary runtime truth surface for cut/restore semantics

### 2. LoadLynx freshness must stay on one live ownership path

Current passing formal evidence depends on avoiding self-conflict between:

- the live LoadLynx status poller
- fallback direct status verification commands

The runner must reuse the active live lease/status path instead of opening a second competing read path against the same USB device.

The formal LoadLynx telemetry path is now `status-stream` over the same selected
USB/devd transport. Supplying `--load-ipc` does not mean "use slow IPC polling";
it means "run `status-stream` through this IPC endpoint". Fallback polling is
diagnostic unless it separately proves the same sampling contract.

Formal sampling thresholds are fixed:

- target cadence: about `3Hz`
- minimum accepted cadence: `>=2Hz`
- maximum accepted gap: `<=0.5s`

Before any combined formal scene starts, the operator or runner must prove all
required live telemetry paths meet that contract:

- UPS direct `status --watch`
- UPS direct `diag-snapshot --watch`
- LoadLynx USB `status-stream`
- IsolaPurr source telemetry

If one path fails, the run is not valid for sign-off even if the chart can be
drawn.

Current Rust readiness command:

```bash
mains-aegis --ipc .tmp/mains-aegis-devd-power-validation.sock \
  power-validation check \
  --isolapurr-cli "$ISOLAPURR_CLI" \
  --isolapurr-url "$ISOLAPURR_URL" \
  --load-cli "$LOADLYNX_CLI" \
  --load-ipc .tmp/loadlynx-devd-power-validation.sock \
  --samples 12
```

Current observed result:

- UPS `status`: `5.027Hz`, max gap `0.201s`, pass
- UPS `diag-snapshot`: `4.998Hz`, max gap `0.202s`, pass
- LoadLynx: `5.0Hz`, max gap `0.213s`, pass
- IsolaPurr USB/devd: fail, `device did not respond to IsolaPurr info`
- IsolaPurr URL transport: allowed and should be used for the next run

Current USB/devd IPC proof:

- UPS `status --watch --interval-ms 250 --watch-freshness-ms 750 --samples 40`:
  `4.0Hz`, max gap `272ms`, no missed or stale rows
- UPS `diag-snapshot --watch --interval-ms 250 --watch-freshness-ms 750 --samples 40`:
  `4.0Hz`, max gap `283ms`, no missed or stale rows
- LoadLynx `status-stream --interval-ms 250 --count 40`: about `3.99Hz`, max
  gap `280ms`

The host must keep status-derived `diag_snapshot` timestamps synchronized with the
status timestamp. A fresh derived diagnostic with a stale `diag_snapshot_updated_at`
is a host bug, not a device telemetry failure.

### 3. Source-cut rows must be evaluated by cut-state truth

When IsolaPurr input is intentionally cut:

- the runner must use `power output auto` for source cut
- `status=not_inserted`
- `voltage/current = null`

are valid cut-state facts, not missing telemetry.

`power config set --usb-c-path disconnected` is not a source-cut action for the
bench output. It only changes the USB-C path and cannot prove that the banana
jack / TPS high-voltage output is off. Formal source-cut truth comes from UPS
runtime evidence: `source=dcin` must clear or the high-voltage VIN reading must
fall out of the active profile voltage range. USB-C 5V management power must
not be treated as a failed DCIN cut.

### 4. Runtime-mode reruns must quantify output voltage explicitly

For current-board runtime-mode work, every accepted rerun must preserve:

- source output voltage
- UPS `DCIN`
- UPS INA `VOUT`
- load actual voltage

This is mandatory because runtime-mode conclusions are not acceptable without output-voltage behavior.

### 5. Dual-voltage suite reports must carry profile-specific metadata

Every formal suite scene now has to record:

- `output_profile=12v|19v`
- `scene_type=assist_path|backup_only`
- `source_voltage_mv`
- `source_current_limit_ma`
- `load_min_v_mv`
- `load_max_i_ma_total`
- `load_max_p_mw`
- selected artifact identity before the scene

The suite verifier must check source-voltage windows by profile:

- `12V`: `11000..12500mV`
- `19V`: `18000..19500mV`

## Operator Checklist

Before treating any future rerun as valid, confirm in this order:

1. `mains-aegis power-validation report --write-overview <suite-dir>` passes
2. each scene has `run_validity == valid_for_signoff`
3. expected phases complete
4. sample rate and gap thresholds pass
5. all required voltage series are present
6. `timeseries.jsonl` row counts match `results.json.samples`
7. `failed_acceptance_checks` is empty
8. freshness diagnostics are recorded and reviewed, even though they are no longer standalone veto gates

If any one item fails, the rerun is diagnostic-only.

## Source-limited 12V Suite

Use the dedicated contract when validating MCU takeover of an upstream source
that remains connected but cannot sustain the load:

```bash
mains-aegis power-validation run \
  --suite-contract source-limited-12v \
  --ups-device <ups-device-id> \
  --power-device <isolapurr-device-id> \
  --load-device <loadlynx-device-id> \
  --load-cli <loadlynx-cli>
```

The contract always runs exactly four 12V scenes with a `12000mV / 3000mA`
source and `3000mV / 4000mA / 80000mW` load protection rails:

Before enabling the source, the runner reads the selected UPS identity and
settings. It requires the 12V profile with `rated_vout_mv=12000` and the
following source-limited settings: `enter_delta=2500mA`, `exit_delta=0mA`,
`required_samples=2`, `recover_margin=400mV`, and `vin_drop_pct=1`.

- `backup_only / 1000mA`: physical VIN cut must yield `backup_reason=input_absent`.
- `source_in_budget / 2900mA`: VIN remains online for the whole scene. The UPS
  must not publish `mode=backup`, `assist_power_stage=backup`, or
  `backup_reason=source_limited`; any such sample blocks scene and suite sign-off.
- `source_limited_online / 3900mA`: LoadLynx applies `CC 3900mA` while
  IsolaPurr remains at `12000mV / 3000mA`. This deliberately exceeds upstream
  capability and verifies that UPS backup supplies the missing load current.
  VIN stays connected and the UPS must enter `backup_reason=source_limited`
  within two seconds of the load transition.
- `source_limited_cut / 3900mA`: uses the same `CC 3900mA` stimulus. The runner must observe source-limited backup
  in its final pre-cut sample before cutting VIN, then require continuous backup
  and `input_absent` truth.

For source-limited scenes, the report stores `backup_reason`, charger state,
the source-limited entry time, pre/post-latch low-voltage durations, and the
post-latch LoadLynx voltage minimum. Formal acceptance additionally requires
the post-latch load voltage to remain at or above `11000mV`; any pre-latch
sub-`11000mV` interval may not exceed one second.

LoadLynx report telemetry uses one owner-facing measured total current field,
`load_i_total_ma`. Reports must not synthesize or display local/remote current
components.

## References

- `docs/specs/xjpvj-runtime-mode-switching/SPEC.md`
- `docs/specs/xjpvj-runtime-mode-switching/IMPLEMENTATION.md`
- `docs/solutions/firmware/runtime-mode-hil-with-isolapurr-loadlynx.md`
- `tools/hil/README.md`
- `tools/hil/advanced_power_12v_runner.py`
- `tools/hil/render_voltage_chart_html.py`
