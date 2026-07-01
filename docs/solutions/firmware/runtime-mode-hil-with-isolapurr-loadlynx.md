---
title: Runtime-mode Power Path Validation with IsolaPurr and LoadLynx
module: firmware
problem_type: hardware-validation
component: UPS runtime mode switching
tags:
  - ups
  - power-validation
  - runtime-mode
  - isolapurr
  - loadlynx
status: active
related_specs:
  - docs/specs/xjpvj-runtime-mode-switching/SPEC.md
  - docs/specs/p8k3d-mains-aegis-devd/SPEC.md
---

# Runtime-mode Power Path Validation with IsolaPurr and LoadLynx

## Context

`#xjpvj` runtime-mode work needs one bench that can independently stimulate:

- UPS input side
- UPS output load side
- owner-facing evidence from the UPS itself

The current accepted bench is:

- IsolaPurr as the controllable DC input source
- LoadLynx as the controllable electronic load
- UPS CLI/devd IPC `status`
- UPS CLI/devd IPC `diag-snapshot`

Current active formal source baselines are:

- `12V / 3A`
- `19V / 3A`

## Symptoms

Before the current cleanup, runtime-mode Power Path Validation repeatedly produced misleading conclusions because several different failure classes were mixed together:

- partial scene capture was treated as product evidence
- chart rendering gaps were mistaken for missing raw data
- competing LoadLynx status paths created fake `offline / stale / timeout` failures
- intermediate exploratory parameter conclusions were allowed to leak into long-lived docs as if they were current truth

## Root cause

The recurring root cause was not a single firmware bug. It was a verification-discipline bug:

- formal Power Path Validation truth was not consistently separated from diagnostic-only evidence
- runtime-mode product conclusions and host-tooling failures were not kept apart
- the runner sometimes verified LoadLynx state through a second competing USB path instead of through the active poller/lease path
- the runner also allowed cached `devd /api/v1/devices` listing payloads to stand
  in for live UPS runtime truth during source-cut scenes
- formal start-up also assumed the bench had been left in a clean source-online
  state, which was false after aborted runs

## Resolution

### 1. Treat run validity as the only formal acceptance truth

A runtime-mode scene is formally valid only when:

- `summary.all.acceptance.run_validity == valid_for_signoff`

Anything else is:

- `invalid_diagnostic_only`

This rule is stronger than:

- smooth-looking charts
- apparently complete phase labels
- one or two convincing screenshots

### 2. Require full-scene synchronized evidence from all four surfaces

Formal runtime-mode conclusions must retain synchronized evidence from:

- IsolaPurr source telemetry
- UPS CLI/devd IPC `status`
- UPS CLI/devd IPC `diag-snapshot`
- LoadLynx USB telemetry

For current-board runtime-mode work, those four surfaces are the real truth surface. Any scene missing one of them is diagnostic-only.

### 3. Reuse the active LoadLynx lease/status path

One key false-failure pattern was:

- the live LoadLynx poller already owned the device lease
- the runner then opened a second fallback `loadlynx status` path
- both paths competed for the same USB owner
- the scene then produced fake `offline / stale / timeout` evidence

Reusable rule:

- if a scene already has a live LoadLynx poller/lease, scene-local verification must reuse that same path
- do not open a second competing status path against the same physical device during the scene

Additional operator hardening:

- use the explicitly selected fixed LoadLynx development CLI for formal Power Path Validation
- reject the run if that CLI does not expose `status-stream`; do not substitute
  the released CLI or an implicit fallback poll path
- treat `--load-ipc` as the selected transport for `status-stream`; do not skip
  the stream path just because the scene is using a Unix socket
- fallback polling is diagnostic-only and cannot be used as sign-off evidence

Reusable sampling rule:

- design for `3Hz` telemetry on every required live device path
- reject formal sign-off below `2Hz` or above `0.5s` maximum sample gap
- prove UPS CLI/devd IPC `status`, UPS CLI/devd IPC `diag-snapshot`, LoadLynx USB
  telemetry, and IsolaPurr source telemetry independently before a combined
  scene is trusted

UPS CDC freshness has one additional implementation lesson: the USB transport
was not the limiting factor when `status` / `diag-snapshot` appeared to stall.
The host path first had to avoid synchronous persistence on every monitor trace,
then the firmware had to stop doing large BMS diagnostic reads in one runtime
tick. Runtime BMS detail refreshes are now split so a single poll refreshes at
most one detail group, and `diag-snapshot` uses cached BMS detail fields instead of
doing extra DataFlash reads on the hot path. The host cache also must update
`diag_snapshot_updated_at` whenever monitor/status-derived `diag_snapshot` is written;
otherwise `diag-snapshot --watch` can incorrectly label a fresh derived diagnostic
sample as stale.

Current proof after that fix:

- UPS `status --watch --interval-ms 250 --watch-freshness-ms 750 --include-meta`:
  `96/96` rows, `0` misses, `4.0Hz`, max cache age `356ms`
- UPS `diag-snapshot --watch --interval-ms 250 --watch-freshness-ms 750 --include-meta`:
  `96/96` rows, `0` misses, `4.0Hz`, max cache age `421ms`
- LoadLynx `status-stream --interval-ms 250 --count 40`: about `3.99Hz`, max
  gap `274ms`

Current proof after syncing status-derived `diag_snapshot` timestamps:

- UPS `status --watch --interval-ms 250 --watch-freshness-ms 750 --samples 40`:
  `40/40` rows, `0` misses, `4.0Hz`, max output gap `272ms`, `0` stale rows
- UPS `diag-snapshot --watch --interval-ms 250 --watch-freshness-ms 750 --samples 40`:
  `40/40` rows, `0` misses, `4.0Hz`, max output gap `283ms`, `0` stale rows
- LoadLynx `status-stream --interval-ms 250 --count 40`: `40/40` rows,
  about `3.99Hz`, max gap `280ms`

UPS front-panel liveness added one more transport lesson:

- the accepted host-side `3Hz` target does not require firmware-side unsolicited
  Web Serial status push
- request-driven `service_web_serial_if_due()` is sufficient for USB host reads
  when devd monitor/cache is healthy
- an always-on compact status push path in the firmware main loop can starve or
  materially perturb front-panel rendering even when the USB transport itself is
  otherwise healthy

Current proof after removing unsolicited status push while keeping request
service enabled:

- USB `status --watch --interval-ms 333 --samples 12 --include-meta` with
  monitor running: `12/12` rows, `0` misses, `3.003Hz`, all
  `meta.sample_fresh=true`
- USB `status --fresh --watch --interval-ms 333 --samples 8 --include-meta`:
  `8/8` rows, `3.177Hz`, all `meta.sample_fresh=true`
- front-panel runtime remained `ready=true`; when the panel was in
  `display_power_mode=sleeping`, `frame_no` remained flat by design and must not
  be misclassified as a frozen render loop

Reusable rule:

- do not add firmware-side unsolicited telemetry push just to improve host poll
  cadence unless it is explicitly subscription-gated and proven not to interfere
  with front-panel liveness
- the default Mains Aegis truth path for continuous UPS status collection is:
  devd monitor/cache stream over IPC, with direct CDC fresh reads used only when
  explicitly requested

### 4. Treat chart HTML as presentation, not as the acceptance source

The HTML chart is useful, but it is not the primary acceptance source.

Acceptance truth comes from:

- `results.json`
- `summary.json`
- raw `timeseries.jsonl`

If the chart looks broken but raw/report continuity is still valid, fix the renderer instead of rewriting product conclusions.

### 4.5. Treat source-cut semantics as a first-class acceptance gate

For backup scenes, one more rule is now explicit:

- if IsolaPurr `port_c` is cut, the UPS-side runtime truth must react
- at least one of `mains_present=false`, `mode=backup`, or
  `assist_power_stage=backup` must be observed
- UPS `vin_vbus_mv` must also move with the cut

If the source is cut but UPS-side runtime truth remains logically frozen, that
run is diagnostic-only even when all three devices still appear to be sampling.

### 4.7. Let the runner self-heal the bench before formal preflight

One concrete failure class in this thread was not a product behavior failure at
all:

- a previous aborted scene left IsolaPurr `port_c` disabled
- the next formal rerun started with the UPS already in `backup`
- formal preflight then failed for the right reason, but the operator had to
  recover the bench manually first

Reusable rule:

- before formal preflight, the runner should first force IsolaPurr `port_c` off
- before any `12V` / `19V` switch or flash path, it should disable LoadLynx
  output first and only then cut IsolaPurr `port_c`
- then it should read UPS capability truth and confirm the actual hardware profile
  matches the intended scene
- `DCIN` must stay unpowered until UPS `output_profile` and `rated_vout_mv`
  have both been confirmed
- only after that may it program the source voltage/current target, read that
  configuration back, and finally re-enable `port_c`
- a formal rerun must not assume the previous scene exited cleanly

### 4.6. Verify the source cut itself before blaming UPS VIN

One concrete lesson from this thread:

- if you test UPS `vin_vbus_mv` correlation, first prove that the source-cut
  command actually disabled the source
- on the active bench, the reliable live check is:
  - cut IsolaPurr `port_c` with `enabled=0`
  - confirm IsolaPurr eventually reports `status=not_inserted`
  - confirm UPS `vin_vbus_mv` drops and `mode` reaches `backup`

Without that, a failed cut command can masquerade as a flat UPS VIN series.

### 5. Keep runtime VOUT micro-adjustment separate from TPS bring-up

For runtime-mode work, software mode/stage changes are allowed to micro-adjust the TPS target voltage, but they must not re-bring-up the active output.

Reusable implementation rule:

- runtime target-voltage changes on an already active output should directly apply the new `VOUT`
- they should not go through `disable -> init -> enable`

This matters because otherwise Power Path Validation can accidentally be measuring TPS reinitialization artifacts instead of runtime-mode behavior.

### 6. Dual-voltage suite execution needs an explicit power-off gate

Reusable suite rule:

- before switching between the `12V` and `19V` firmware artifacts, the source must be cut first
- the required order is:
  - disable LoadLynx output
  - prove the chosen IsolaPurr target is reachable before the first `port_c`
    write of the switch / flash path
  - cut IsolaPurr `port_c`
  - confirm the UPS is no longer fed by the source
  - only then switch/select/flash the next artifact
- after reboot, keep `port_c` off, verify UPS identity/settings, confirm USB
    + IPC capability truth matches the next scene, program the source
    voltage/current target, read it back, and only then restore source power

Hard safety consequence:

- until USB + IPC capability truth is confirmed, `DCIN` must stay de-energized
- source restoration is forbidden before that capability gate passes
- the same sequence applies even when the next step is only a profile change and
  not a flash command

This is not just operator caution. It is part of the suite contract and belongs in the runner and docs.

### 7. Formal evidence now scales from one scene to one four-scene suite

Reusable suite structure:

- `12V assist_path`
- `12V backup_only`
- `19V assist_path`
- `19V backup_only`

Scene-level fixed load targets:

- `assist_path -> 3900mA`
- `backup_only -> 1000mA`

Shared protection fence:

- `UVP=3000mV`
- `OCP=4000mA`
- `OPP=80000mW`

Reusable delivery rule:

- each scene keeps its own `results.json`, `summary.json`, `timeseries.jsonl`, and `voltage-chart.html`
- the suite adds one summary JSON, one suite verification JSON, and one single-page overview HTML that embeds the four charts

### 8. Current runner truth: dual-voltage execution must still pass live safety gates

Current repository reality:

- dual-voltage execution now has one accepted raw-scene suite and one accepted
  Rust-composed suite:
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/`
  - `tools/hil/reports/composed-current-four-scenes/`
- readiness may resolve both manifests before any source/load wiring is
  connected
- that resolution does not bypass the later per-scene power-off and capability
  gates

Reusable rule:

- treat manifest resolution as a readiness fact, not as a substitute for live
  bench safety checks
- if a future run cannot resolve one manifest again, report that as a new
  readiness failure for that run instead of keeping a stale global blocker in
  long-lived docs

### 9. Owner-facing Web handoff must use the real bench path

Reusable rule:

- owner-facing local Web handoff must use the clean app URL
- real hardware sessions must rely on same-origin `/api` proxying to the active
  worktree-owned `mains-aegis-devd` bridge
- do not use alternate query-parameter routing or any mock transport for real
  bench operation
- any URL/query combination containing `mock:` targets or `mock_devd_target`
  belongs to mock-only evidence and must not be described as a real bench path

### 10. Expected scene profile is not enough; the runner must verify real hardware capability before DCIN power

One hard lesson from the incident investigation:

- a planned `12V` scene is not proof that the UPS is actually running the `12V` hardware profile
- the runner must not restore external `DCIN` power only because the intended suite profile says `12V` or `19V`

Reusable rule:

- before the first `port_c` write of any scene, prove the chosen IsolaPurr
  target is reachable
  - the selected IsolaPurr CLI source telemetry path must respond
  - the CLI source payload must expose `port_c` or equivalent USB-C source state
  - the CLI status path must identify the expected bound IsolaPurr `device_id`
- keep IsolaPurr `port_c` off
- prove the UPS has actually detached from external `DCIN`
  - required UPS-side cut truth:
    - `input.vin_vbus_mv <= 2999`
    - `input.mains_present == false`
    - `mode=backup` or `input.assist_power_stage=backup`
  - do not use USB `5V` presence or `input.input_vbus_mv` as the DCIN cut veto
- USB-C host power / communication may remain attached during firmware profile
  switching; the safety gate only covers external `DCIN` high-voltage input
- read UPS `identity` over USB/devd IPC
- read UPS `settings` over USB/devd IPC
- verify `identity.hardware_capabilities.output_profile`
- verify `identity.hardware_capabilities.rated_vout_mv`
- verify `settings.advanced_power_capabilities.rated_vout_mv`
- treat those three fields as USB + IPC combined capability truth for the UPS
- treat CLI/devd IPC `status` as the separate runtime-truth surface for cut
  confirmation; it proves the live UPS actually reacted to the source cut
- do not let any one of `status`, USB `identity`, or IPC `settings` stand in
  for the others
- `DCIN` must not have source power until `output_profile` and
  `rated_vout_mv` are confirmed from that capability truth
- do not energize `DCIN` until the capability truth above is confirmed
- if source reachability fails, or if the live source identity mismatches the
  expected IsolaPurr device, the runner must abort before issuing `port_c`
  power writes
- this must surface as a normal gate-failure report, not as a readiness crash
  during the safe-prepare path
- only after those values match the intended scene may the runner program the source voltage and re-enable `port_c`
- this rule must run before every formal scene, not only before a `12V <-> 19V`
  profile switch

If any one of those checks fails, the runner must abort in the power-off state.

## Guardrails / Reuse notes

- Active formal source baselines are `12V / 3A` and `19V / 3A`.
- Every accepted runtime-mode rerun must quantify output voltage through all of:
  - source output voltage
  - UPS `DCIN`
  - UPS INA `VOUT`
  - load actual voltage
- If one rerun only proves that the capture chain is healthy, record that as tooling truth, not as a product behavior win.
- If one rerun only proves that the chart is wrong, fix the chart and preserve the report as the real source.

## Current accepted evidence snapshot

Current accepted formal sign-off suite after the truth-source, runner, and
capability-gate fixes:

- `tools/hil/reports/formal-12v-19v-four-scenes-cli-r4/suite-summary.json`
- `tools/hil/reports/formal-12v-19v-four-scenes-cli-r4/suite-overview.html`

Current owner-facing runner state:

- `mains-aegis power-validation` is the current Rust owner-facing entry
- `just power-validation ...` is the project shortcut
- `mains-aegis power-validation adapter-protocol` is the truth source for
  third-party source/load adapter authors
- external adapters receive explicit role/action parameters:
  - power source: `--role power-source`, `--voltage-mv`,
    `--current-limit-ma`, `--enabled`
  - electronic load: `--role electronic-load`, `--target-ma`, `--min-v-mv`,
    `--max-i-ma-total`, `--max-p-mw`
- adapter `stream` stdout is strict NDJSON; diagnostics belong on stderr
- Python suite runners under `tools/hil/` are migration references or focused
  diagnostics, not the owner-facing suite entry
- latest Rust four-scene suite `power-validation-rust-four-scenes-url-r7` is
  accepted:
  - `mains-aegis power-validation report --write-overview` passes
  - the verifier checks the suite summary, each scene `results.json`, raw
    `timeseries.jsonl` row count, required voltage-series presence, sampling
    thresholds, and chart existence
  - UPS transport: `cli+ipc+usb`
  - LoadLynx transport: `cli+ipc+usb`
  - IsolaPurr transport: `cli+url`
  - every scene satisfies `>=2Hz` and `max gap <=0.5s`
- IsolaPurr URL transport is acceptable for source control/telemetry; do not
  apply UPS/LoadLynx USB-only constraints to IsolaPurr. If the USB/devd path is
  flaky but the IsolaPurr CLI URL path is stable, use URL for the source
  adapter.
- current USB/IPC-backed Rust four-scene suite
  `formal-12v-19v-four-scenes-current-20260629T024800Z` is accepted:
  - `mains-aegis power-validation report --write-overview` passes
  - UPS transport: `cli+ipc+usb`
  - LoadLynx transport: `cli+ipc+usb`
  - IsolaPurr transport: `cli+ipc+usb`
  - every scene satisfies `>=2Hz` and `max gap <=0.5s`

Current passing `advanced_power` snapshot for all four scenes:

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

Current suite result under the updated formal contract:

- `12V assist_path`
  - `target_ma=3900`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `effective_sample_rate_hz=4.944`
  - `max_sample_gap_s=0.370`
- `12V backup_only`
  - `target_ma=1000`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `effective_sample_rate_hz=4.907`
  - `max_sample_gap_s=0.379`
- `19V assist_path`
  - `target_ma=3900`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `effective_sample_rate_hz=4.963`
  - `max_sample_gap_s=0.213`
- `19V backup_only`
  - `target_ma=1000`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `effective_sample_rate_hz=4.962`
  - `max_sample_gap_s=0.214`

Current incident fix that made the four-scene suite safe to run:

- CDC/native serial `device_identity` and `device_settings` must use fresh
  request/response reads, not stale capability cache entries, after a profile
  switch or flash
- successful devd-backed flash must invalidate identity/settings/status/diag-snapshot
  runtime caches
- firmware build metadata must rerun when `CARGO_FEATURE_*` changes so identity
  features match the actual compiled output profile
- after programming IsolaPurr manual voltage/current, the runner must explicitly
  force source output off again and verify `port_c` remains off before capability
  validation and source restore
- source-cut must use IsolaPurr `power output auto`; `power config set
  --usb-c-path disconnected` only changes the USB-C path and does not prove the
  banana jack / TPS high-voltage output is off
- active-scene source-cut gates must use the live collector truth instead of
  synchronous blocking status/power reads, otherwise `transition_backup` can
  produce artificial `timeseries` gaps even though the hardware telemetry
  streams are healthy
- runtime BQ40 block-detail diagnostics must stay lightweight in the firmware
  runtime path; heavy BQ40 block reads can starve USB `status` / `diag-snapshot`
  delivery and make a good bench look like a sampling failure

Current USB/IPC suite result:

- `12V assist_path`
  - `target_ma=3900`
  - `run_validity=valid_for_signoff`
  - `effective_sample_rate_hz=5.103`
  - `max_sample_gap_s=0.227`
- `12V backup_only`
  - `target_ma=1000`
  - `run_validity=valid_for_signoff`
  - `effective_sample_rate_hz=5.110`
  - `max_sample_gap_s=0.224`
- `19V assist_path`
  - `target_ma=3900`
  - `run_validity=valid_for_signoff`
  - `effective_sample_rate_hz=5.057`
  - `max_sample_gap_s=0.268`
- `19V backup_only`
  - `target_ma=1000`
  - `run_validity=valid_for_signoff`
  - `effective_sample_rate_hz=5.093`
  - `max_sample_gap_s=0.250`

Current focused live proof:

- `tools/hil/verify_ups_vin_source_cut_live.py --json`
- current observed result on the active bench:
  - pre-cut `vin_vbus_mv=12024`
  - cut intermediate `vin_vbus_mv=6096`
  - cut settled `vin_vbus_mv=2104`
  - `mains_present=false`
  - `mode=backup`
  - restore `vin_vbus_mv=12016`

## References

- `docs/hil-runtime-mode-switching.md`
- `docs/specs/xjpvj-runtime-mode-switching/SPEC.md`
- `docs/specs/xjpvj-runtime-mode-switching/IMPLEMENTATION.md`
- `tools/hil/advanced_power_12v_runner.py`
- `tools/hil/render_voltage_chart_html.py`
