# TPS55288 Buck-Boost Driver (Rust)

 Rust driver skeleton for TI TPS55288 36 V / 16 A four-switch buck-boost converter with I2C control. This repo follows the patterns proven in `sc8815-rs` (no-std, sync by default, async behind the `async` feature, type-safe registers, strong error handling). Core driver helpers are in place; example/CI are pending.

## Status
- Sync + async driver helpers implemented (I2C read/write, VOUT/ILIM, VOUT_SR, feedback, cable comp, STATUS decode, init).
- Datasheet-driven register modeling in place; conversion helpers tested.
- STM32G031G8U6 reference example reserved under `examples/stm32g031g8u6/` (code TBD).

## Scope & Goals
- Provide a safe, no-std Rust driver with optional async + defmt.
- Cover I2C register map: output voltage/current limits, mode control (buck/boost/auto), PPS-style programmable voltage steps, protections, status/interrupts.
- Ship integration example for STM32G031G8U6 demonstrating PPS-like VOUT control and fault polling.

## Usage (sync)
```rust
use tps55288::driver::Tps55288;
use tps55288::{VoutSlewRate, OcpDelay, FeedbackSource, InternalFeedbackRatio, CableCompOption, CableCompLevel};
use embedded_hal::i2c::I2c;

fn example<I2C: I2c>(i2c: I2C) {
    let mut dev = Tps55288::new(i2c);
    // init() configures safe defaults but leaves OE disabled so that
    // callers can finish all register writes before enabling the power stage.
    dev.init().ok();
    // Configure output voltage, current limit, slew rate, feedback, and cable compensation.
    dev.set_vout_mv(5_000).ok();
    dev.set_ilim_ma(3_000, true).ok();
    dev.set_vout_sr(VoutSlewRate::Sr2p5MvPerUs, OcpDelay::Us128).ok();
    dev.set_feedback(FeedbackSource::Internal, InternalFeedbackRatio::R0_0564).ok();
    dev.set_cable_comp(CableCompOption::Internal, CableCompLevel::V0p0, true, true, true).ok();
    // Finally enable the output once configuration is complete.
    dev.enable_output().ok();

    let _ = dev.read_status();
}
```

## Repo Layout
- `src/` — registers, data types, driver core (sync + async helpers), error handling.
- `examples/stm32g031g8u6/` — board wiring notes and build plan (code pending).
- `docs/tps55288-datasheet.md` — full datasheet extraction with images.
- `DESIGN.md` — architecture and API plan.
- `TODO.md` — execution checklist.

## Next Steps
See `TODO.md` for the ordered task list. Remaining work includes STM32 example, CI/lefthook, defmt display polish, and expanded docs.
