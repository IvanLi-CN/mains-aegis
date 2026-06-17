# Runtime-Mode Switching HIL

This page defines the owner-facing hardware-in-the-loop path for verifying the `#xjpvj` UPS runtime-mode contract with an IsolaPurr bench source and a LoadLynx electronic load.

## Safety Scope

- UPS observation target:
  - LAN device id: `mains-aegis-198840`
  - Approved serial binding: `serial-04f3bb3f5367`
- Bench source:
  - IsolaPurr device id: `856a141cdbd4`
  - Bench wiring: `2 mm banana -> DC5025 -> UPS DCIN`
- Electronic load:
  - LoadLynx device id: `loadlynx-d68638`
- Agents must not directly invoke `espflash`, `cargo espflash`, or `cargo-espflash`.
- Agents must not use `mcu-agentd` for Mains Aegis hardware operations, enumerate `/dev/*`, or try alternate serial ports.
- This flow allows read/session-read operations plus owner-authorized bench-source and electronic-load state changes that are required to exercise `STANDBY / ASSIST / BACKUP`.

## Goals

- Verify the runtime-mode transition loop `STANDBY -> ASSIST -> BACKUP -> STANDBY`.
- Verify `ASSIST` and `BACKUP` both force non-charging behavior.
- Verify that a raised DC input source can keep the UPS in `STANDBY` until direct-input headroom is actually exhausted.
- Capture owner-facing evidence from UPS `status` and `power-diag`; use `trace(kind=event,target=power)` only as a secondary channel when it emits fresh stage-local events.

## Required Tooling

- UPS host tools from the current repository:

```bash
cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis -- devices scan
cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- serve-http --allow-dev-cors
```

- Released IsolaPurr tools:

```bash
isolapurr --help
isolapurr-devd --help
```

- Released LoadLynx tools:

```bash
loadlynx --help
loadlynx-devd --help
```

## Bench Topology

- IsolaPurr `856a141cdbd4` provides the UPS `DCIN` bench source through the `2 mm banana` output and the `DC5025` cable.
- LoadLynx `loadlynx-d68638` is connected to UPS `OUT` and provides the stimulus through `CC` mode.
- For the current firmware, IsolaPurr should stay in manual `13.0V / 3.0A` mode with `usb-c-path disconnected`.
- `13.0V` is a HIL compensation, not a product-semantic TPS target:
  - the current firmware still drives TPS55288 with one fixed `12.0V` target across `STANDBY / ASSIST / BACKUP`
  - the project has not yet implemented the future “lower TPS standby voltage, rated backup voltage” split
  - using `13.0V` keeps `VIN` clearly above the fixed TPS setpoint and avoids a misleading “DCIN and TPS are tied at the same nominal voltage” bench condition during non-`BACKUP` validation
- With manual `13.0V / 3.0A`, the bench source can provide about `39W`, so a clean HIL run should keep the UPS in `STANDBY` through roughly `3A` output load and only enter `ASSIST` slightly above that after conversion loss and board overhead are counted.
- `USB-C port power` on IsolaPurr `port_c` is the approved automatic cutoff path for the shared bench output rail.
- On the observed bench state, raw device HTTP:

```bash
curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=0'
curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=1'
```

  cuts and restores the banana/DC5025 rail.
- The released `isolapurr ports --url ... power --enabled false|true` path returned `HTTP 400` against this DUT, so the accepted run used the raw device HTTP `enabled=0|1` workaround.

## Observed Run Notes

- The accepted `13.0V` run on `2026-06-17` captured seven stage-local snapshots:
  - `STANDBY baseline`
  - `1A target`
  - `2A target`
  - `3A target`
  - `3200mA target`
  - `BACKUP @ 3200mA`
  - restored `STANDBY`
- Owner-facing evidence in this run comes from:
  - UPS LAN `status`
  - `mains-aegis-devd serve-http` `GET /api/v1/devices/serial-04f3bb3f5367/power-diag`
- The UPS bare LAN surface still does not expose a standalone `/api/v1/power-diag` endpoint.
- In this run, the bound USB `trace(kind=event,target=power)` surface did not produce fresh stage-local power events for the new `13.0V` sequence. Treat trace as degraded evidence for this run and do not use it as the primary acceptance signal.

## Preparation Sequence

1. Verify tooling:

   ```bash
   isolapurr --help
   isolapurr-devd --help
   loadlynx --help
   loadlynx-devd --help
   cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis -- devices scan
   ```

2. Verify device identity:

   ```bash
   isolapurr discover --json
   loadlynx devices --json
   cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis -- devices scan
   cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis -- device serial-04f3bb3f5367 connect
   ```

3. Start the UPS observation service:

   ```bash
   cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- serve-http --allow-dev-cors
   ```

   Do not keep a separate default `serve` process alive on the same IPC endpoint while using `serve-http`.

4. Prepare IsolaPurr:

   ```bash
   isolapurr power show --url http://192.168.31.122 --json
   isolapurr power output manual \
     --url http://192.168.31.122 \
     --voltage-mv 13000 \
     --current-limit-ma 3000 \
     --usb-c-path disconnected
   curl -fsS http://192.168.31.122/api/v1/ports
   ```

5. Prepare LoadLynx:

   ```bash
   loadlynx control set --url http://192.168.31.216 --disable
   loadlynx status --url http://192.168.31.216 --json
   loadlynx control get --url http://192.168.31.216 --json
   ```

6. Confirm the bench wiring is already in place:
   - IsolaPurr is already configured for `13.0V / 3.0A`.
   - `2 mm banana -> DC5025 -> UPS DCIN` is already connected.
   - Continue only after the current bench state is verified from telemetry.

## Evidence Commands

- UPS connection:

```bash
cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis -- \
  device serial-04f3bb3f5367 connection
```

- UPS power trace:

```bash
cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis -- \
  device serial-04f3bb3f5367 trace --kind event --trace-limit 20
```

- UPS `power-diag` snapshot:

```bash
curl -fsS http://127.0.0.1:30080/api/v1/devices/serial-04f3bb3f5367/power-diag
```

- UPS LAN `status` snapshot:

```bash
curl -fsS http://192.168.31.232/api/v1/status
```

- Load baseline / stimulus:

```bash
loadlynx status --url http://192.168.31.216 --json
loadlynx cc <target_i_ma> --url http://192.168.31.216 --max-i-ma-total 5500 --max-p-mw 200000
loadlynx control set --url http://192.168.31.216 --enable
loadlynx control set --url http://192.168.31.216 --disable
```

- IsolaPurr source toggling:

```bash
curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=0'
curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=1'
```

## Execution Sequence

### 1. `STANDBY` baseline

- Keep LoadLynx disabled.
- Accepted baseline environment:
  - IsolaPurr: manual `13.0V / 3.0A`, `port_c=13026mV / 4mA`
  - LoadLynx: disabled, `v_local_mv=13050`, `calc_p_mw=91`
  - UPS input: `input_vbus_mv=5088`, `input_ibus_ma=209`, `vin_vbus_mv=13024`, `vin_iin_ma=28`, `tps_total_iout_ma=40`
  - UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/20mA`
  - Battery: `pack_mv=16234`, `current_ma=0`, `soc_pct=93`
  - Charger: `detail_status=WAIT`, `allow_charge=false`

### 2. `1A` direct-input check

- Set LoadLynx to `CC 1000mA` and enable output.
- Accepted `1A` environment:
  - IsolaPurr: `port_c=12971mV / 1003mA`
  - LoadLynx: `output_enabled=true`, `target_i_ma=1000`, `i_local_ma=1002`, `calc_p_mw=12774`
  - UPS input: `input_vbus_mv=5089`, `input_ibus_ma=208`, `vin_vbus_mv=12720`, `vin_iin_ma=1051`, `tps_total_iout_ma=40`
  - UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/20mA`
  - Battery: `pack_mv=16234`, `current_ma=0`
  - Charger: `detail_status=WAIT`
- Acceptance meaning:
  - direct input is still carrying the load
  - TPS output current stays near baseline
  - battery current stays at `0`
  - UPS remains `mode=standby`

### 3. `2A` direct-input check

- Step LoadLynx to `CC 2000mA`.
- Accepted `2A` environment:
  - IsolaPurr: `port_c=12970mV / 2002mA`
  - LoadLynx: `output_enabled=true`, `target_i_ma=2000`, `i_local_ma=1000`, `i_remote_ma=988`, `calc_p_mw=24512`
  - UPS input: `input_vbus_mv=5087`, `input_ibus_ma=208`, `vin_vbus_mv=12472`, `vin_iin_ma=2068`, `tps_total_iout_ma=36`
  - UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/20mA`
  - Battery: `pack_mv=16234`, `current_ma=0`
  - Charger: `detail_status=WAIT`
- Acceptance meaning:
  - `2A` still stays in `STANDBY`
  - battery assist has not started
  - this stage invalidates the earlier too-early `2A -> ASSIST` conclusion from the lower-voltage bench setup

### 4. `3A` source-ceiling check

- Step LoadLynx to `CC 3000mA`.
- Accepted `3A` environment:
  - IsolaPurr: `port_c=12841mV / 2978mA`
  - LoadLynx: `output_enabled=true`, `target_i_ma=3000`, `i_local_ma=1499`, `i_remote_ma=1487`, `calc_p_mw=35473`
  - UPS input: `input_vbus_mv=5088`, `input_ibus_ma=210`, `vin_vbus_mv=12088`, `vin_iin_ma=3062`, `tps_total_iout_ma=68`
  - UPS output: `out_a=12064mV/16mA`, `out_b=12064mV/52mA`
  - Battery: `pack_mv=16231`, `current_ma=-22`
  - Charger: `detail_status=WAIT`
- Acceptance meaning:
  - the source is near its `3A` ceiling
  - UPS still stays in `STANDBY`
  - `tps_total_iout_ma=68` remains below the `100mA` `ASSIST` threshold
  - the tiny negative battery current is not enough to count as `ASSIST`

### 5. `3200mA` `ASSIST` entry

- Step LoadLynx to `CC 3200mA`.
- Accepted `3200mA` environment:
  - IsolaPurr: `port_c=12836mV / 2978mA`
  - LoadLynx: `output_enabled=true`, `target_i_ma=3200`, `i_local_ma=1600`, `i_remote_ma=1587`, `calc_p_mw=37826`
  - UPS input: `input_vbus_mv=5086`, `input_ibus_ma=214`, `vin_vbus_mv=12096`, `vin_iin_ma=3062`, `tps_total_iout_ma=272`
  - UPS output: `out_a=12064mV/16mA`, `out_b=12072mV/256mA`
  - Battery: `pack_mv=16208`, `current_ma=-173`
  - Charger: `detail_status=LOAD`, `allow_charge=false`
- Acceptance meaning:
  - IsolaPurr current has already plateaued near `3A`
  - the extra output demand is now being covered by battery/TPS
  - `tps_total_iout_ma` crosses the `100mA` enter threshold
  - UPS transitions to `mode=supplement`
  - charger token changes to `LOAD`

### 6. `BACKUP` entry at `3200mA`

- Keep LoadLynx at `3200mA`.
- Cut input with:

  ```bash
  curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=0'
  ```

- Accepted `BACKUP` environment:
  - IsolaPurr: `port_c_enabled=false`, no valid `port_c` or `usb_c` voltage/current telemetry
  - LoadLynx: still enabled, `target_i_ma=3200`, `i_local_ma=1600`, `i_remote_ma=1587`, `calc_p_mw=37523`
  - UPS input: `input.source=usbc`, `mains_present=false`, `input_vbus_mv=5076`, `input_ibus_ma=255`, `vin_vbus_mv=2096`, `vin_iin_ma=5`, `tps_total_iout_ma=3300`
  - UPS output: `out_a=12000mV/1660mA`, `out_b=12000mV/1640mA`
  - Battery: `pack_mv=15882`, `current_ma=-2462`, `soc_pct=93`
  - Charger: `detail_status=NOAC`, `allow_charge=false`
- Acceptance meaning:
  - UPS loses confirmed input and enters `mode=backup`
  - output power is now carried by battery/TPS
  - charger token changes to `NOAC`

### 7. Restore `STANDBY`

- Re-enable the source:

  ```bash
  curl -fsS -X POST 'http://192.168.31.122/api/v1/ports/port_c/power?enabled=1'
  ```

- Disable LoadLynx and wait for UPS `mains_present=true`.
- Accepted restored environment:
  - IsolaPurr: `port_c=13024mV / 3mA`, `port_c_enabled=true`
  - LoadLynx: disabled, `v_local_mv=13038`, `calc_p_mw=91`
  - UPS input: `input_vbus_mv=5089`, `input_ibus_ma=206`, `vin_vbus_mv=13024`, `vin_iin_ma=28`, `tps_total_iout_ma=36`
  - Battery: `pack_mv=16203`, `current_ma=0`
  - Charger: `detail_status=WAIT`
  - UPS mode: `standby`

## Through Conditions

- `13.0V` baseline must show `vin_vbus_mv` above TPS output voltage in `STANDBY`.
- `1A`, `2A`, and `3A` must remain acceptable only if UPS still reports `STANDBY`, `battery.current_ma` stays near `0`, and `tps_total_iout_ma` stays below the `100mA` `ASSIST` threshold.
- `3200mA` must show `ASSIST`, negative battery current, `tps_total_iout_ma > 100mA`, and charger `LOAD`.
- `BACKUP` must show `mains_present=false`, charger `NOAC`, and `tps_total_iout_ma` carried by battery/TPS.
- This run validates runtime-mode coupling, not the separate charger-pressure branch:
  - all accepted stages still showed `pressure_reason=none`
  - `LOAD` came from runtime-mode coupling, not from the `eu2b8` pressure/cooldown path
- If fresh `trace(kind=event,target=power)` events are unavailable, record the run as degraded-on-trace and rely on `status + power-diag` only.

## Raw Evidence

- `/tmp/mains-aegis-hil-20260617-175412-13000-final`
- `/tmp/mains-aegis-hil-20260617-180711-13000-backup-tail-v3`
