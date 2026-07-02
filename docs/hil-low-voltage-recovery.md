# Low-Voltage Recovery HIL

This page defines the controlled hardware-in-the-loop path for the low-voltage recovery feature. The flow intentionally uses two ESP32-S3 firmware flashes:

1. Temporary `tools/bq40-comm-tool` firmware applies the BQ40Z50 live Data Flash baseline.
2. Main firmware is flashed back and verified through USB CDC `diag-snapshot`.

The main firmware must not write BQ40 Data Flash. BQ40 DF maintenance stays behind the explicit `bq40-comm-tool` `live-df-mainboard` entrypoint.

## Safety Scope

- Real HIL has no baked-in device id, serial-port allowlist, or serial-port denylist.
- The owner must provide the current devd device id and serial port in the same invocation with `--device-id` and `--port`.
- The runner validates that devd scan results and selector caches match the explicit target exactly before any real flash step.
- Agents must not directly invoke `espflash`, `cargo espflash`, or `cargo-espflash`.
- Agents must not use `mcu-agentd` as a Mains Aegis hardware operation path, enumerate `/dev/*`, or try alternate ports.
- Real flash/reset/monitor operations require an explicit owner-supplied target and owner authorization.

## Runner

The project runner is:

```bash
tools/hil/low-voltage-recovery.sh --dry-run
```

Real HIL requires the current target to be stated explicitly:

```bash
tools/hil/low-voltage-recovery.sh \
  --real \
  --device-id <devd-device-id> \
  --port <serial-port>
```

The runner refuses real HIL when either `firmware/.esp32-port` or `tools/bq40-comm-tool/.esp32-port` points at another port. This prevents the temporary tool firmware and the final main firmware from being flashed to different devices.

## HIL Sequence

1. Validate that the runner received explicit `--device-id` and `--port`, then confirm devd scan results and selector caches match that target exactly.
2. Run:

   ```bash
   tools/bq40-comm-tool/bin/run.sh apply-df \
     --mode canonical \
     --duration-sec 120 \
     --force-min-charge true \
     --repair-profile live-df-mainboard
   ```

   The runner exports the explicit devd target for this step, so the temporary tool firmware is flashed through `mains-aegis-devd` as well.

3. Build main firmware:

   ```bash
   just firmware-build
   ```

4. Generate a Firmware Catalog manifest for the built main firmware. The
   runner uses the same artifact generation path as `just firmware-release`.
5. Start or reuse `mains-aegis-devd`.
6. Run devd scan, bind only the explicit `--device-id`, select the generated manifest, and flash the main firmware through devd.
7. Reconnect through devd and read `GET /api/v1/devices/{id}/diag-snapshot?package=bq25792.regs&package=bq40.manufacturing&package=derived.power`.

## Pass Criteria

The HIL runner writes reports under `tools/hil/reports/<timestamp>/`. A pass requires:

- BQ40 DF apply report completed through `live-df-mainboard`.
- Main firmware flash response includes backend success from devd.
- USB `diag-snapshot` is readable from the main firmware.
- `diag-snapshot.packages["derived.power"].payload.charger.vbat_lowv_pct_x10 == 714`.
- `diag-snapshot.packages["derived.power"].payload.charger.iprechg_ma == 120`.
- `diag-snapshot.packages["derived.power"].payload.bms.cuv_recovery_mv == 2550`.
- `diag-snapshot.packages["derived.power"].payload.bms.cuv_recov_chg == false`.
- `diag-snapshot.packages["derived.power"].payload.policy.recovery_stage` is either `null`, `bq40_pchg`, or `bq25792_precharge`.

When the physical pack is actually in the low-voltage recovery window, run with `--require-recovery-state true`. That additionally requires `diag-snapshot.packages["derived.power"].payload.policy.status == RECOV` and a non-null recovery stage.
