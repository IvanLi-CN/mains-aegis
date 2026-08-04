# Power Path Validation Runbook

This directory contains the current operator tooling for Mains Aegis power path
validation. The directory name is historical; the owner-facing product surface
is **Power Path Validation** / **电源路径验证**, exposed by
`mains-aegis power-validation`.

Use this file as the execution guide.

Use `docs/hil-runtime-mode-switching.md` as the acceptance truth source.

Use `docs/solutions/firmware/runtime-mode-hil-with-isolapurr-loadlynx.md` for
historical findings and debugging context.

## Scope

Hardware bindings are never built into the repository. Provide the current bench
targets explicitly with CLI flags or environment variables such as
`MAINS_AEGIS_UPS_DEVICE_ID`, `MAINS_AEGIS_POWER_DEVICE_ID`,
`MAINS_AEGIS_LOAD_DEVICE_ID`, and `MAINS_AEGIS_LOAD_USB_PORT`.

Active source profiles:

- `12V / 3A`
- `19V / 3A`

## Validation Levels

Use precise names when recording validation evidence:

- **HIL unit tests** are Python/Rust tests for the HIL tooling itself, such as
  `python3 -m unittest tools.hil.test_formal_hil_readiness ...`. They use mocks,
  fixtures, temporary files, and synthetic reports. They do not operate hardware
  and do not prove a real bench run.
- **HIL dry-runs** exercise command construction, report wiring, and safety
  gates without changing hardware state. They are useful for runner validation,
  but they are not real HIL evidence.
- **Real formal power-path HIL** is a power-path validation run against the
  explicit UPS, source, and load targets. It is separate from HIL unit tests.
- **Read-only diag-snapshot HIL** is `diag_snapshot_readonly.py`. It only reads
  `GET /api/v1/devices/{id}/diag-snapshot` and validates package shape. It must
  not bind, flash, reset, monitor, write settings, or apply BQ40 Data Flash.

Do not shorten "HIL unit tests passed" to "HIL passed". A sign-off statement
must say which level ran and, for real HIL, which runner produced the report.

## Owner-Facing Entry

The owner-facing validation entry is now the Rust host command:

```bash
just power-validation run --dry-run --load-cli "$LOADLYNX_CLI" --load-ipc "$LOADLYNX_DEVD_SOCKET"
```

The product surface is `mains-aegis power-validation ...`, not a Python script.
Python files in this directory are retained as migration references, report
utilities, or focused internal diagnostics. Existing raw scene directories may
still be composed and verified by Rust `power-validation compose`, but a legacy
Python runner is not the owner-facing command for new validation runs.

Power Path Validation keeps the UPS path fixed to `mains-aegis` CLI + native devd
IPC + UPS USB CDC. Power sources and electronic loads are adapters:

- built-in power adapter: `isolapurr`
- built-in load adapter: `loadlynx`
- third-party devices: external adapter command protocol printed by
  `mains-aegis power-validation adapter-protocol`

External adapter actions receive explicit parameters; they must not infer scene
settings from ambient state:

```bash
source-adapter --role power-source --action configure \
  --voltage-mv 12000 --current-limit-ma 3000 --enabled false
source-adapter --role power-source --action enable \
  --voltage-mv 12000 --current-limit-ma 3000
source-adapter --role power-source --action stream --interval-ms 200

load-adapter --role electronic-load --action set-load \
  --target-ma 3900 --min-v-mv 3000 --max-i-ma-total 4000 --max-p-mw 80000
load-adapter --role electronic-load --action stream --interval-ms 200
```

`stream` stdout is strict NDJSON. Non-data logs go to stderr. Unsupported or
unsafe settings return one JSON object with `ok=false` and an explicit
`error_code`; adapters must not silently clamp voltage, current, or load
requests.

## Script Inventory

- `advanced_power_12v_runner.py`
  - historical URL-oriented diagnostic runner
  - not accepted as the current formal CLI-only sign-off path
- `formal_hil_cli_suite.py`
  - migration reference for the Rust `power-validation` runner
  - not the long-term owner-facing entry
  - uses only UPS CLI/devd IPC, LoadLynx CLI/devd IPC, and IsolaPurr CLI
- `advanced-power-12v-sweep.py`
  - multi-round advanced-power sweep wrapper around the formal scene flow
  - useful for candidate comparison and reboot/readback loops
- `capture_load_transition_timeseries.py`
  - focused single-scene capture runner for transition and boundary studies
  - useful when you do not need the full staged advanced-power wrapper
- `capture_source_vin_vout_keypoints.py`
  - keypoint collector for source output, UPS VIN, UPS INA VOUT, and output-voltage snapshots
- `probe_loadlynx_released_telemetry.py`
  - preflight probe for whether the active LoadLynx host path can satisfy the
    formal sampling gate
- `loadlynx_ipc_status_helper.py`
  - helper for same-IPC lease-backed LoadLynx status reads
- `render_voltage_chart_html.py`
  - renders the interactive HTML chart from one `timeseries.jsonl`
- `render_output_voltage_chart.py`
  - renders the static PNG output-voltage chart from one `timeseries.jsonl`
- `verify_formal_suite.py`
  - verifies a suite summary against its referenced report directories
- `diag_snapshot_readonly.py`
  - read-only HIL gate for `diag-snapshot`
  - validates package response shape for explicitly requested packages
  - performs no flash, reset, monitor, settings write, BQ40 DF write, source
    control, or load control
- `formal_hil_suite.py`
  - legacy HTTP-oriented suite orchestrator
  - not accepted as the current formal CLI-only sign-off path
- `formal_hil_readiness.py`
  - legacy HTTP-oriented pre-connection readiness checker
  - not accepted as the current formal CLI-only sign-off path
- `render_formal_suite_html.py`
  - renders the single-page four-card suite overview
- `verify_runtime_vout_live.py`
  - focused live check for the runtime VOUT-update path
  - writes one standby-drop step, waits for owner-facing status to follow, then restores
  - proves runtime target changes are applied in-place on the live UPS
- `verify_ups_vin_source_cut_live.py`
  - focused live check for UPS `vin_vbus_mv` source-cut correlation
  - cuts IsolaPurr `port_c`, samples UPS CLI/devd IPC `status`, then restores
    the source
  - proves whether UPS `vin_vbus_mv`, `mains_present`, and `backup` semantics react to a real source cut

## Formal Acceptance Floor

One run is valid only when `results.json` says:

- `summary.all.acceptance.run_validity == valid_for_signoff`

And therefore all of the following are true:

- `effective_sample_rate_hz >= 2.0`
- `max_sample_gap_s <= 0.5`
- source output voltage exists for the whole scene
- UPS `DCIN` voltage exists for the whole scene
- UPS INA `VOUT` exists for the whole scene
- load actual voltage exists for the whole scene
- for backup scenes, UPS cut-state semantics are observable
  - formal UPS truth comes from direct UPS `status`, not from a cached devd
    devices listing projection
  - once `port_c_enabled=false`, the UPS evidence must show at least one of:
    - `mains_present=false`
    - `mode=backup`
    - `assist_power_stage=backup`
  - UPS `DCIN` voltage must also move with the cut
- `failed_acceptance_checks` is empty

If any one line above fails, the run is diagnostic-only.

Freshness diagnostics are still recorded in `results.json`:

- `load_status_max_age_s`
- `source_status_max_age_s`
- `ups_status_max_age_s`
- `diag_snapshot_max_age_s`

They remain mandatory observability fields for debugging, but they are no longer
independent veto conditions when the scene already preserves continuous complete
sampling and the required source-cut semantics.

## End-to-End Flow

### 1. Prepare live control paths

Power Path Validation uses fixed stable control paths per device:

- UPS: `mains-aegis --ipc <socket> device <id> ...`
- LoadLynx: the fixed development `loadlynx` CLI with `--ipc <socket>`
- IsolaPurr: `isolapurr` CLI over the currently stable source path
  - preferred on this bench: `--url http://127.0.0.1:30182`
  - devd IPC / USB is allowed only when it is proven responsive

Do not hard-code stale localhost ports from older runs.

For local Web verification and owner-facing browser handoff only:

- use the clean Web app URL, for example `http://127.0.0.1:<web-port>/`
- rely on same-origin `/api` proxying to the current worktree-owned
  `mains-aegis-devd` HTTP bridge
- do not use alternate query-parameter routing or any mock transport for a real
  hardware session

Minimum formal Power Path Validation surfaces:

- UPS `mains-aegis device <id> status`
- UPS `mains-aegis device <id> diag-snapshot`
- UPS `mains-aegis device <id> settings`
- IsolaPurr source telemetry
- LoadLynx USB CLI control

Formal continuous sampling has two fixed thresholds:

- engineering target: every live stream should sustain about `3Hz`
- hard acceptance floor: every required live stream must sustain `>=2Hz` and
  `max_sample_gap_s <= 0.5`

Before a combined scene can be treated as formal evidence, prove these three
paths independently on the same bench:

- UPS `status --watch --interval-ms 200 --watch-freshness-ms 750`
- UPS `diag-snapshot --watch --interval-ms 200 --watch-freshness-ms 750`
- LoadLynx `status-stream` through the USB/devd path that the scene will use,
  using the explicitly selected development CLI binary
- IsolaPurr CLI source telemetry at the same `3Hz` cadence, using whichever
  IsolaPurr CLI transport is stable for the bench

If any path cannot meet the floor, the next validation run is diagnostic-only. Do not
hide a slow hardware path behind interpolation, chart rendering, or a different
out-of-band command path.

The readiness checker records the same gate in its summary JSON under
`telemetry_gate.probes`. A formal run is allowed only when all four required
probes are fresh and pass the rate/gap floor:

- `ups_status`
- `ups_diag_snapshot`
- `source`
- `load`

A single passing device path is not enough to start combined Power Path Validation. For example,
a LoadLynx path that reports `effective_sample_rate_hz >= 3` and
`max_sample_gap_s <= 0.5` is still only a LoadLynx pass when UPS `status`,
UPS `diag-snapshot`, or IsolaPurr source telemetry are stale, unreachable, or
below the floor.

The UPS status and diag-snapshot surfaces must be available through the
`mains-aegis` CLI over devd IPC/USB. Do not replace missing CLI capability with
ad-hoc IPC JSON-RPC calls in formal validation scripts; fix the CLI contract first.
Do not use the local HTTP bridge or UPS LAN HTTP for formal UPS telemetry
evidence.

```bash
mains-aegis device fixture-ups-device status --watch --interval-ms 200
mains-aegis device fixture-ups-device diag-snapshot --watch --interval-ms 200
```

`--watch` reads the devd monitor cache by default and does not issue extra CDC
requests that compete with monitor. Use `--fresh` only for targeted latency or
request/response diagnostics, not for formal sampling evidence.

The UPS USB path is considered healthy only when the emitted watch rows are both
continuous and fresh:

- `status --watch --interval-ms 250 --watch-freshness-ms 750 --include-meta`
  should show `0` misses, output gaps below `0.5s`, and `cache_age_ms <= 750`
- `diag-snapshot --watch --interval-ms 250 --watch-freshness-ms 750 --include-meta`
  must meet the same rule
- stale cache rows may be emitted to preserve a diagnostic timeline, but stale
  rows do not satisfy the formal freshness gate

The current fixed UPS proof after the BMS detail refresh split and
status-derived `diag_snapshot` timestamp sync is:

- `status`: `40/40` rows, `0` misses, `4.0Hz`, max output gap `272ms`,
  `0` stale rows
- `diag-snapshot`: `40/40` rows, `0` misses, `4.0Hz`, max output gap `283ms`,
  `0` stale rows
- LoadLynx `status-stream`: `40/40` rows, about `3.99Hz`, max gap `280ms`

Do not paper over a slow UPS path by switching to HTTP, raw IPC JSON-RPC, or
interpolated chart data. Fix the CLI/devd/firmware path first.

### 2. Prove LoadLynx telemetry can meet the formal gate

Run the probe before a new formal scene whenever the host path changed:

```bash
python3 tools/hil/probe_loadlynx_released_telemetry.py \
  --load-device fixture-load-device \
  --load-usb-device-id fixture-load-usb-device \
  --load-cli "$LOADLYNX_CLI" \
  --load-devd-base-url "$LOADLYNX_DEVD_BASE_URL" \
  --load-devd-socket "$LOADLYNX_DEVD_SOCKET"
```

The probe should show a formal-capable path before you rely on the next run for
acceptance.

The accepted LoadLynx CLI contract for formal scenes is:

```bash
"$LOADLYNX_CLI" --ipc "$LOADLYNX_DEVD_SOCKET" status-stream \
  --device fixture-load-device \
  --interval-ms 200
```

`LOADLYNX_CLI` must point at the fixed development CLI under the agent-owned
LoadLynx worktree. Power Path Validation must not silently fall back to
`~/.local/bin/loadlynx` or any released binary on `PATH`.

`--load-ipc` is a transport selector, not a reason to skip `status-stream`.
When a socket endpoint is supplied, the runner must still prefer
`status-stream` over that IPC transport and must run the live-poller capability
probe before scene start. A fallback poll path is diagnostic-only and cannot
produce formal acceptance evidence.

### 3. Historical single-scene runner

`advanced_power_12v_runner.py` is historical diagnostic tooling. It still
contains URL-oriented code paths and must not be used as formal sign-off
evidence for the current CLI-only contract.

The current formal path is the suite runner below.

### 4. Run the formal dual-voltage suite

The suite-level contract is fixed to four scenes:

- `12V assist_path`
- `12V backup_only`
- `19V assist_path`
- `19V backup_only`

The formal runner is the Rust `power-validation` command:

```bash
LOADLYNX_CLI="$LOADLYNX_CLI" \
ISOLAPURR_IPC=.tmp/isolapurr-devd-power-validation.sock \
just power-validation run \
  --profile 12v --profile 19v \
  --scene assist-path --scene backup-only \
  --artifact-manifest-12v web/public/firmware/<12v>.manifest.json \
  --artifact-manifest-19v web/public/firmware/<19v>.manifest.json \
  --allow-profile-flash \
  --ups-device "$MAINS_AEGIS_UPS_DEVICE_ID" \
  --load-cli "$LOADLYNX_CLI" \
  --load-ipc .tmp/loadlynx-devd-power-validation.sock \
  --load-device "$MAINS_AEGIS_LOAD_DEVICE_ID" \
  --isolapurr-cli isolapurr \
  --isolapurr-url http://127.0.0.1:30182 \
  --power-device "$MAINS_AEGIS_POWER_DEVICE_ID" \
  --report-root tools/hil/reports
```

Important operator rules:

- `--allow-profile-flash` is required for a live `12V <-> 19V` profile switch;
  without it the runner fails closed if the UPS capability truth does not match
  the requested scene profile
- `--artifact-manifest-12v` and `--artifact-manifest-19v` are required for live
  profile switching; dry-run plans may use placeholder manifest paths
- the runner must not enter `pre` before `load_status_ready` is recorded
- `status-stream` is the preferred continuous LoadLynx telemetry path
- if the selected LoadLynx CLI does not expose `status-stream`, preflight must
  reject the run; do not substitute a released CLI or fallback polling path
- the full scene must remain one continuous capture
- every formal scene is self-gated
  - the runner must re-run the full pre-scene safety sequence even when the
    previous scene used the same voltage profile
  - do not assume that one earlier profile-level check is still valid for the
    next scene
- before the first `port_c` write of any scene, the runner must prove that the
  chosen IsolaPurr target is itself reachable
- the selected IsolaPurr CLI path must use an explicitly selected, responsive
  transport; HTTP/URL is acceptable for IsolaPurr when it is the stable source
  control path
- the IsolaPurr CLI source telemetry path must respond
  - the CLI source payload must expose `port_c` or equivalent USB-C source state
  - the selected `isolapurr` CLI path must also respond
  - the CLI status payload must identify the expected IsolaPurr `device_id`
  - if reachability fails, or if the observed CLI identity mismatches the bound
    IsolaPurr target, the run must abort before touching `port_c`
- after that first successful reachability proof, the runner may force
  IsolaPurr `port_c` off and must then prove the UPS has
  actually detached from external `DCIN` before any flash or source restore step
  is allowed
  - this proof must come from UPS-side truth, not only from IsolaPurr ack
  - required UPS-side cut truth:
    - `input.vin_vbus_mv <= 2999`
    - `input.mains_present == false`
    - `mode=backup` or `input.assist_power_stage=backup`
  - USB `5V` presence may still be real and must not veto this cut proof
- USB-C host power / communication may remain attached during `12V <-> 19V`
  firmware switching because it does not participate in the UPS direct-output
  path; only external `DCIN` high-voltage input is part of this gate
- the runner must keep `port_c` off until the UPS hardware capability has been
  read and validated for the intended profile
- USB + IPC capability truth means:
  - UPS USB identity reports `identity.hardware_capabilities.output_profile`
  - UPS USB identity reports `identity.hardware_capabilities.rated_vout_mv`
  - UPS CLI/devd IPC settings report `settings.advanced_power_capabilities.rated_vout_mv`
- `DCIN` must not have source power until UPS `output_profile` and
  `rated_vout_mv` have been confirmed from that USB + IPC capability truth
- until that USB + IPC capability confirmation completes, `DCIN` must stay
  de-energized and `port_c` must remain off
- after capability validation, the runner must program the IsolaPurr manual
  voltage/current target, read that configuration back, confirm `port_c` is
  still off, and only then re-enable `port_c`
- formal scene safety order is fixed:
  - first disable LoadLynx output
  - then cut IsolaPurr `port_c`
  - only after UPS-side cut truth is proven may flash, profile switch, or
    source-target reprogramming continue
- do not restore source power to `DCIN` merely because the intended scene says
  `12V` or `19V`; live UPS capability truth must already match that profile
- the formal UPS runtime truth surface is UPS CLI/devd IPC `status`
- `devd /api/v1/devices` listing payloads are cache/projection material and must
  not be treated as the primary UPS `status` source for formal scene capture

Capability confirmation must come from three observation surfaces with fixed
roles:

- CLI/devd IPC `status`
  - confirms runtime cut truth and live power-path semantics
  - use it to prove `mains_present`, `mode`, `assist_power_stage`, and live
    `vin_vbus_mv` actually react to the source cut
- USB/IPC `settings`
  - confirms owner-facing configured capability on the live UPS CLI/devd IPC
    surface
  - use it to read `settings.advanced_power_capabilities.rated_vout_mv`
- USB `identity`
  - confirms the actual attached hardware profile on the UPS control path
  - use it to read `identity.hardware_capabilities.output_profile` and
    `identity.hardware_capabilities.rated_vout_mv`

Formal capability truth is valid only when those surfaces agree where they
overlap:

- `output_profile` comes from USB `identity`
- `rated_vout_mv` must agree between USB `identity` and USB/IPC `settings`
- source restore to `DCIN` is forbidden until that agreement is proven

The suite-level contract is:

- `12V assist_path`
- `12V backup_only`
- `19V assist_path`
- `19V backup_only`

Scene definitions are fixed:

- `assist_path`
  - source online
  - CC load `3900mA`
  - `standby -> assist_low -> backup -> assist_low -> standby`
- `backup_only`
  - source online
  - CC load `1000mA`
  - `standby -> backup -> standby`

Load protection is fixed across all four scenes:

- `UVP=3000mV`
- `OCP=4000mA`
- `OPP=80000mW`

Hard safety gate before any `12V <-> 19V` artifact switch or flash:

1. disable LoadLynx output
2. prove the chosen IsolaPurr target is reachable before the first `port_c`
   write of this switch / flash path
3. cut IsolaPurr `port_c` without changing source voltage first
4. confirm the source is no longer feeding the UPS
   - required UPS-side cut truth:
     - `input.vin_vbus_mv <= 2999`
     - `input.mains_present == false`
     - `mode=backup` or `input.assist_power_stage=backup`
   - do not use `input.input_vbus_mv` or USB `5V` presence as the DCIN cut proof
5. only then select/flash the next artifact
6. after reboot, read UPS identity and settings while `port_c` remains off
7. confirm USB + IPC capability truth matches the intended scene profile
   - USB identity must expose `output_profile` and `rated_vout_mv`
   - CLI/devd IPC settings must confirm `rated_vout_mv`
   - keep `DCIN` unpowered until those `output_profile` / `rated_vout_mv`
     checks pass
8. program the new source voltage/current target and read it back while `port_c` remains off
9. only then re-enable `port_c`
10. prove the source actually reaches UPS `DCIN`
    - IsolaPurr output voltage/current is only the source-side precondition
    - required UPS-side online truth:
      - `input.vin_vbus_mv` is within the active profile window
      - `input.mains_present == true`
      - `mode != backup` and `input.assist_power_stage != backup`
    - if IsolaPurr reports a valid output but UPS `vin_vbus_mv` remains near
      the cut value, the scene must fail before enabling LoadLynx
11. start the next scene

`DCIN` must stay de-energized from the source-cut step through the capability
confirmation and source readback steps above.

The same hard gate also applies to any non-flash profile transition that would
change the effective `12V` / `19V` output profile for the next formal scene.

Per-scene precondition for all four suite scenes:

- before each individual scene runner starts `pre`, it must repeat:
  - disable LoadLynx output
  - prove the chosen IsolaPurr target is reachable before the first `port_c`
    write of that scene
  - force IsolaPurr `port_c` off
  - prove UPS-side external-input cut truth
  - read UPS `identity`
  - read UPS `settings`
  - verify USB/IPC capability truth matches the intended scene profile
    through `output_profile` and `rated_vout_mv`
  - do not energize `DCIN` until that capability confirmation succeeds
  - program IsolaPurr source voltage/current
  - read back IsolaPurr source voltage/current while `port_c` is still off
  - only then enable `port_c`
  - prove UPS-side online truth before enabling LoadLynx
- this per-scene gate applies even when two adjacent scenes use the same
  `12V` or `19V` profile
- the suite implementation must enforce this scene gate itself; it is not an
  operator memory step and must not rely only on the earlier profile-level
  prepare sequence

`formal_hil_suite.py` and `formal_hil_readiness.py` are legacy HTTP-oriented
orchestration helpers. They are not the formal entry point for current
CLI-only USB/devd IPC validation.

### 5. Inspect the report directory

Each formal scene report directory should contain at least:

- `results.json`
- `summary.json`
- `timeseries.jsonl`
- `progress.json`
- `settings_snapshot.json`

Use `results.json` as the primary scene validity source. For a suite, use the
Rust verifier and let it trace every scene back to its raw evidence:

```bash
mains-aegis power-validation report --write-overview tools/hil/reports/<suite-id>
```

The suite is valid only when this command reports:

- `signoff_valid=true`
- empty `suite_failures`
- empty `report_failures`
- all four scene `results.json` files are `valid_for_signoff`
- every scene `timeseries.jsonl` row count matches `results.json.samples`
- every scene `timeseries.jsonl` independently recomputes the required voltage
  series, effective sample rate, and max gap, and those values still match the
  scene `results.json` summary
- every scene chart exists

If the raw scene directories already exist and need to be combined into a suite
without re-running hardware, use Rust `compose`:

```bash
mains-aegis power-validation compose \
  --suite-id <suite-id> \
  --output-dir tools/hil/reports/<suite-id> \
  tools/hil/reports/<raw-scene-a> \
  tools/hil/reports/<raw-scene-b> \
  tools/hil/reports/<raw-scene-c> \
  tools/hil/reports/<raw-scene-d>
```

`compose` reads each scene `results.json`, preserves a relative link to that
scene directory, writes a new `suite-summary.json`, regenerates
`suite-overview.html`, and then runs the same Rust sign-off verifier. It does
not copy, edit, or synthesize raw samples.

### 6. Render the voltage chart

Interactive HTML:

```bash
python3 tools/hil/render_voltage_chart_html.py \
  --input tools/hil/reports/<report>/timeseries.jsonl \
  --output tools/hil/reports/<report>/voltage-chart.html \
  --title "12V Formal Scene"
```

Static PNG:

```bash
python3 tools/hil/render_output_voltage_chart.py \
  --input tools/hil/reports/<report>/timeseries.jsonl \
  --output /tmp/hil-output-chart.png \
  --title "12V Formal Scene"
```

Chart rendering rules:

- charts only connect adjacent samples when the gap is `<= 0.5s`
- charts are evidence presentation, not the acceptance source
- if a chart looks broken, verify `timeseries.jsonl` and `results.json` before
  concluding the scene actually dropped samples

### 7. Verify a suite when comparing multiple reports

```bash
mains-aegis power-validation report --write-overview tools/hil/reports/<suite-id>
```

The verifier is intentionally stricter than opening `suite-overview.html`.
Opening the overview only proves that a page exists; the verifier proves that
the overview can be regenerated from a real suite summary whose scene reports,
raw JSONL samples, voltage series, and charts are all present and internally
consistent.

Use this when you need one machine-readable verdict across several report
directories.

### 8. Open the suite overview

After a complete suite run, the suite overview HTML is the single owner-facing
entry point for all four charts:

- `<suite-id>-overview.html`

It should show:

- suite metadata
- four scenario cards
- one interactive scene chart per card
- scene-level source voltage, load target, validity, and advanced-power snapshot

Current accepted suite:

- suite id: `formal-12v-19v-four-scenes-current-20260629T024800Z`
- summary: `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-summary.json`
- overview: `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-overview.html`
- tracked evidence summary:
  `docs/specs/runtime-mode-switching/evidence/formal-12v-19v-four-scenes-current-20260629T024800Z-suite-summary.json`
- tracked evidence overview:
  `docs/specs/runtime-mode-switching/evidence/formal-12v-19v-four-scenes-current-20260629T024800Z-suite-overview.html`
- transport: UPS `cli+ipc+usb`, LoadLynx `cli+ipc+usb`, IsolaPurr `cli+ipc+usb`
- `12V assist_path`: `3900mA`, `valid_for_signoff`, `5.103Hz`,
  max gap `0.227s`
- `12V backup_only`: `1000mA`, `valid_for_signoff`, `5.110Hz`,
  max gap `0.224s`
- `19V assist_path`: `3900mA`, `valid_for_signoff`, `5.057Hz`,
  max gap `0.268s`
- `19V backup_only`: `1000mA`, `valid_for_signoff`, `5.093Hz`,
  max gap `0.250s`

Rust-composed view of the same raw scene evidence:

- suite id: `composed-current-four-scenes`
- summary: `tools/hil/reports/composed-current-four-scenes/suite-summary.json`
- overview: `tools/hil/reports/composed-current-four-scenes/suite-overview.html`
- generation command: `mains-aegis power-validation compose`
- provenance: raw scene directories are linked from
  `formal-12v-19v-four-scenes-current-20260629T024800Z`; no raw samples are
  copied or synthesized

Historical accepted suite retained for comparison:

- suite id: `formal-12v-19v-four-scenes-cli-r4`
- summary: `tools/hil/reports/formal-12v-19v-four-scenes-cli-r4/suite-summary.json`
- overview: `tools/hil/reports/formal-12v-19v-four-scenes-cli-r4/suite-overview.html`
- tracked evidence summary:
  `docs/specs/runtime-mode-switching/evidence/formal-12v-19v-four-scenes-cli-r4-suite-summary.json`
- tracked evidence overview:
  `docs/specs/runtime-mode-switching/evidence/formal-12v-19v-four-scenes-cli-r4-suite-overview.html`

The current accepted suite also proves the safety fix that `DCIN` stays
unpowered until the live UPS `output_profile` and `rated_vout_mv` are read from
fresh USB/IPC capability truth and the IsolaPurr source configuration has been
read back with `port_c` still off.

## Runtime VOUT live check

Use this when the question is specifically:

- does runtime target-voltage micro-adjustment apply directly on the live UPS
- without requiring a full output re-bring-up sequence

Command:

```bash
python3 tools/hil/verify_runtime_vout_live.py
```

Current default behavior:

- reads current `advanced_power` and `status`
- adds `+100mV` to `standby_drop_mv`
- waits until `/api/v1/status` reflects the new `assist_target_vout_mv`
- verifies `out_a/out_b` move in the same direction
- restores the original `standby_drop_mv`
- the runtime path is expected to apply the new TPS VOUT directly in place, without
  a full TPS disable/init/enable sequence

This is a focused live device check, not a substitute for the formal
three-device `12V` scene contract.

## UPS VIN source-cut live check

Use this when the question is specifically:

- does UPS `vin_vbus_mv` actually follow a real source cut
- or is one observed flat VIN series more likely to be a capture / truth-source problem

Command:

```bash
python3 tools/hil/verify_ups_vin_source_cut_live.py --json
```

Current expected behavior:

- pre-cut:
  - `port_c_enabled=true`
  - UPS `vin_vbus_mv` is near the source voltage
- after cut settles:
  - `port_c_enabled=false`
  - UPS `vin_vbus_mv` drops materially
  - UPS eventually reports `mains_present=false` and `mode=backup`
- after restore:
  - `port_c_enabled=true`
  - UPS `vin_vbus_mv` returns near the pre-cut level

This is a focused live proof for VIN correlation. It is not a replacement for the
full formal runtime-mode scene.

## Current Formal Truth

The historical report:

- `tools/hil/reports/20260623T051149Z-formal-12v-3900-complete-r2-liveups`

is still useful as a diagnostic reference for the `3900mA` cut/restore scene,
but it is not sufficient as the current final sign-off artifact anymore.

Current reason:

- the active acceptance contract now requires all four voltage series to be
  present in `summary.all.acceptance.required_voltage_series`:
  - source output voltage
  - UPS `DCIN` voltage
  - UPS INA `VOUT`
  - load actual voltage
- that historical `results.json` does not currently encode
  `required_voltage_series.ups_output_voltage`, so it cannot be treated as the
  sole final proof against the current contract
- the current released LoadLynx path must therefore pass a fresh preflight and
  produce a new formal run before the thread goal can be considered complete
- after the source-cut truth fix, any report that keeps UPS `mains_present=true`
  together with a flat UPS `vin_vbus_mv` after `port_c_enabled=false` is
  diagnostic-only even if the chart itself looks continuous

## Git Tracking Rules

- scripts in this directory are source artifacts and must be Git-tracked
- `tools/hil/reports/` is runtime output and stays ignored
- `tools/hil/__pycache__/` and generated `.pyc` files are runtime garbage and
  stay ignored
