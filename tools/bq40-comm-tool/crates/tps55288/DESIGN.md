# TPS55288 Driver Design (Rust, no-std)

This document maps the `sc8815-rs` patterns onto TI TPS55288 (36 V, 16 A, four-switch buck-boost with I2C). No code is implemented yet; this is the blueprint for register modeling, APIs, and validation.

## Hardware Notes (datasheet highlights)
- Input 2.7–36 V; output 0.8–22 V, DAC LSB 20 mV (10-bit DAC), PPS-friendly steps.
- Programmable output current limit up to ~6.35 A (50 mA step) via I2C and sense resistor.
- Avg current-mode control; switching freq 200 kHz–2.2 MHz (R on FSW), optional sync and spread spectrum; fixed ~4 ms soft-start.
- Modes: buck/boost/auto (four-switch), light-load PFM or FPWM selectable; MODE pin also selects I2C address (to confirm from register map section).
- Protections: OVP/UVP, current limit (avg + peak), hiccup for short, OTP, output discharge, watchdog/UVLO on EN.
- Status/FAULT: status bits and write-1-to-clear registers (see register map); FB/INT pin can signal faults when INT mode selected.

## Register Map Summary (from datasheet)
- 0x00/0x01 `REF`: Reference voltage DAC (10-bit, 20 mV LSB, VOUT range 0.8–22 V).
- 0x02 `IOUT_LIMIT`: Output current limit (sense between ISP/ISN, 50 mA LSB, ~6.35 A max).
- 0x03 `VOUT_SR`: Slew-rate control for VOUT steps.
- 0x04 `VOUT_FS`: Feedback selection/internal DAC vs external FB divider.
- 0x05 `CDC`: Cable droop compensation.
- 0x06 `MODE`: Light-load mode (PFM/PWM), VCC source (internal/external 5 V), I2C address select (0x74/0x75), hiccup, discharge, FSW double, OE.
- 0x07 `STATUS`: Operating/fault status (write-1-to-clear fields; railed faults on FB/INT when INT mode used).

### MODE pin preset table (datasheet)
- Resistor presets choose I2C address and light-load behavior: 0 kΩ (int VCC, 0x74, PWM), 6.19 kΩ (int, 0x74, PFM), 14.3 kΩ (int, 0x75, PWM), 24.9 kΩ (int, 0x75, PFM), 51.1 kΩ (ext, 0x74, PWM), 75.0 kΩ (ext, 0x74, PFM), 105 kΩ (ext, 0x75, PWM), Open (ext, 0x75, PFM).

## Architecture (mirrors `sc8815-rs`)
- **Crate shape**: `lib.rs` exports types + driver, doc-includes README when ready.
- **Modules**
  - `registers.rs`: address map, bitfields via `bitflags`, conversion helpers (raw↔physical units), constants (default addr, ranges, steps).
  - `data_types.rs`: enums/structs for modes, voltage/current limits, frequency steps, protection configs, status, faults.
  - `driver.rs`: sync/async (via `maybe-async-cfg`) I2C wrapper; high-level methods: init/reset, set/get VOUT, set ILIM, configure mode, enable/disable, read status/fault, clear faults, configure switching freq/soft-start/dither if available.
  - `error.rs`: generic error over I2C + validation errors (out-of-range), plus defmt support when feature is on.
- **Feature flags**
  - `async`: depend on `embedded-hal-async`, mirror APIs.
  - `defmt`: logging support for errors/types.

## API Surface (planned)
- Constructors: `new(i2c, addr)`, `into_async` symmetry optional.
- Core controls: `init()`, `reset()`, `enable(bool)`, `set_mode(Buck|Boost|Auto)`, `set_vout_mv(u16)`, `set_current_limit_ma(u16)`, `set_switching_frequency(freq enum)`, `set_soft_start(time enum)`, `set_dither(bool)` (if supported).
- Protections: `configure_ovp_uvp`, `configure_ocp`, `configure_otp`, `set_watchdog`, `clear_faults()`, `read_faults()`.
- Status/telemetry: `read_status()`, `read_operating_point()`, optional ADC reads if exposed.
- Low-level passthrough: `read_reg`, `write_reg`, `update_reg_masked` for advanced users.
- Validation: clamp/return error when VOUT/ILIM/Frequency outside datasheet limits.

- Define enums with exact step sizes per register field (VOUT DAC 20 mV LSB, ILIM 50 mA LSB capped at ~6.35 A, freq options 200 kHz–2.2 MHz, soft-start fixed 4 ms noted in docs).
- Provide `try_from`/`to_raw` helpers for physical↔raw conversions; round to nearest legal step.
- Mark write-1-to-clear fields; expose `clear_faults()` that writes the appropriate mask.
- Keep raw accessors for users needing non-standard values.

## Testing Strategy
- Unit tests with `embedded-hal-mock` for register writes/reads sequences.
- Table-driven tests for conversion helpers (VOUT, ILIM, freq, timing).
- Feature-matrix CI: sync, async, sync+defmt builds; `no_std` compilation check.
- (Later) HIL tests: scripted I2C transactions against hardware or simulator.

## Example: STM32G031G8U6 (planned under `examples/stm32g031g8u6/`)
- Goal: demonstrate PPS-like dynamic VOUT and ILIM on a minimal STM32G031 board.
- Wiring assumptions: I2C1 pins (SCL/SDA), EN pin control via GPIO if required, IRQ/PG pins optional; VIN via bench supply, VOUT to load.
- Flow: init → set mode auto → set VOUT (e.g., 5 V), ramp to 9/12/15/20 V steps, set ILIM, poll status/fault → clear faults → loop.
- Build: `cargo run --example stm32g031g8u6` with target/board config described in example README (using Embassy or HAL TBD).

## Work Phases
1) Datasheet extraction → `registers.rs` skeleton + constants.
2) Data types and conversions.
3) Driver scaffolding (sync first) + validation.
4) Async parity + defmt annotations.
5) STM32G031 example bring-up.
6) Tests/CI + docs polish.

## Deliverables for this init phase
- Repo skeleton and metadata.
- Placeholder modules/files.
- TODO checklist to drive implementation.
