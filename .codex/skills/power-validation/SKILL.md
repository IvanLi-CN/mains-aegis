---
name: "power-validation"
description: "Use when operating, extending, or debugging Mains Aegis Power Path Validation: the owner-facing power-validation CLI, its adapter protocol for replaceable power sources/electronic loads, safety gates, report generation, and DIY parameter-testing workflow."
---

# Power Path Validation

Use this skill for Mains Aegis Power Path Validation work: running validation scenes, checking adapters, interpreting reports, or developing a new adapter for a non-IsolaPurr power source or non-LoadLynx electronic load.

The owner-facing name is **Power Path Validation** / **电源路径验证**. The CLI remains `mains-aegis power-validation ...` for brevity and compatibility. The historical `tools/hil` path is only a storage/location name.

## Hard Rules

- Use `mains-aegis power-validation ...` or `just power-validation ...`; do not use legacy Python validation runners as owner-facing commands.
- UPS communication is fixed: `mains-aegis` CLI -> native devd IPC -> UPS USB CDC.
- LoadLynx communication is fixed for the approved bench: `loadlynx` CLI -> native devd IPC -> LoadLynx USB.
- IsolaPurr communication is selected for stability. It may use CLI + devd IPC + USB or CLI + `--url` LAN HTTP; do not generalize UPS/LoadLynx transport restrictions to IsolaPurr.
- The power source and electronic load are replaceable adapters. Built-ins are `isolapurr` and `loadlynx`; third-party devices must use the external adapter protocol.
- Do not energize `DCIN` until UPS capability truth confirms the intended `output_profile` and `rated_vout_mv`.
- Do not bypass missing CLI capability with raw HTTP, raw IPC JSON-RPC, Web UI writes, or ad-hoc scripts. Fix the relevant CLI/adapter first.
- Formal evidence requires complete source / UPS VIN / UPS VOUT / load voltage telemetry at `>=2Hz` and max gap `<=0.5s`; target engineering rate is about `3Hz`.

## Standard Commands

```bash
just power-validation check \
  --isolapurr-url "$ISOLAPURR_URL" \
  --load-cli "$LOADLYNX_CLI" \
  --load-ipc "$LOADLYNX_DEVD_SOCKET"
```

```bash
just power-validation run --dry-run \
  --isolapurr-url "$ISOLAPURR_URL" \
  --load-cli "$LOADLYNX_CLI" \
  --load-ipc "$LOADLYNX_DEVD_SOCKET"
```

```bash
just power-validation run \
  --profile 12v --profile 19v \
  --scene assist-path --scene backup-only \
  --artifact-manifest-12v web/public/firmware/<12v>.manifest.json \
  --artifact-manifest-19v web/public/firmware/<19v>.manifest.json \
  --allow-profile-flash \
  --isolapurr-url "$ISOLAPURR_URL" \
  --load-cli "$LOADLYNX_CLI" \
  --load-ipc "$LOADLYNX_DEVD_SOCKET"
```

```bash
just power-validation compose \
  --suite-id <suite-id> \
  --output-dir tools/hil/reports/<suite-id> \
  tools/hil/reports/<raw-scene-a> \
  tools/hil/reports/<raw-scene-b>
```

```bash
just power-validation report --write-overview tools/hil/reports/<suite-id>
```

Use `compose` when the raw scene result directories already exist and need to be
recombined into a suite. Each scene directory must contain `results.json`,
`timeseries.jsonl`, and `voltage-chart.html`; the generated overview links back
to those raw scene files instead of copying or synthesizing evidence.

## Adapter Model

Core runner responsibilities:

- Own scene orchestration, safety gates, UPS capability checks, timestamps, acceptance checks, and reports.
- Treat power source and electronic load only through adapter capabilities and telemetry.

Adapter responsibilities:

- Power-source adapter supports capabilities, configure disabled, enable, disable, and stream telemetry.
- Electronic-load adapter supports capabilities, disable, CC load with UVP/OCP/OPP protection, and stream telemetry.
- Stream output is NDJSON with normalized fields: `timestamp_ms`, `voltage_mv`, `current_ma`, `power_mw`, `enabled`, `protection_state`.
- The runner invokes external adapters as:
  - `<adapter> --role power-source --action configure --voltage-mv <mv> --current-limit-ma <ma> --enabled false`
  - `<adapter> --role power-source --action enable --voltage-mv <mv> --current-limit-ma <ma>`
  - `<adapter> --role power-source --action stream --interval-ms <ms>`
  - `<adapter> --role electronic-load --action set-load --target-ma <ma> --min-v-mv <mv> --max-i-ma-total <ma> --max-p-mw <mw>`
  - `<adapter> --role electronic-load --action stream --interval-ms <ms> [--count <n>]`
- Unsupported actions must return explicit `ok=false` / `error_code=unsupported`; adapters must not silently clamp values.
- Stream stdout must be strict NDJSON; progress logs and diagnostics belong on stderr.

Use `mains-aegis power-validation adapter-protocol` for the current JSON protocol skeleton.

## Built-In Adapters

- `--power-adapter isolapurr` uses `isolapurr` CLI. Pass either `--isolapurr-url` / `ISOLAPURR_URL` for LAN HTTP or `--isolapurr-ipc` / `ISOLAPURR_IPC` for devd IPC.
- `--load-adapter loadlynx` uses `loadlynx status-stream` over the selected LoadLynx devd IPC.
- `LOADLYNX_CLI` or `--load-cli` is required; do not silently fall back to a random released binary on `PATH`.

## References

- Operational runbook: `tools/hil/README.md`
- Acceptance truth source: `docs/hil-runtime-mode-switching.md`
- Historical debugging notes: `docs/solutions/firmware/runtime-mode-hil-with-isolapurr-loadlynx.md`
