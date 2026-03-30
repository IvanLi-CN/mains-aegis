# Operations Runbook

## 1) Canonical diagnose (default safe path)

No BQ ROM write is allowed in diagnose mode. This is the required first step in the supported workflow and should use the proven wake profile on a no-pack bench.

```bash
cd /Users/ivan/Projects/Ivan/mains-aegis/tools/bq40-comm-tool
./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true
```

Expected:
- build + flash + monitor + report complete
- `summary.json` produced
- `verdict.pass=true` when `max_valid_streak >= 10`
- command exits non-zero when `verdict.pass=false` (CI/automation friendly)
- `--recover` is rejected in this mode (to avoid silent overrides)
- `--force-min-charge true` applies the supported wake profile: `VREG=16.8V / ICHG=200mA / IINDPM=500mA`
- `tps_sync` unavailable only emits warning; output self-test still proceeds
- `--duration-sec` must satisfy the tool-derived minimum for diagnose: `>=30s` without wake, or `>=42s` with `--force-min-charge true` (10s repower-off + 2s settle + 12s startup-to-first-sample + 9 more samples at the 2s successful-poll cadence)

## 1.1) Deep diagnostic fallback (no ROM write)

Use this only when canonical diagnose still returns zero samples and you need a stronger signal about SMBus liveness without escalating to ROM write.

```bash
./bin/run.sh diagnose --mode dual-diag --duration-sec 120 --force-min-charge true --probe-mode mac-only
```

What to inspect:
- `probe_mode=mac_only` should appear in the log
- `addr=0x0B` returning `write=i2c_nack_data` means something still ACKs the canonical address but rejects every command byte
- `addr=0x16` staying `write=i2c_nack_addr` means the fallback/RAM/ROM address never came alive
- if both happen together and no `stage=rom_mode_detected` appears, stop and report a blocked state instead of escalating to `force`

## 2) Recover (state-changing path)

Only run recover after canonical diagnose fails and the monitor log proves `stage=rom_mode_detected`. The supported repo workflow is `dual-diag + if-rom + force-min-charge + asset-df-mainboard`; do not escalate to `force` when ROM signature is absent.

```bash
./bin/run.sh recover --mode dual-diag --recover if-rom --force-min-charge true --rom-image r2 --repair-profile asset-df-mainboard
```

Policy:
- `--duration-sec` must be `>=30s` for `recover --recover never`, or `>=42s` when that path is combined with `--force-min-charge true` (same floor logic as diagnose)
- `--duration-sec` must be `>=` the tool-derived minimum for `recover --recover if-rom|force` (omit `--duration-sec` to auto-select; for example `--force-min-charge true --rom-image r2` currently computes `118s`, while the script still keeps the safer historical default of `155s` when the option is omitted)
- `--recover never`: disable ROM recovery (no state-changing ROM write)
- `--recover if-rom`: recover only when ROM signature is detected
- `--recover force`: debug-only escape hatch; not part of the supported repo recovery sequence
- `--rom-image r2|r3|r5`: select the ROM recovery image explicitly when the bench target is not the default R2 pack
- `--repair-profile asset-df-mainboard`: use the official TI `section1.bin` as the DF base, then apply this board's fixed DF overrides before ROM flash

Mainboard policy:
- `asset-df-mainboard` is the supported board-repair path for this repository
- it does not depend on live MB44 DF capture, so a chip that falls back to TI stock DF or rejects live capture can still be repaired onto the board's 4S baseline
- when the pack still answers MB44 in app mode, the tool preserves live `CELL_GAIN` / `PACK_GAIN` / `BAT_GAIN` on top of the official asset base only if all three words are captured; otherwise it falls back to the asset defaults instead of flashing a mixed live/default calibration set
- it is intentionally different from "writing TI default DF fields"; the tool writes an official DF section base plus project-specific overrides

## 3) Verify (offline)

```bash
./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file /abs/path/to/file.mon.ndjson
```

Notes:
- `verify` is offline-only and does not accept `--flash` or `--recover`.

## 4) Supported sequence

1. `diagnose --mode canonical --force-min-charge true`
2. If the log contains `stage=rom_mode_detected`, run `recover --mode dual-diag --recover if-rom --force-min-charge true --repair-profile asset-df-mainboard`
3. Re-flash canonical firmware and re-run `diagnose --mode canonical --force-min-charge true`
4. Run `verify --mode canonical --monitor-file <canonical log>` on the final canonical log


## 5) Scenario checklist

- A: BQ40 disconnected -> stable `i2c_nack`, `verdict.pass=false`
- B: BQ40 connected + diagnose -> report with categorized poll errors
- C: ROM signature `0x9002` + recover -> `rom_events` reflects recovery stages
- D: charger/IRQ disturbance -> still hit `max_valid_streak >= 10`
- E: canonical mode -> no `addr=0x16` in monitor log
- F: `verify` over same log -> reproducible summary

## 6) Troubleshooting

- Symptom: `mcu-agentd` command hangs (no JSON output) or reports `managerd ipc failed`.
- Check:

```bash
mcu-managerd status
```

- Recovery (session-local):

```bash
mcu-managerd run
```

Keep it running in one terminal, then execute tool commands in another terminal.
