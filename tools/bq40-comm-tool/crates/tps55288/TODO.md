# TODO

- [x] Pull latest TPS55288 datasheet; extract register map, defaults, limits (VOUT DAC, ILIM DAC, FSW, protections, address selection via MODE pin).
- [x] Define register addresses/bitfields in `src/registers.rs` with conversion helpers.
- [x] Model data types (modes, voltage/current limits, frequency, soft-start, protections, status/fault).
- [x] Scaffold driver API in `src/driver.rs` (sync), including low-level read/write/update helpers.
- [x] Add async parity via `embedded-hal-async` feature and mirror tests.
- [x] Add defmt formatting for public types and Error.
- [x] Write unit tests for conversions and register transactions using `embedded-hal-mock`.
- [ ] Create STM32G031G8U6 example (wiring notes + code) under `examples/stm32g031g8u6/`.
- [ ] Add CI workflows (lint/format/test feature matrix) and lefthook config.
- [ ] Expand README with usage examples once APIs stabilize.
