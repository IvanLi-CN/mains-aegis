# bq40-comm-tool

Standalone BQ40Z50 communication toolchain under `tools/bq40-comm-tool`.

This tool is isolated from the main firmware path and provides:
- firmware build/flash/monitor orchestration
- diagnose/recover/verify workflows
- structured report output (`summary.json` + `summary.md`)

## 3-minute quick start

```bash
cd /Users/ivan/Projects/Ivan/mains-aegis/tools/bq40-comm-tool

# 1) For devd-backed live maintenance, export the explicit devd target
export BQ40_TOOL_DEVD_URL=http://127.0.0.1:30080
export BQ40_TOOL_DEVICE_ID=<devd-device-id>

# 2) Run canonical diagnose with the proven wake profile (no ROM write)
./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true

# 3) Only if step 2 reports `rom_mode_detected`, run dual-diag recover
./bin/run.sh recover --mode dual-diag --recover if-rom --force-min-charge true --rom-image r2

# 4) Re-run canonical diagnose after recovery
./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true

# 5) Offline verify from the canonical monitor log
./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file /abs/path/to/xxx.mon.ndjson
```

## Command contract

`./bin/run.sh <diagnose|recover|verify> [options]`

Options:
- `--mode canonical|dual-diag` (`diagnose`/`verify` default to `canonical`; `recover` defaults to `dual-diag` unless explicitly overridden)
- `--duration-sec <N>` (default: `120`; explicit lower bounds are computed from the selected subcommand and wake/recover knobs, so `--force-min-charge` / `--recover` / `--rom-image` can all raise the minimum; when omitted, ROM-enabled `recover` uses the larger of the computed minimum and the historical safe `155s` default)
- `--flash true|false` (default: `true`; not accepted by `verify`)
- `--recover never|if-rom|force` (default: `if-rom`; not accepted by `diagnose`/`verify`; `force` requires `--mode dual-diag`)
- `--force-min-charge true|false` (default: `false`; not accepted by `verify`; also lengthens the minimum live-monitor window because the tool adds a repower/min-charge settle budget before liveness probing)
- `--probe-mode strict|mac-only` (default: `strict`; not accepted by `verify`; `mac-only` is diagnostic-only and narrows steady-state liveness checks to ManufacturerAccess()/ManufacturerBlockAccess() after the normal wake/ROM handling)
- `--rom-image r2|r3|r5` (default: `r2`; not accepted by `verify`; affects the computed `recover` minimum)
- `--monitor-file <path>` (`verify` required; others optional)
- `--report-out <dir>` (default: `tools/bq40-comm-tool/reports/<timestamp>`)

## Output

Each run produces:
- `summary.json`
- `summary.md`
- process exit code (`0` when `verdict.pass=true`; non-zero when `verdict.pass=false`)

Required `summary.json` fields:
- `mode`, `duration_sec`, `samples_total`, `valid_samples`, `max_valid_streak`
- `run_config` (`force_min_charge`, `probe_mode`, `rom_image`)
- `poll_errors` (by error type)
- `rom_events` (`detected`, `flash_attempted`, `flash_image_done`, `flash_done`, `fw_seen`, `runtime_invalid`, `runtime_status_unconfirmed`)
  - `flash_image_done=true` means the ROM flash sequence reached `stage=rom_flash_done` (image write completed), but post-flash resume may still fail.
  - `flash_done=true` means the recover flow emitted `stage=probe_rom_flash_done` after the gauge was validated back in firmware mode (including delayed post-flash resume).
  - `fw_seen=true` means post-flash probing observed firmware-mode evidence such as `probe_rom_post_flash_fw_seen*` or `rom_post_flash_resume_not_rom`; a bare ROM-exit acknowledgement is not enough.
  - `runtime_invalid=true` / `runtime_status_unconfirmed=true` make the post-flash failure mode explicit when the gauge leaves ROM but still cannot produce a trustworthy runtime snapshot.
- `verdict.pass`, `verdict.reason`

## Common issues

- `BQ40_TOOL_DEVICE_ID is required for devd flash/monitor`
  - export `BQ40_TOOL_DEVICE_ID` before running live commands, or use the low-voltage recovery maintenance runner which injects it for you
- `BQ40_TOOL_ARTIFACT_MANIFEST_PATH is required for devd flash`
  - run `./bin/run.sh ...` end-to-end so `bin/build.sh` can generate the manifest, or export a valid manifest path when calling `flash.sh` directly
- `mains-aegis-devd` health check fails
  - ensure the bridge is up at `BQ40_TOOL_DEVD_URL` and that the selected device is already bound in devd
- `monitor output did not advance`
  - confirm the explicit device is connected through devd and that `./bin/run.sh` is using the same `BQ40_TOOL_DEVICE_ID`
- `monitor file not found: ...`
  - for `verify`, make sure `--monitor-file` points to an existing `.mon.ndjson`
- `duration-sec` floors (computed by `./bin/run.sh`; `diagnose` / `recover --recover never` require `>=30s` without wake and `>=42s` with `--force-min-charge true`; ROM-enabled `recover` also adds post-flash quiet + resume + transfer/gap budget and still defaults to the historical safe `155s` bench duration when `--duration-sec` is omitted)
  - the parser now scores every successful `bms:` poll, so the steady-state cadence is the 2s main loop rather than the older 5s working-info print; for example, `recover --recover if-rom --force-min-charge true --rom-image r2` currently computes a hard minimum of `118s` before the extra safe default is applied
- `verdict.fail: canonical_mode_touched_0x16`
  - canonical mode should not touch `0x16`; check firmware mode and logs
- canonical diagnose still has `samples_total=0`
  - re-run with `--force-min-charge true`; the supported no-pack wake profile is `VREG=16.8V / ICHG=200mA / IINDPM=500mA`
- dual-diag still has `samples_total=0` and no ROM signature
  - run `./bin/run.sh diagnose --mode dual-diag --duration-sec 120 --force-min-charge true --probe-mode mac-only`; note that this only changes the discovery probe path (before an address is latched), and the normal wake/ROM checks still run before the MAC probe is used
- recover report shows `flash_attempted=true` but `flash_done=false`
  - the ROM sequence ran but did not exit ROM; stop and inspect the monitor log instead of assuming reflashing succeeded

## More docs

- `docs/README.md`
- `docs/operations.md`
- `docs/log-signatures.md`
- `docs/recovery-safety.md`
- `docs/troubleshooting-notes.md`
- `docs/references.md`
