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

# 2) Run diagnose flow (no ROM write)
./bin/run.sh diagnose --mode canonical --duration-sec 120

# 3) Run recover flow (ROM write allowed only in recover)
./bin/run.sh recover --mode canonical --duration-sec 120 --recover if-rom

# 4) Offline verify from an existing monitor log
./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file /abs/path/to/xxx.mon.ndjson
```

## Command contract

`./bin/run.sh <diagnose|recover|verify> [options]`

Options:
- `--mode canonical|dual-diag` (default: `canonical`)
- `--duration-sec <N>` (default: `120`)
- `--flash true|false` (default: `true`; not accepted by `verify`)
- `--recover never|if-rom|force` (default: `if-rom`; not accepted by `diagnose`/`verify`; `force` requires `--mode dual-diag`)
- `--monitor-file <path>` (`verify` required; others optional)
- `--report-out <dir>` (default: `tools/bq40-comm-tool/reports/<timestamp>`)

## Output

Each run produces:
- `summary.json`
- `summary.md`

Required `summary.json` fields:
- `mode`, `duration_sec`, `samples_total`, `valid_samples`, `max_valid_streak`
- `poll_errors` (by error type)
- `rom_events` (`detected`, `flash_attempted`, `flash_done`)
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
- `verdict.fail: canonical_mode_touched_0x16`
  - canonical mode should not touch `0x16`; check firmware mode and logs

## More docs

- `docs/README.md`
- `docs/operations.md`
- `docs/log-signatures.md`
- `docs/recovery-safety.md`
- `docs/troubleshooting-notes.md`
- `docs/references.md`
