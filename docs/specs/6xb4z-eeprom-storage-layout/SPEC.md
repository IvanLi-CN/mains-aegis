# EEPROM storage layout (#6xb4z)

## 状态

- Status: active

## 背景 / 问题陈述

主固件已经把多类用户偏好与诊断信息写入板载 I2C EEPROM，包括手动充电偏好、USB-PD breadcrumb、WiFi 凭据和前面板蜂鸣器音量偏好。此前 EEPROM 布局只在功能 spec 中局部记录，容易在新增记录时出现地址冲突、schema 语义不一致或旧文档误导实现。

本规格作为 EEPROM 存储布局的长期真相源。任何固件 EEPROM 地址、record 编码、CRC、schema 或持久化边界变更，都必须先对齐本规格。

## 目标 / 非目标

### Goals

- 固定 Mains Aegis 主板 EEPROM 的全局地址 map。
- 规范 32-byte block 对齐、CRC、record magic/version 和默认值回退行为。
- 明确哪些状态允许跨复位持久化，哪些只能保存在运行时 RAM。
- 为新增 EEPROM record 提供扩展规则，避免复用或覆盖既有 slot。

### Non-goals

- 不规定外置 EEPROM 具体供应商型号的写周期寿命策略。
- 不把运行时事件日志扩展为长期审计日志；PD breadcrumb 只保留 bounded recovery 诊断。
- 不持久化手动充电会话、前面板临时 UI route、连接 session、devd lease 或 monitor 状态。

## EEPROM 设备

- Bus: firmware `I2C1`
- Address: `0x50`
- Logical block size: `32B`
- Known hardware capacity: `64 Kbit` / `8192B`
- Access owner: `firmware/src/output/mod.rs`
- Current low-level helpers:
  - `read_eeprom_block(i2c, offset)`
  - `write_eeprom_block(i2c, offset, [u8; 32])`

All single-block records must be aligned to `32B` offsets. Multi-block records must start at a `32B` boundary and consume whole blocks.

## Canonical Layout

| Offset range | Size | Owner / record | Encoding | Purpose |
| --- | ---: | --- | --- | --- |
| `0x0000..0x001f` | `32B` | `StorageSuperblockV1` | `magic="AEG1"`, schema byte, CRC8 | Global EEPROM layout marker for table-managed records |
| `0x0020..0x003f` | `32B` | `StorageRecordTableV1` | manual prefs record id/version/offset/size, CRC8 | Lookup table for the table-managed manual charge prefs record |
| `0x0040..0x005f` | `32B` | `ManualChargePrefsRecordV1` | version + enum bytes + CRC8 | Manual charge target/speed/timer user preferences |
| `0x0060..0x015f` | `8 * 32B` | `PdBreadcrumbRecordV1` ring | `magic="PDBG"`, version, seq, compact state fields, CRC8 | USB-PD recovery breadcrumbs across reset/log loss |
| `0x0160..0x01df` | `128B` | WiFi config record | USB CDC protocol WiFi config record | Plaintext SSID/PSK secret record for Web Serial / LAN bootstrap |
| `0x01e0..0x01ff` | `32B` | `BeeperPrefsRecordV1` | `magic="BEEP"`, version, action/system/selected bytes, CRC8 | Front panel ACTION/SYSTEM beeper volume preferences |
| `0x0200..0x1fff` | remaining | reserved | none | Future EEPROM records |

## Record Contracts

### StorageSuperblockV1

- Offset: `0x0000`
- Size: `32B`
- Fields:
  - bytes `0..4`: ASCII magic `AEG1`
  - byte `4`: `schema_version`
  - byte `5`: current table count marker
  - byte `31`: CRC8 over bytes `0..31`
- Current schema version: `1`
- If magic or CRC is invalid, table-managed records fall back to default values and the layout may be initialized.
- If a future schema version is newer than firmware supports, firmware must not overwrite table-managed records blindly.

### StorageRecordTableV1

- Offset: `0x0020`
- Size: `32B`
- Current scope: manual charge prefs only.
- Fields:
  - byte `0`: manual prefs record id, currently `1`
  - byte `1`: manual prefs record version, currently `1`
  - bytes `2..4`: little-endian manual prefs offset, currently `0x0040`
  - byte `4`: record size, currently `32`
  - byte `31`: CRC8 over bytes `0..31`
- New fixed-address records must not be inserted into this V1 table retroactively. If the table becomes multi-record, introduce an explicit table schema contract and migration behavior.

### ManualChargePrefsRecordV1

- Offset: currently `0x0040`
- Size: `32B`
- Fields:
  - byte `0`: record version, currently `1`
  - byte `1`: target (`0=Pack3V7`, `1=Rsoc80`, `2=Full100`)
  - byte `2`: speed (`0=100mA`, `1=500mA`, `2=1000mA`)
  - byte `3`: timer (`0=1h`, `1=2h`, `2=6h`)
  - byte `31`: CRC8 over bytes `0..31`
- Defaults on missing/invalid/incompatible data:
  - target: `Full100`
  - speed: `500mA`
  - timer: `2h`
- Only preferences are persistent. Manual charge active/takeover/deadline/stop-inhibit/last-stop-reason remain RAM-only.

### PdBreadcrumbRecordV1

- Offset range: `0x0060..0x015f`
- Slots: `8`
- Slot size: `32B`
- Fields include:
  - bytes `0..4`: ASCII magic `PDBG`
  - byte `4`: record version, currently `1`
  - sequence and compact USB-PD recovery state fields
  - byte `31`: CRC8 over bytes `0..31`
- Storage is a bounded ring. It is diagnostic state, not an append-only audit log.

### WiFi Config Record

- Offset range: `0x0160..0x01df`
- Size: `128B`
- Encoding is owned by `firmware/src/usb_cdc_protocol.rs`.
- Written as four `32B` EEPROM blocks.
- Contains plaintext WiFi credentials by current project decision.
- On missing/invalid/cleared record, firmware must keep WiFi disabled until new credentials are written.

### BeeperPrefsRecordV1

- Offset: `0x01e0`
- Size: `32B`
- Fields:
  - bytes `0..4`: ASCII magic `BEEP`
  - byte `4`: record version, currently `1`
  - byte `5`: ACTION volume step (`0=Off`, `1..6=L1..L6`)
  - byte `6`: SYSTEM volume step (`0=Off`, `1..6=L1..L6`)
  - byte `7`: selected target (`0=Action`, `1=System`)
  - byte `31`: CRC8 over bytes `0..31`
- Defaults on missing/invalid data:
  - ACTION volume: `L4`
  - SYSTEM volume: `L4`
  - selected target: `Action`
- Firmware must write this record only when `BeeperPrefs` changes, not on every preview at unchanged bounds.

## Extension Rules

- New EEPROM records must reserve a non-overlapping aligned range in this spec before firmware writes are added.
- New single-block records should include either:
  - a unique 4-byte magic + version + CRC8, or
  - a table-managed record id/version/offset/size entry with an explicit table migration plan.
- New multi-block records must define total byte length, per-block write behavior, integrity check location and default fallback.
- Persistent records must store user intent or bounded diagnostics only. Runtime session state must stay in RAM unless a separate spec justifies recovery semantics.
- EEPROM writes should be deduplicated when practical. Repeated UI actions that do not change the effective preference must not rewrite the same record.
- CRC failure must be treated as invalid data and fall back to safe defaults rather than partially accepting fields.

## Acceptance Criteria

- Given any firmware change touches `EEPROM_*_OFFSET`, record encoding, CRC, schema, or default fallback, Then this spec is updated in the same change set.
- Given a new EEPROM record is introduced, Then its byte range does not overlap any range in the canonical layout table.
- Given EEPROM contains missing or invalid manual charge prefs, Then firmware uses `Full100 / 500mA / 2h`.
- Given EEPROM contains missing or invalid beeper prefs, Then firmware uses `L4 / L4 / Action`.
- Given a user changes beeper ACTION or SYSTEM volume, Then firmware persists `BeeperPrefsRecordV1` and reads it back after reset/flash without returning to maximum volume.
- Given WiFi config is cleared, Then firmware wipes the WiFi config record and runtime WiFi returns to disabled.

## References

- `firmware/src/output/mod.rs`
- `firmware/src/usb_cdc_protocol.rs`
- `docs/specs/zp4cg-manual-charge-dashboard/SPEC.md`
- `docs/specs/ypfpu-web-management-ui/SPEC.md`
- `docs/specs/hn29u-usb-c-pd-sink-pps/SPEC.md`
