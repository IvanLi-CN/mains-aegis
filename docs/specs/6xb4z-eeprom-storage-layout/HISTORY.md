# EEPROM storage layout history (#6xb4z)

## Decisions

- EEPROM layout is tracked as a dedicated topic spec because multiple feature specs now write persistent records.
- Manual charge prefs remain the only table-managed record under `StorageRecordTableV1`; newer fixed-address records are documented explicitly until a table migration is designed.
- Beeper volume preferences use an independent `BEEP` record at `0x01e0` to avoid rewriting or migrating the existing manual charge table.
- Beeper default volume is `L4` for both ACTION and SYSTEM routes so a fresh EEPROM no longer starts at maximum volume.
