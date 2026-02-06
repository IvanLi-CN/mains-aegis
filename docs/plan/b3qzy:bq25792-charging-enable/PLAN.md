# Plan: BQ25792 charging enable + status capture

## Background / Problem

We have populated (soldered) the charger circuit based on **TI BQ25792**. The firmware must:

- correctly detect charger / battery state via I2C registers, and
- enable charging safely (respecting `CE` active-low semantics),
- while keeping bring-up observability (logs) for debugging.

References:

- Charger design notes: `docs/charger-design.md`
- I2C map: `docs/i2c-address-map.md`
- GPIO allocation: `docs/hardware-selection/esp32-s3-fh4r2-gpio.md`
- Datasheet (offline): `docs/datasheets/BQ25792/BQ25792.md`

## Goals

- Add a minimal BQ25792 integration to firmware:
  - read key status / fault registers;
  - print a stable, greppable status line;
  - enable charging when it is safe to do so.

## Non-goals (for this PR)

- USB-C/PD policy (PPS/APDO contracts).
- Full three-current profile management (`1A/500mA/100mA`) and DPM tuning.
- OTG mode and role-switch sequencing.
- BMS (BQ40Z50) policy integration (we only gate on BQ25792 status for now).

## Scope (In / Out)

### In

- Firmware configures these GPIOs (per GPIO map):
  - `GPIO15` `CHG_CE` (active-low charge enable)
  - `GPIO16` `CHG_ILIM_HIZ_BRK` (brake; keep deasserted)
  - `GPIO17` `CHG_INT` (optional; not required for minimal bring-up)
- Firmware talks to BQ25792 on `I2C1 addr=0x6B`:
  - reads `REG1B..REG1F` (charger status 0..4)
  - reads `REG20..REG21` (fault status)
  - reads `REGOF` (charger control 0) to validate / enforce `EN_CHG=1` and `EN_HIZ=0`
- Charging enable policy:
  - default: `CE=HIGH` (disabled)
  - enable only when:
    - `VBAT_PRESENT_STAT=1` (REG1D bit0)
    - TS is not out-of-range: `TS_COLD_STAT=0` and `TS_HOT_STAT=0` (REG1F bits3/0)
  - otherwise keep disabled; on I2C errors also keep disabled (fail-safe)

### Out

- Any hardware flashing/monitoring automation.
- Any port selection / enumeration.

## Requirements (MUST)

- MUST keep charging disabled on boot until at least one successful status read is performed.
- MUST not enable charging if battery is not present (per `VBAT_PRESENT_STAT`).
- MUST not enable charging if TS indicates cold/hot range (per `TS_*_STAT`).
- MUST log charger state periodically (at least every 1s), including:
  - `chg_enabled` (our GPIO decision)
  - `vbus_present/ac1_present/ac2_present/pg`
  - `chg_stat` and `vbus_stat`
  - `vbat_present` and `ts_{cold,cool,warm,hot}`
  - `fault_status_0/1` (raw hex)

## Acceptance Criteria

- Given firmware boots with BQ25792 reachable on I2C,
  - When `VBAT_PRESENT_STAT=1` and TS is not cold/hot,
  - Then firmware sets `CHG_CE=LOW`, and logs show `chg_enabled=true`.

- Given battery is removed (or BQ25792 reports `VBAT_PRESENT_STAT=0`),
  - When firmware detects this in periodic polling,
  - Then firmware sets `CHG_CE=HIGH`, and logs show `chg_enabled=false`.

- Given TS goes to cold/hot range (BQ25792 reports `TS_COLD_STAT=1` or `TS_HOT_STAT=1`),
  - When firmware detects this in periodic polling,
  - Then firmware disables charging (`CHG_CE=HIGH`) and logs reflect TS state.

- Given any I2C error talking to BQ25792,
  - Then firmware keeps charging disabled and prints an error log with `err=<kind>`.

## Testing

- Local automated:
  - `cd firmware && cargo build --release`
- Manual (human, optional):
  - Flash + monitor with `mcu-agentd` and confirm logs contain `charger:` line and `chg_enabled` toggles as expected.

## Risks / Open Questions

- `CE` polarity: datasheet pin definition says **charge enabled when `EN_CHG=1` and `CE=LOW`**; some text elsewhere is inconsistent. We follow pin definition.
- Watchdog/default-mode interaction: we should avoid unnecessary register writes unless needed; if we write, we should not rely on staying in host mode.

