# Plan: BQ40Z50 BMS bring-up (SMBus poll + fault expectations)

## Background / Problem

We have populated (soldered) the BMS / battery protection circuit based on **TI BQ40Z50-R2**
(pack manager / protector) on the mainboard.

Firmware needs basic SMBus observability so we can validate wiring and see expected fault
reporting while **no battery pack / cells are connected**.

References:

- BMS design notes: `docs/bms-design.md`
- PCB netlist summary: `docs/pcbs/mainboard/README.md`
- I2C map: `docs/i2c-address-map.md`
- GPIO allocation: `docs/hardware-selection/esp32-s3-fh4r2-gpio.md`
- TRM (offline): `docs/manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md`
- Datasheet (offline): `docs/datasheets/BQ40Z50-R2/BQ40Z50-R2.md`

## Goals

- Add a minimal BQ40Z50 integration to firmware:
  - poll key SBS word commands (Voltage/Current/Temperature/BatteryStatus + cell voltages);
  - print a stable, greppable `bms:` line (addr + snapshot + decoded BatteryStatus bits);
  - sample `BMS_BTP_INT_H` (`GPIO21`) and include it in logs.

## Non-goals (for this PR)

- Full BMS policy for enabling/disabling charger and power rails.
- Data flash programming / sealing / authentication flows.
- Secondary OVP (`BQ296100`) fuse-blow validation.

## Scope (In / Out)

### In

- Firmware configures:
  - `GPIO21` as input for `BMS_BTP_INT_H` (active-high on alert).

- Firmware polls BQ40Z50 over `I2C1`/SMBus at `400kHz`:
  - prefer `addr=0x0B` (per project I2C map); fallback `addr=0x16` (TI TRM default) if `0x0B`
    NACKs.
  - reads:
    - Temperature() `0x08`
    - Voltage() `0x09`
    - Current() `0x0A`
    - RelativeStateOfCharge() `0x0D`
    - RemainingCapacity() `0x0F`
    - FullChargeCapacity() `0x10`
    - BatteryStatus() `0x16`
    - CellVoltage1..4() `0x3F..0x3C`
  - on errors: classify as `i2c_nack` / `i2c_timeout` / ... and back off retries.

### Out

- Any port selection / enumeration.
- Any commands that can change BQ40Z50 state (ManufacturerAccess writes, FET toggles, etc).

## Requirements (MUST)

- MUST never crash / panic when BQ40Z50 is absent or unpowered.
- MUST rate-limit error logs (do not spam every telemetry tick).
- MUST log at least:
  - which SMBus address responded (`0x0B` or `0x16`)
  - BatteryStatus raw hex + key flags (`INIT/DSG/FC/FD` and alarms)
  - pack voltage/current/temp and per-cell voltages (raw)

## Acceptance Criteria

- Given BQ40Z50 is reachable on `I2C1`/SMBus (either `0x0B` or `0x16`),
  - When firmware polls SBS word commands,
  - Then logs contain a `bms:` line with decoded snapshot fields and `err` is not present.

- Given no battery pack / cells are connected (current bring-up condition),
  - When firmware polls BQ40Z50,
  - Then one of the following is observed and is treated as expected:
    - device does not ACK (e.g. `i2c_nack` / `i2c_timeout`), and firmware keeps running; or
    - device responds but reports "empty/uninit/faulty" status (e.g. `INIT=0` or alarms set), and
      firmware logs the raw flags.

## Testing

- Local automated:
  - `cd firmware && cargo build --release`
- Manual (human, optional):
  - Flash + monitor with `mcu-agentd` and confirm `bms:` line behavior matches the "no pack"
    expectations above.

## Risks / Open Questions

- SMBus address: project doc says `0x0B`; TI TRM states default `0x16`. We implement an auto-probe
  (`0x0B -> 0x16`) to keep bring-up resilient until DF is confirmed.
- Without cells connected, reported SBS values may be zeros/`0xFFFF` or alarms; treat as
  signal-level validation rather than gauging accuracy.

