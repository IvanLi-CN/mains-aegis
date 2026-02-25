# Operations Runbook

## 1) Diagnose (default safe path)

No BQ ROM write is allowed in diagnose mode.

```bash
cd /Users/ivan/Projects/Ivan/mains-aegis/tools/bq40-comm-tool
./bin/run.sh diagnose --mode canonical --duration-sec 120
```

Expected:
- build + flash + monitor + report complete
- `summary.json` produced
- `verdict.pass=true` when `max_valid_streak >= 10`
- `--recover` is rejected in this mode (to avoid silent overrides)
- `tps_sync` unavailable only emits warning; output self-test still proceeds

## 2) Recover (state-changing path)

Recover mode allows ROM recovery policy control.

```bash
./bin/run.sh recover --mode canonical --duration-sec 120 --recover if-rom
```

Policy:
- `--recover never`: disable ROM recovery
- `--recover if-rom`: recover only when ROM signature is detected
- `--recover force`: force ROM recovery path even when signature is not detected (requires `--mode dual-diag`)

## 3) Verify (offline)

```bash
./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file /abs/path/to/file.mon.ndjson
```

Notes:
- `verify` is offline-only and does not accept `--flash` or `--recover`.

## 4) Scenario checklist

- A: BQ40 disconnected -> stable `i2c_nack`, `verdict.pass=false`
- B: BQ40 connected + diagnose -> report with categorized poll errors
- C: ROM signature `0x9002` + recover -> `rom_events` reflects recovery stages
- D: charger/IRQ disturbance -> still hit `max_valid_streak >= 10`
- E: canonical mode -> no `addr=0x16` in monitor log
- F: `verify` over same log -> reproducible summary

## 5) Troubleshooting

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
