# BQ40Z50 R5 v5.05 build96 recovery sections

## Purpose

These binary sections are used only by the tool firmware ROM-recovery path:

- `tools/bq40-comm-tool/bin/run.sh recover ...`
- `tools/bq40-comm-tool/firmware/src/output/mod.rs` (`include_bytes!` constants)

`diagnose` mode does not write these sections.

## Provenance anchor in this repository

- First introduced by commit:
  - `40fb1b82fae19f71d682f1cea369888c50cf55e5`
- Commit message:
  - `feat(tooling): add standalone bq40 communication toolchain`
- Commit date:
  - `2026-02-24T14:14:46+08:00`

This repository currently does not store a separate raw vendor package archive for these sections.
For audit and reproducibility, use this manifest together with commit history and file hashes below.

## SHA-256 manifest

- `section1.bin`
  - `bbffecad923114eb0658a07d002d73361bb9b3e37b367d3545fffde7d2137fe8`
- `section2.bin`
  - `6c8223faef101cffdd17d7c6dc58ce4cf682919b216a071c27d22714f3b921de`
- `section3_blk00.bin`
  - `5c529d4c5646d624a30b7e1d1f880642f5a241fe67ea534a1d632b66f5143f09`
- `section3_blk80.bin`
  - `b63b8dc8bb4a4f6d3111825942f4cf331d60e115a44c466043391a8971892b07`
- `section4_blk.bin`
  - `bef2392ca2bbdf62f5a50914e589d250484cc8cb14406ad48e64db8bb4b4cce1`

Verification command:

```bash
cd tools/bq40-comm-tool/firmware/assets/bq40z50_r5_v5_05_build_96
shasum -a 256 section1.bin section2.bin section3_blk00.bin section3_blk80.bin section4_blk.bin
```

## Related external references

- `tools/bq40-comm-tool/docs/references.md`
