# Low-Voltage Recovery HIL

This page defines the controlled hardware-in-the-loop path for the low-voltage recovery feature. The flow intentionally uses two ESP32-S3 firmware flashes:

1. Temporary `tools/bq40-comm-tool` firmware applies the BQ40Z50 live Data Flash baseline.
2. Main firmware is flashed back and verified through USB CDC `power-diag`.

The main firmware must not write BQ40 Data Flash. BQ40 DF maintenance stays behind the explicit `bq40-comm-tool` `live-df-mainboard` entrypoint.

## Safety Scope

- Approved HIL device id: `serial-04f3bb3f5367`.
- Approved HIL serial port: `/dev/cu.usbmodem212301`.
- Denied serial port: `/dev/cu.usbmodem212101`.
- Agents must not directly invoke `espflash`, `cargo espflash`, or `cargo-espflash`.
- Agents must not run `mcu-agentd selector list`, `mcu-agentd selector set`, enumerate `/dev/*`, or try alternate ports.
- Real flash/reset/monitor operations require the known approved target and owner authorization.

## Runner

The project runner is:

```bash
tools/hil/low-voltage-recovery.sh --dry-run
```

Real HIL requires the approved target to be stated explicitly:

```bash
tools/hil/low-voltage-recovery.sh \
  --real \
  --device-id serial-04f3bb3f5367 \
  --port /dev/cu.usbmodem212301
```

The runner refuses real HIL when either `firmware/.esp32-port` or `tools/bq40-comm-tool/.esp32-port` points at another port. This prevents the temporary tool firmware and the final main firmware from being flashed to different devices.

## HIL Sequence

1. Validate that both local mcu-agentd selector caches are bound to `/dev/cu.usbmodem212301`.
2. Run:

   ```bash
   tools/bq40-comm-tool/bin/run.sh apply-df \
     --mode canonical \
     --duration-sec 120 \
     --force-min-charge true \
     --repair-profile live-df-mainboard
   ```

3. Build main firmware:

   ```bash
   cd firmware
   cargo build --release --bin esp-firmware --features net_http,web_serial
   ```

4. Generate a Firmware Catalog manifest for the built main firmware.
5. Start or reuse `mains-aegis-devd`.
6. Run devd scan, bind only `serial-04f3bb3f5367`, select the generated manifest, and flash the main firmware through devd.
7. Reconnect through devd and read `GET /api/v1/devices/{id}/power-diag`.

## Pass Criteria

The HIL runner writes reports under `tools/hil/reports/<timestamp>/`. A pass requires:

- BQ40 DF apply report completed through `live-df-mainboard`.
- Main firmware flash response includes backend success from devd.
- USB `power-diag` is readable from the main firmware.
- `power-diag.charger.vbat_lowv_pct_x10 == 714`.
- `power-diag.charger.iprechg_ma == 120`.
- `power-diag.bms.cuv_recovery_mv == 2900`.
- `power-diag.bms.cuv_recov_chg == true`.
- `power-diag.policy.recovery_stage` is either `null`, `bq40_pchg`, or `bq25792_precharge`.

When the physical pack is actually in the low-voltage recovery window, run with `--require-recovery-state true`. That additionally requires `power-diag.policy.status == RECOV` and a non-null recovery stage.
