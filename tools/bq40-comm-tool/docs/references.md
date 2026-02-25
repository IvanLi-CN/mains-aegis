# External References

## ROM recovery image traceability

- Asset directory:
  - `tools/bq40-comm-tool/firmware/assets/bq40z50_r5_v5_05_build_96/`
- First repository commit that introduced current files:
  - `40fb1b82fae19f71d682f1cea369888c50cf55e5`
  - commit message: `feat(tooling): add standalone bq40 communication toolchain`
  - commit date: `2026-02-24T14:14:46+08:00`
- Runtime reference (where these files are consumed):
  - `tools/bq40-comm-tool/firmware/src/output/mod.rs`
- Integrity manifest:
  - `tools/bq40-comm-tool/firmware/assets/bq40z50_r5_v5_05_build_96/README.md`

> Note: this repo currently keeps the recovery binary sections only (no original vendor package archive in-tree).
> Traceability anchor is therefore: `commit -> file hashes -> runtime reference -> external protocol references`.

## TI E2E protocol references

- TI E2E: BQ40Z50 firmware update issue
  - https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/964264/bq40z50-firmware-update-problem
- TI E2E: BQ40Z50-R2 ROM mode to FW mode
  - https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1591571/bq40z50-r2-rom-mode-to-fw-mode
- TI E2E: BQ40z50 SREC programming sequence
  - https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/446520/bq40z50-srec-programming
