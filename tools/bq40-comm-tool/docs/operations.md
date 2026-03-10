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
- `--duration-sec` must be `>=70` for diagnose (10s repower-off + 2s settle + two 2s loop quanta before the first steady-state sample + the remaining 9 samples landing on an effective ~6s cadence)

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

Only run recover after canonical diagnose fails and the monitor log proves `stage=rom_mode_detected`. The supported repo workflow is `dual-diag + if-rom + force-min-charge`; do not escalate to `force` when ROM signature is absent.

```bash
./bin/run.sh recover --mode dual-diag --duration-sec 155 --recover if-rom --force-min-charge true
```

Policy:
- `--duration-sec` must be `>=155` for recover (diagnose floor + 10s post-flash boot quiet + 30s post-flash resume window + current ROM flash transfer/gap budget before the 10-sample steady-state verdict)
- `--recover never`: disable ROM recovery
- `--recover if-rom`: recover only when ROM signature is detected
- `--recover force`: debug-only escape hatch; not part of the supported repo recovery sequence
- `--rom-image r2|r3|r5`: select the ROM recovery image explicitly when the bench target is not the default R2 pack

## 3) Verify (offline)

```bash
./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file /abs/path/to/file.mon.ndjson
```

Notes:
- `verify` is offline-only and does not accept `--flash` or `--recover`.

## 4) Supported sequence

1. `diagnose --mode canonical --force-min-charge true`
2. If the log contains `stage=rom_mode_detected`, run `recover --mode dual-diag --recover if-rom --force-min-charge true`
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
