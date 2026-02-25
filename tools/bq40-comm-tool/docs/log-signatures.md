# Log Signatures

## Core communication

- Prefix note: parser accepts both raw `bms: ...` lines and defmt-style `[INFO ] bms: ...`.
- `bms: bq40z50 discovered addr=0xb`
  - BQ40 endpoint discovered
- `bms: addr=0xb temp_c_x10=... voltage_mv=... current_ma=... soc_pct=... status=0x...`
  - valid sample candidate line used by report parser

## Poll diagnostics

- `bms_diag: addr=0xb stage=poll_snapshot err=i2c_nack`
  - SMBus transport nack during poll
- `bms_diag: addr=0xb stage=poll_snapshot err=inconsistent_sample`
  - dual-read consistency check failed
- `bms: bq40z50 transport_retry ...`
  - transient transport error, retry without dropping endpoint
- `bms: bq40z50 transport_lost ...`
  - transport fail streak exceeded threshold, endpoint dropped

## ROM recovery

- `stage=rom_mode_detected`
  - ROM signature observed
- `stage=probe_rom_flash_begin`
  - flash recovery attempt started
- `stage=rom_flash_start`
  - ROM flash sequence entered
- `stage=rom_flash_done`
  - flash sequence completed with readback
- `stage=probe_rom_flash_done`
  - probe flow returned from ROM flash attempt

## Address semantics

- `fw: bms_addr_semantics addr7=0x0b addr8_w=0x16 addr8_r=0x17`
  - canonical runtime address and 8-bit byte mapping
