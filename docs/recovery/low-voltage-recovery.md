# Low-Voltage Recovery Maintenance

This page defines the controlled maintenance path for the low-voltage recovery baseline. The flow intentionally uses two ESP32-S3 firmware flashes:

1. Temporary `tools/bq40-comm-tool` firmware applies the explicit seven-field
   `bq40-charge-temperature` Data Flash profile.
2. Main firmware is flashed back and verified through USB CDC `diag-snapshot`.

The main firmware must not write BQ40 Data Flash. BQ40 charge-temperature
maintenance stays behind the explicit `bq40-comm-tool`
`bq40-charge-temperature` entrypoint. The retired `live-df-mainboard` name is
rejected and must not be replaced with a broad fallback writer.

This runner's temporary tool phase changes only the seven temperature fields.
It does not establish or rewrite the CUV/protection golden baseline; that
baseline remains owned by the explicit asset/ROM recovery path described in
the BQ40 maintenance documentation.

The runner therefore performs a read-only baseline preflight before the
temporary profile is written. A fresh/default pack, or a pack whose
`CUV Recovery` / `CUV_RECOV_CHG` values do not already match the project
baseline, stops before any Data Flash write. Run the explicit
`asset-df-mainboard` ROM-recovery path first when its ROM-signature
precondition is met; do not combine that recovery image with the
`bq40-charge-temperature` profile image.

## Safety Scope

- Real recovery maintenance has no baked-in device id, serial-port allowlist, or serial-port denylist.
- The owner must provide the current devd device id and serial port in the same invocation with `--device-id` and `--port`.
- The runner validates that devd scan results and selector caches match the explicit target exactly before any real flash step.
- Agents must not directly invoke `espflash`, `cargo espflash`, or `cargo-espflash`.
- Agents must not use `mcu-agentd` as a Mains Aegis hardware operation path, enumerate `/dev/*`, or try alternate ports.
- Real flash/reset/monitor operations require an explicit owner-supplied target and owner authorization.

## Runner

The project runner is:

```bash
tools/recovery/low-voltage-recovery.sh --dry-run
```

Real recovery maintenance requires the current target to be stated explicitly:

```bash
tools/recovery/low-voltage-recovery.sh \
  --real \
  --device-id <devd-device-id> \
  --port <serial-port>
```

The runner refuses real execution when either `firmware/.esp32-port` or `tools/bq40-comm-tool/.esp32-port` points at another port. This prevents the temporary tool firmware and the final main firmware from being flashed to different devices.

`tools.hil.test_*` unit tests and `--dry-run` invocations validate the runner's
software behavior and safety gates only. They must not be reported as HIL.

## Sequence

1. Validate that the runner received explicit `--device-id` and `--port`, then confirm devd scan results and selector caches match that target exactly.
2. Connect to the existing main firmware and verify the read-only low-voltage baseline (`CUV Recovery=2550mV`, `CUV_RECOV_CHG=0`, `VBAT_LOWV=71.4%`, and `IPRECHG=120mA`). If it is absent, stop and complete the explicit asset/ROM recovery path first.
3. Run:

   ```bash
   tools/bq40-comm-tool/bin/run.sh apply-df \
     --mode canonical \
     --duration-sec 120 \
     --force-min-charge true \
     --repair-profile bq40-charge-temperature
   ```

   The runner exports the explicit devd target for this step, so the temporary tool firmware is flashed through `mains-aegis-devd` as well.

4. Build main firmware:

   ```bash
   just firmware-build
   ```

5. Generate a Firmware Catalog manifest for the built main firmware. The
   runner uses the same artifact generation path as `just firmware-release`.
6. Start or reuse `mains-aegis-devd`.
7. Run devd scan, bind only the explicit `--device-id`, select the generated manifest, and flash the main firmware through devd.
8. Reconnect through devd and read `GET /api/v1/devices/{id}/diag-snapshot?package=bq25792.regs&package=bq40.manufacturing&package=derived.power`.

## Pass Criteria

The runner writes reports under `tools/recovery/reports/<timestamp>/`. A pass requires:

- BQ40 DF apply report completed through `bq40-charge-temperature` with exactly
  seven temperature fields, a containing-block snapshot, and per-field
  MB44 echo/length/raw/decoded readback.
- Main firmware flash response includes backend success from devd.
- USB `diag-snapshot` is readable from the main firmware.
- `diag-snapshot.packages["derived.power"].payload.charger.vbat_lowv_pct_x10 == 714`.
- `diag-snapshot.packages["derived.power"].payload.charger.iprechg_ma == 120`.
- `diag-snapshot.packages["derived.power"].payload.bms.cuv_recovery_mv == 2550`.
- `diag-snapshot.packages["derived.power"].payload.bms.cuv_recov_chg == false`.
- `diag-snapshot.packages["derived.power"].payload.policy.recovery_stage` is either `null`, `bq40_pchg`, or `bq25792_precharge`.

When the physical pack is actually in the low-voltage recovery window, run with `--require-recovery-state true`. That additionally requires `diag-snapshot.packages["derived.power"].payload.policy.status == RECOV` and a non-null recovery stage.
