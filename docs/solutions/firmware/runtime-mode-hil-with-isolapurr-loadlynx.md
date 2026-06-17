---
title: Runtime-mode HIL with IsolaPurr and LoadLynx
module: firmware
problem_type: hardware-validation
component: UPS runtime mode switching
tags:
  - ups
  - hil
  - runtime-mode
  - isolapurr
  - loadlynx
status: active
related_specs:
  - docs/specs/xjpvj-runtime-mode-switching/SPEC.md
  - docs/specs/eu2b8-bq25792-charge-policy/SPEC.md
  - docs/specs/p8k3d-mains-aegis-devd/SPEC.md
---

# Runtime-mode HIL with IsolaPurr and LoadLynx

## Context

The `#xjpvj` runtime-mode contract exposes `VIN` truth, `TPS total output current`, runtime charger tokens, and battery current. A useful HIL rig must stimulate the UPS input and output sides independently while preserving owner-facing evidence from the UPS itself.

For this project the accepted bench rig is:

- IsolaPurr `856a141cdbd4` as the controllable DC input source
- `2 mm banana -> DC5025` into UPS `DCIN`
- LoadLynx `loadlynx-d68638` on UPS `OUT`
- UPS LAN `status` plus devd `power-diag` on approved binding `serial-04f3bb3f5367`

## Why the current HIL still keeps `13.0V / 3.0A`

The current firmware now stages TPS target voltage internally:

- `standby` uses a near-zero-assist hot-standby target
- `assist_low` uses a higher low-supplement target
- `assist_rated` and `backup` use rated output target

Even with that software contract in place, the bench still benefits from a source setting that stays comfortably above the standby / assist-low targets.

A lower bench source such as `12.0V` or `12.5V` can still blur the intended non-`BACKUP` condition:

- `VIN` is too close to the fixed TPS target
- the UPS can appear to enter `assist_rated` too early
- the rig stops proving whether direct input can independently cover the load

Using IsolaPurr in manual `13.0V / 3.0A` mode with `usb-c-path disconnected` gives the current firmware a cleaner test condition:

- `VIN` stays clearly above the standby / assist-low TPS targets in `STANDBY`
- the source power budget is about `39W`
- a `12V` output load near `3A` should still remain direct-input dominated, with `ASSIST` expected only slightly above that after conversion loss and board overhead are counted

The accepted run matched that expectation:

- `1A`, `2A`, and `3A` all stayed in `STANDBY`
- the first clean `ASSIST` transition appeared at `3200mA`

This is still a bench convenience, not a product contract.

## What to look at now

For owner-facing staged-assist validation, check these first:

- `status.input.assist_power_stage`
- `status.input.assist_target_vout_mv`
- `power-diag.input.assist_power_stage`
- `power-diag.input.assist_target_vout_mv`
- `power-diag.input.vin_baseline_mv`
- `power-diag.input.vin_drop_mv`
- `power-diag.input.tps_total_iout_ma`

## Acceptance must come from the UPS, not from the bench tools alone

Use the UPS itself as the source of truth:

1. UPS LAN `status`
2. devd `GET /api/v1/devices/serial-04f3bb3f5367/power-diag`
3. USB `trace(kind=event,target=power)` only when it emits fresh stage-local events

IsolaPurr and LoadLynx prove the stimulus. They do not prove runtime mode by themselves.

## Accepted `13.0V` run on 2026-06-17

The accepted rerun captured `STANDBY -> 1A -> 2A -> 3A -> ASSIST(3200mA) -> BACKUP(3200mA) -> STANDBY`.

### `STANDBY`

- IsolaPurr: manual `13.0V / 3.0A`, `port_c=13026mV / 4mA`
- LoadLynx: disabled, `v_local_mv=13050`, `calc_p_mw=91`
- UPS input: `input_vbus_mv=5088`, `input_ibus_ma=209`, `vin_vbus_mv=13024`, `vin_iin_ma=28`, `tps_total_iout_ma=40`
- UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/20mA`
- Battery: `pack_mv=16234`, `current_ma=0`, `soc_pct=93`
- Charger: `detail_status=WAIT`, `allow_charge=false`

This stage proves the intended bench condition: `VIN` is above TPS output voltage before `BACKUP`.

### `1A`

- LoadLynx: `output_enabled=true`, `target_i_ma=1000`, `i_local_ma=1002`, `calc_p_mw=12774`
- IsolaPurr: `port_c=12971mV / 1003mA`
- UPS input: `vin_vbus_mv=12720`, `vin_iin_ma=1051`, `tps_total_iout_ma=40`
- UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/20mA`
- Battery: `current_ma=0`
- Charger: `detail_status=WAIT`
- UPS mode: `standby`

This confirms that `1A` is still fully covered by direct input under the raised-source bench.

### `2A`

- LoadLynx: `output_enabled=true`, `target_i_ma=2000`, `i_local_ma=1000`, `i_remote_ma=988`, `calc_p_mw=24512`
- IsolaPurr: `port_c=12970mV / 2002mA`
- UPS input: `vin_vbus_mv=12472`, `vin_iin_ma=2068`, `tps_total_iout_ma=36`
- UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/20mA`
- Battery: `current_ma=0`
- Charger: `detail_status=WAIT`
- UPS mode: `standby`

This stage disproves the earlier lower-voltage HIL conclusion that `2A` already meant `ASSIST`.

### `3A`

- LoadLynx: `output_enabled=true`, `target_i_ma=3000`, `i_local_ma=1499`, `i_remote_ma=1487`, `calc_p_mw=35473`
- IsolaPurr: `port_c=12841mV / 2978mA`
- UPS input: `vin_vbus_mv=12088`, `vin_iin_ma=3062`, `tps_total_iout_ma=68`
- UPS output: `out_a=12064mV/16mA`, `out_b=12064mV/52mA`
- Battery: `current_ma=-22`
- Charger: `detail_status=WAIT`
- UPS mode: `standby`

This is the important boundary stage. The source is already near its `3A` ceiling, but the UPS still does not meet the `ASSIST` condition because `tps_total_iout_ma` remains below the `100mA` threshold.

### `3200mA` `ASSIST`

- LoadLynx: `output_enabled=true`, `target_i_ma=3200`, `i_local_ma=1600`, `i_remote_ma=1587`, `calc_p_mw=37826`
- IsolaPurr: `port_c=12836mV / 2978mA`
- UPS input: `vin_vbus_mv=12096`, `vin_iin_ma=3062`, `tps_total_iout_ma=272`
- UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/256mA`
- Battery: `pack_mv=16208`, `current_ma=-173`
- Charger: `detail_status=LOAD`, `allow_charge=false`
- UPS mode: `supplement`

This is the first clean assist stage:

- IsolaPurr current has already plateaued near `3A`
- the extra output demand is being covered by battery/TPS
- `tps_total_iout_ma` crosses the `100mA` enter threshold
- charger token changes to `LOAD`

With the staged-assist firmware contract, this overload point should now be interpreted in two phases:

- first `assist_low` while direct input is still given priority
- then `assist_rated` once `VIN` sag and TPS output current both stay elevated for the configured hold window

### `BACKUP @ 3200mA`

- Trigger: raw device HTTP `POST /api/v1/ports/port_c/power?enabled=0`
- LoadLynx: still enabled, `target_i_ma=3200`, `i_local_ma=1600`, `i_remote_ma=1587`, `calc_p_mw=37523`
- UPS input: `input.source=usbc`, `mains_present=false`, `vin_vbus_mv=2096`, `vin_iin_ma=5`, `tps_total_iout_ma=3300`
- UPS output: `out_a=12000mV/1660mA`, `out_b=12000mV/1640mA`
- Battery: `pack_mv=15882`, `current_ma=-2462`
- Charger: `detail_status=NOAC`, `allow_charge=false`
- IsolaPurr: `port_c_enabled=false`, no valid `port_c` or `usb_c` power telemetry
- UPS mode: `backup`

### restored `STANDBY`

- Trigger: raw device HTTP `POST /api/v1/ports/port_c/power?enabled=1`, then disable LoadLynx
- IsolaPurr: `port_c=13024mV / 3mA`, `port_c_enabled=true`
- LoadLynx: disabled, `v_local_mv=13038`, `calc_p_mw=91`
- UPS input: `vin_vbus_mv=13024`, `vin_iin_ma=28`, `tps_total_iout_ma=36`
- Battery: `current_ma=0`
- Charger: `detail_status=WAIT`
- UPS mode: `standby`

## What this run proved

- The raised-source `13.0V / 3.0A` bench setting is materially better than the earlier lower-voltage setups for current firmware.
- `1A`, `2A`, and `3A` all stayed in `STANDBY`.
- The first clean `ASSIST` transition appeared only once source current had effectively saturated near `3A` and `tps_total_iout_ma` crossed the `100mA` threshold.
- `ASSIST` correctly coupled to charger token `LOAD`.
- `BACKUP` correctly coupled to charger token `NOAC`.

## What this run did not prove

- It did not prove zero-sag seamless takeover; this solution is intentionally “current board first” and only promises direct-input priority plus software-staged online takeover.
- It did not prove the separate charger-pressure/cooldown branch inside `eu2b8`, because all accepted stages still showed `pressure_reason=none`.
- It did not produce fresh stage-local power trace events for the new `13.0V` sequence. The USB trace surface remained stale, so this run is `trace-degraded`.

## Operational pitfalls and workarounds

### IsolaPurr released `ports power --url` path rejected this DUT

Against `http://192.168.31.122`, the released `isolapurr ports --url ... power --enabled false|true` path returned `HTTP 400` during HIL.

The accepted workaround was to use raw device HTTP with numeric query values:

```bash
curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=0'
curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=1'
```

Use that path until the IsolaPurr host/device control-plane mismatch is fixed.

### LoadLynx control-plane failures should be treated separately from UPS mode validation

During bench work, LoadLynx showed two non-UPS issues:

- one transient `HTTP 503 LINK_DOWN` during enable, which recovered on retry
- one earlier control/status divergence investigation at `1A`, recorded in `/tmp/loadlynx-1a-divergence-20260617-173239`

Neither changes the accepted UPS-side runtime-mode evidence for the final `13.0V` run. Keep UPS acceptance tied to UPS telemetry, not to LoadLynx control-plane quirks.

### Prefer explicit `--url` control for both IsolaPurr and LoadLynx

Do not rely on saved transports during HIL:

- IsolaPurr saved device records may prefer a Local USB path that is no longer present
- LoadLynx may keep an old `.local` HTTP route after reboot

Use explicit URLs during bench automation:

- IsolaPurr: `http://192.168.31.122`
- LoadLynx: `http://192.168.31.216`

## Raw evidence

- `/tmp/mains-aegis-hil-20260617-175412-13000-final`
- `/tmp/mains-aegis-hil-20260617-180711-13000-backup-tail-v3`

## References

- `docs/hil-runtime-mode-switching.md`
- `docs/specs/xjpvj-runtime-mode-switching/SPEC.md`
- `docs/specs/eu2b8-bq25792-charge-policy/SPEC.md`
