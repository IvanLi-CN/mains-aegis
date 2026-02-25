# ina3221-async

Rust driver for TI **INA3221** (3-channel shunt + bus voltage monitor over I2C).

- `#![no_std]`
- Optional `async` API via `embedded-hal-async`
- Optional `defmt` support

This repository is intentionally kept minimal; higher-level alert configuration (PV/WARNING/CRITICAL)
can be added on top as needed.

