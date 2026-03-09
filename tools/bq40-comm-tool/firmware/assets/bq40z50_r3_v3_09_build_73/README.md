# BQ40Z50 R3 v3.09 build73 recovery sections

## Purpose

These binary sections are used only by the tool firmware ROM-recovery path.

## Source bundle

- Source SREC located locally at:
  - `downloads/bq40z50/bq40z50R3_v3_09_build73.srec`

## Section derivation

- `section1.bin`: `0x4000..0x5FFF`, padded with `0xFF`
- `section2.bin`: `0x100000..0x10DFFF`, rebased to ROM instruction-flash offset `0x0000`, padded with `0xFF`
- `section3_blk00.bin`: info block `0x140000..0x14001F`, but bytes `0x140000..0x140001` are masked to `0xFFFF`
- `section3_blk80.bin`: info block `0x140080..0x14009F`
- `section4_blk.bin`: info block word `0x140000..0x140001`
- `section3_info.bin`: sparse `0x140000..0x1400FF` info window for diagnostics

## SHA-256 manifest
- `section1.bin`
  - `ee8ed4b21397f7a9cab8e2413d24005e3ee198e45e127c283c12507745577dff`
- `section2.bin`
  - `20e475b4b965f3c37cbd9e0f64f4287f6498c45bdacf85a43941c9bc2a05deda`
- `section3_blk00.bin`
  - `46bd2313d42191eddfcb8c156c83951b20815c0da327508d0a64e98fde36eea5`
- `section3_blk80.bin`
  - `847ecc029802557f398cdb2f2e8895eca07543d51f6e15abd256231e8c0fcb3b`
- `section4_blk.bin`
  - `bef2392ca2bbdf62f5a50914e589d250484cc8cb14406ad48e64db8bb4b4cce1`
- `section3_info.bin`
  - `4d2f1961fa807849bf23d5b6ca797825e344401d0b6c308d91d7bc7dd2ebe233`
