# ESP32-S3-FH4R2 (ESP32-S3 series) datasheet

Chosen main MCU/SoC variant for this project: **ESP32-S3-FH4R2** (in-package **4 MB flash + 2 MB PSRAM**, Quad SPI).

## Source

- Main landing page (official): https://documentation.espressif.com/esp32-s3_datasheet_en.pdf
- Direct PDF used for the conversion (may change if Espressif updates their portal): https://documentation.espressif.com/api/resource/doc/file/rz94aWY3/FILE/esp32-s3_datasheet_en.pdf

## Markdown conversion

This repo’s convention is to convert vendor documents to Markdown using **MinerU** and keep any extracted images in a local `images/` folder for offline rendering.

Because the MinerU MCP tool has a per-call time limit, this datasheet was converted in multiple `page_ranges` chunks and then concatenated.

Converted with MinerU MCP tool output (kept as-is):

- `esp32-s3-fh4r2.md`
- `images/`
