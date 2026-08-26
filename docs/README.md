# Docs index

This folder stores project documentation and offline-renderable datasheets.

## Project docs

- Hardware selection overview: `docs/hardware-selection.md`
- Dashboard module design: `firmware/ui/dashboard-design.md`
- Self-check module design: `firmware/ui/self-check-design.md`
- Firmware bring-up (ESP32-S3 / esp-hal / no_std): `firmware/README.md`
- Module docs index: `docs/modules/README.md`
- Regulated output module (TPS55288 + TMP112 + INA3221 output channels): `docs/modules/regulated-output.md`
- Code quality & CI: `docs/quality-gates.md`
- BMS design (system-level): `docs/bms-design.md`
- Charger design (BQ25792 + PD/PPS): `docs/charger-design.md`
- Boot self-test flow (module gating + emergency-stop): `docs/boot-self-test-flow.md`
- UPS main output design: `docs/ups-output-design.md`
- Web management UI plan: `docs/web-management-ui.md`
- Web summary ownership decision: `docs/adr/0001-assign-summary-ownership-to-pages.md`
- USB CDC / Web Serial protocol: `docs/usb-cdc-web-serial-protocol.md`
- Mains Aegis device daemon, host power control, and firmware catalog: `docs/specs/mains-aegis-devd/SPEC.md`, `docs/firmware-catalog.md`
- Low-voltage recovery maintenance: `docs/recovery/low-voltage-recovery.md`
- Runtime-mode Power Path Validation: `docs/hil-runtime-mode-switching.md`
  - active truth source for `12V / 3A` runtime-mode scene design, three-device data capture, output-voltage fluctuation acceptance, and current candidate evidence
- Power Path Validation runbook: `tools/hil/README.md`
- Agent hardware collaboration workflow: `docs/hardware-collaboration-workflow.md`
- Power monitoring & protection (INA3221 + UPS VIN/TPS outputs): `docs/power-monitoring-design.md`
- I2C/SMBus address map: `docs/i2c-address-map.md`
- Audio alert output (buzzer -> TDM speaker): `docs/audio-design.md`
- Audio cue comparison and preview: `docs-site/docs/design/audio-cues.mdx`
- Speaker cue previews (status/warning/error): `docs/audio-cues-preview/README.md`
- Marketing product render, social preview, and poster assets: `docs/marketing/mains-aegis/README.md`
- ESP32-S3 GPIO assignment: `docs/hardware-selection/esp32-s3-fh4r2-gpio.md`
- Solutions index: `docs/solutions/README.md`

## UI docs

- UI docs index: `firmware/ui/README.md`
- Front panel design language (SoT): `firmware/ui/design-language.md`
- Front panel component contracts: `firmware/ui/component-contracts.md`
- Front panel touch target design: `firmware/ui/touch-targets.md`
- Front panel visual regression checklist: `firmware/ui/visual-regression-checklist.md`

## Datasheets

- Datasheets index: `docs/datasheets/README.md`

## PCBs

- PCBs index: `docs/pcbs/README.md`

## Manuals

- Manuals index: `docs/manuals/README.md`

## Reference designs

- Reference designs index: `docs/reference-designs/README.md`
