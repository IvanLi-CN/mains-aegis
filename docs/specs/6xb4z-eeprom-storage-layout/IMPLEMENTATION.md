# EEPROM storage layout implementation (#6xb4z)

## Current Coverage

- `firmware/src/output/mod.rs` owns EEPROM access on `I2C1 @ 0x50`.
- All current `output/mod.rs` EEPROM records use `32B` block reads/writes and CRC8 for single-block integrity.
- Table-managed storage is implemented for `ManualChargePrefsRecordV1` through `StorageSuperblockV1` and `StorageRecordTableV1`.
- Fixed-address records are implemented for:
  - `PdBreadcrumbRecordV1` ring at `0x0060..0x015f`
  - WiFi config record at `0x0160..0x01df`
  - `BeeperPrefsRecordV1` at `0x01e0`
  - `AdvancedPowerRecordV1` at `0x0200`
- Beeper preferences default to `L4 / L4 / Action` and persist only when preferences change.
- Advanced Power defaults to `1200 / 600 / 0 / 0 / 4 / 2`, persists only relative offsets/thresholds, and expands against the active device rated output at runtime.

## Known Gaps

- `StorageRecordTableV1` only indexes manual charge prefs. Later table expansion needs an explicit schema migration rather than silently changing V1 semantics.
- There is no dedicated host-unit mock for EEPROM byte layout round trips; validation currently relies on focused firmware tests, ESP build, and HIL monitor evidence.
- The WiFi config record integrity format is owned by `usb_cdc_protocol.rs`; this spec records the reserved address range but not every byte of the secret encoding.

## Verification Commands

- `cargo +stable fmt --manifest-path firmware/Cargo.toml`
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml beeper`
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml audio`
- `cargo +stable test --target $(rustc +stable -vV | sed -n 's/^host: //p') --manifest-path firmware/host-unit-tests/Cargo.toml`
- `cargo +esp build --release --bin esp-firmware`

## HIL Evidence Expectations

- After flashing a build that includes beeper persistence, device identity should match the selected artifact build id and devd log decode should be `verified`.
- Changing beeper volume should emit a verified monitor log containing `eeprom: beeper prefs saved action=... system=... selected=...`.
- Resetting or reflashing without erasing EEPROM should preserve the last saved beeper volume instead of returning to maximum volume.
