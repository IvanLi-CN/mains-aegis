# bq40-comm-tool docs

- `operations.md`: end-to-end runbook (diagnose/recover/verify)
- `operations.md`: includes the maintenance-only `bq40-charge-temperature` Data Flash profile
- `log-signatures.md`: log keyword to meaning mapping
- `recovery-safety.md`: ROM recovery safety policy
- `troubleshooting-notes.md`: troubleshooting history and convergence strategy
- `references.md`: external references + recovery image traceability index
- `../firmware/assets/bq40z50_r2_v2_11_build_52/README.md`: active ROM recovery image hash manifest
- `../firmware/assets/bq40z50_r5_v5_05_build_96/README.md`: previous R5 ROM recovery image hash manifest

## Charge-temperature profile

`bq40-charge-temperature` is the only supported BQ40 Data Flash write profile for the
charge-temperature policy. It writes exactly seven temperature fields from the locked
profile and does not write ChemID, calibration, protection, termination, precharge, or
maintenance fields.

Before any field write, the tool saves the complete containing block at `0x4A00` with
length `32`. Each MB44 write must pass address, length, raw-vector, and decoded-value
readback checks; the first error stops the operation. The normal firmware never performs
this write automatically.

The complete pre-write and post-write blocks are emitted as machine-readable
`stage=snapshot_before_hex` and `stage=final_after_hex` monitor events. The seven
field verification records are emitted as `stage=field_verify`, and the report parser
requires the ordered `CE high/charger disabled`, dual identity, Full Access, complete
blocks, and all seven field records before writing `verdict.pass=true`. `summary.json`
persists the blocks, gate evidence, and verified field names under
`charge_temperature_apply`. The profile image is canonical-only and cannot be combined
with ROM recovery, non-R2 ROM images, AFE repair, asset repair, `dual-diag`, or `mac-only`; it
is accepted by offline verification only for `apply-df` or `verify` actions, and it also requires
the existing strict BQ40 identity probe with both the Texas manufacturer and BQ40
device-name evidence before the writer image is built or run. The writer records the actual
per-write MB44 echo address, lengths, raw bytes, and decoded value. Offline `report.sh` parsing
also enforces the writer-emitted `strict`/`r2` provenance for `apply-df`; `verify` accepts no
live-run provenance flags, rejects ROM/recovery evidence, and validates the Full Access
operation-status bits.

The writer is armed only by the explicit `apply-df` maintenance image and its
`bms-charge-temperature-apply` feature. Normal BQ40 discovery and polling share the profile
implementation but cannot enter the Data Flash writer.

Low-level `bin/build.sh` calls must pass `--action apply-df` together with this profile before
the writer feature is compiled. The supported `bin/run.sh apply-df` path supplies that action;
diagnostic and recovery actions build without the writer feature.

The former `live-df-mainboard` profile is retired and returns a migration error. ROM,
AFE, and `asset-df-mainboard` recovery paths retain their existing semantics and are not
aliases for this profile.

After an authorized hardware apply, retain the machine-readable `summary.json` evidence with
`writes=7`, `identity_verified=true`, `full_access_verified=true`, one 32-byte pre-write block,
one 32-byte post-write block, and all seven verified field names. Reflash canonical firmware
after the maintenance image and confirm the bound device identity before any runtime HIL.

## 独立充电终止 profile

`bq40-charge-termination` 与温度 profile 分离。唯一字段为 `Charge Term Taper Current`（`0x4A6D`，两字节 little-endian `78 00`，120mA），包含块为 `0x4A60`、32 字节。温度 profile 仍只有原七字段。恢复 `asset-df-mainboard` overlay 使用同一 taper，原厂二进制不变。构建、应用和验证步骤见 [operations.md](operations.md#充电终止配置)。
