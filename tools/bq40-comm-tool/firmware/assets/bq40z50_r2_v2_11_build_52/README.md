# BQ40Z50 R2 v2.11 build52 recovery sections

## Purpose

These binary sections are used only by the tool firmware ROM-recovery path:

- `tools/bq40-comm-tool/bin/run.sh recover ...`
- `tools/bq40-comm-tool/firmware/src/output/mod.rs` (`include_bytes!` constants)

`diagnose` mode does not write these sections.

## Source bundle and extraction path

- Vendor bundle located locally at:
  - `downloads/bq40z50/R2-v2.11/bq40z50FirmwareBundle-2.11-windows-installer.exe`
- Embedded Cookfs payload extracted from that bundle to:
  - `downloads/bq40z50/R2-v2.11/bitrock_payload/default/programfiles/`
- Extracted vendor files used to derive these sections:
  - `4500_2_11-bq40z50R2.bqz`
  - `bq40z50R2_v2_11_build_52.srec`
  - `Manifest.html`
  - `License.htm`

## Section derivation

- `section1.bin`: `0x4000..0x5FFF`, padded with `0xFF`
- `section2.bin`: `0x100000..0x10DFFF`, rebased to ROM instruction-flash offset `0x0000`, padded with `0xFF`
- `section3_blk00.bin`: info block `0x140000..0x14001F`, but bytes `0x140000..0x140001` are masked to `0xFFFF`
- `section3_blk80.bin`: info block `0x140080..0x14009F`, padded with `0xFF`
- `section4_blk.bin`: info block word `0x140000..0x140001`

## SHA-256 manifest

- `section1.bin`
  - `d98c7c8cf9978cc9362085f715876db7bca298b4e5a60617e0e0a242e18f6064`
- `section2.bin`
  - `82a0c028aaff5b83bfdf29a02baadf93b218edefb0fe0032c49a99a2be0e31a3`
- `section3_blk00.bin`
  - `f6c6916d3b7d3fc1262c9312b4e0c5f51081426b8f0322238d6d2b6aab882b02`
- `section3_blk80.bin`
  - `4a22e44be2e6ab2c91453782c09609fae2a9805cafdd006659e20dc00030bfe8`
- `section4_blk.bin`
  - `bef2392ca2bbdf62f5a50914e589d250484cc8cb14406ad48e64db8bb4b4cce1`
- `section3_info.bin`
  - `d4ac27378c0f653e350dda7962b886361484e0f4058fc914bf83c73edeebeca5`
