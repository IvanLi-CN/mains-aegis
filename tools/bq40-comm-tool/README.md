# bq40-comm-tool

Standalone BQ40Z50 communication toolchain under `tools/bq40-comm-tool`.

This tool is isolated from the main firmware path and provides:
- firmware build/flash/monitor orchestration
- diagnose/recover/verify workflows
- structured report output (`summary.json` + `summary.md`)

## 3-minute quick start

```bash
cd /Users/ivan/Projects/Ivan/mains-aegis/tools/bq40-comm-tool

# 1) Optional: bind board port selector for this tool only
cp .esp32-port.example .esp32-port
# edit .esp32-port

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
- `--duration-sec <N>` (default: `120`; explicit lower bounds are computed from the selected subcommand and wake/recover knobs, so `--force-min-charge` / `--recover` / `--rom-image` can all raise the minimum; when omitted, ROM-enabled `recover` automatically picks the computed minimum)
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
- `rom_events` (`detected`, `flash_attempted`, `flash_image_done`, `flash_done`)
  - `flash_image_done=true` means the ROM flash sequence reached `stage=rom_flash_done` (image write completed), but post-flash resume may still fail.
  - `flash_done=true` means the recover flow emitted `stage=probe_rom_flash_done` after the gauge was validated back in firmware mode (including delayed post-flash resume).
- `verdict.pass`, `verdict.reason`

## Common issues

- `mcu-agentd ... config file not found`
  - run commands from `tools/bq40-comm-tool` (this directory has its own `mcu-agentd.toml`)
- `E_SELECTOR_MISSING`
  - set `tools/bq40-comm-tool/.esp32-port` first (example file is provided)
- `monitor output did not advance`
  - ensure board port is correctly selected in `.esp32-port`
- `mcu-agentd ... managerd ipc failed` or command hangs without output
  - check manager status: `mcu-managerd status`
  - if not running, start foreground once for this session: `mcu-managerd run`
  - then re-run `./bin/run.sh ...` (tool report parser works offline on existing logs too)
- `monitor file not found: ...`
  - for `verify`, make sure `--monitor-file` points to an existing `.mon.ndjson`
- `duration-sec` floors (computed by `./bin/run.sh`; wake adds 12s when `--force-min-charge true`; ROM-enabled `recover` also adds post-flash quiet + resume + transfer/gap budget and defaults to the computed minimum, which is `>=155` and depends on `--rom-image`)
  - the no-pack wake path spends 10s with charge off and 2s at minimum charge, but the firmware only checks the 5s working-info target on a 2s main loop, so the parser-visible steady-state cadence is effectively ~6s; ROM-enabled recover also reserves another 10s post-flash boot quiet plus the ROM flash transfer/gap budget
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
