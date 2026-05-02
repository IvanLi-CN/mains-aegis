# Firmware Catalog

Firmware Catalog is the shared firmware artifact contract for Mains Aegis Web Direct mode, `mains-aegis-devd`, local development builds, and GitHub Releases.

## Canonical schema

- Schema: `schemas/firmware-catalog.schema.json`
- Web bundled fallback: `web/public/firmware/firmware-catalog.json`
- Local generator: `tools/firmware-artifact/build-catalog-entry.py`

Each catalog has `schema_version=1` and an `artifacts` array. Each artifact describes one ESP32-S3 firmware build:

- `artifact_id`: stable catalog identifier.
- `git_sha`, `git_dirty`, `build_id`: provenance used for device identity matching.
- `target_chip`: always `esp32s3`.
- `profile`: `debug`, `release`, or `dev`.
- `features`: firmware feature list such as `web_serial` or `main-vout-19v`.
- `protocol`: currently `mains-aegis.cdc.v1`.
- `defmt`: decoding metadata. Logs are verified only when identity matches this artifact.
- `files`: local or published files with `kind`, `path`, `sha256`, and `size`.

## Local build flow

```bash
cd firmware
cargo build --release --bin esp-firmware --features web_serial
cd ..
python3 tools/firmware-artifact/build-catalog-entry.py \
  --elf firmware/target/xtensa-esp32s3-none-elf/release/esp-firmware \
  --out firmware/target/mains-aegis-artifacts \
  --features web_serial \
  --profile release
```

The generator writes:

- `<artifact_id>.manifest.json`
- `firmware-catalog.json`
- `SHA256SUMS`

## Matching and defmt policy

`mains-aegis-devd` compares connected device firmware identity with the selected artifact. Only exact `build_id`, build profile, and feature-set matches mark monitor output as `verified`; `git_sha` is provenance only because different profiles/features can share the same commit. All other cases remain `unverified` and must not be treated as a trusted defmt decode.

## GitHub Release flow

The firmware workflow builds release variants, generates manifests with this same schema, uploads them as workflow artifacts, and can publish the catalog plus artifact files to GitHub Releases. The Web App consumes catalogs rather than hard-coded artifact URLs.
