# Firmware Catalog

Firmware Catalog is the shared firmware artifact contract for Mains Aegis Web Direct mode, `mains-aegis-devd`, local development builds, and GitHub Releases.

## Canonical schema

- Schema: `schemas/firmware-catalog.schema.json`
- Web bundled fallback: `web/public/firmware/firmware-catalog.json`
- Local generator: `tools/firmware-artifact/build-catalog-entry.py`

Each catalog has `schema_version=1` and an `artifacts` array. Each artifact describes one ESP32-S3 firmware build:

- `artifact_id`: stable catalog identifier derived from name, chip, profile, features, and `build_id`.
- `git_sha`, `git_dirty`, `build_id`: provenance used for device identity matching.
- `target_chip`: always `esp32s3`.
- `profile`: `debug`, `release`, or `dev`.
- `features`: firmware feature list such as `net_http`, `web_serial`, or `main-vout-19v`.
- `protocol`: currently `mains-aegis.cdc.v1`.
- `defmt`: decoding metadata. Logs are verified only when identity matches this artifact.
- `files`: local or published files with `kind`, `path`, `sha256`, and `size`.
- `image` files may also carry `flash_address` for browser-side Web Serial flashing.

## Local build flow

```bash
cd firmware
cargo build --release --bin esp-firmware
cd ..
python3 tools/firmware-artifact/build-catalog-entry.py \
  --elf firmware/target/xtensa-esp32s3-none-elf/release/esp-firmware \
  --out firmware/target/mains-aegis-artifacts \
  --features net_http,web_serial \
  --profile release
```

The generator writes:

- `<artifact_id>.manifest.json`
- `firmware-catalog.json`
- `SHA256SUMS`

To embed one or more local builds into the Web app static assets:

```bash
bun run firmware:embed-web
```

This stages `web/public/firmware/firmware-catalog.json`, per-artifact
manifests, `SHA256SUMS`, and artifact files under
`web/public/firmware/<artifact_id>/`. The browser and `mains-aegis-devd`
static hosting then consume the same bundled fallback catalog during local
development and production preview.

If a bundled artifact and a GitHub Release artifact share the same `artifact_id`,
the Web App keeps the bundled copy and treats the release copy as a duplicate.
Because `artifact_id` includes `build_id`, dirty local builds and clean release
builds from the same commit do not mask each other.

## Matching and defmt policy

`mains-aegis-devd` compares connected device firmware identity with the selected artifact. Only exact `build_id`, build profile, and feature-set matches mark monitor output as `verified`; `git_sha` is provenance only because different profiles/features can share the same commit. All other cases remain `unverified` and must not be treated as a trusted defmt decode.

## GitHub Release flow

The firmware workflow builds release variants, generates manifests with this same schema, uploads them as workflow artifacts, and publishes the generated catalog plus artifact files to GitHub Releases on `push` to `main`. The Web App consumes catalogs rather than hard-coded artifact URLs.

The release job publishes a release tagged with the current commit SHA and uploads the full artifact bundle produced by `tools/firmware-artifact/build-catalog-entry.py`, including `firmware-catalog.json`, `SHA256SUMS`, the manifest, and the firmware file(s) referenced by the catalog. The Web App resolves the latest release through the GitHub Releases API and reads the `firmware-catalog.json` asset from that release.

The Web-serialable image is produced from the release ELF with Espressif `elf2image` and is recorded at flash address `0x10000`. That is the `image` file the browser fetches and writes during Web Serial flashing.

## Browser lookup policy

When the Web App connects to a device, it merges the bundled catalog under `web/public/firmware/firmware-catalog.json` with the configured GitHub Release catalog. Bundled entries win on duplicate `artifact_id`.

Default GitHub catalog reference:

```text
github-release:IvanLi-CN/mains-aegis
```

The browser resolves that reference through the GitHub Releases API and reads the `firmware-catalog.json` asset with an asset API request. This avoids the CORS failure seen when fetching `https://github.com/.../releases/latest/download/...` directly from a browser. You can override it with `VITE_FIRMWARE_CATALOG_URL` during local development, including a direct CORS-safe JSON URL. The browser matches the connected device identity against the merged catalog, and only then selects the artifact for defmt decoding or flash flows. If the remote catalog is unavailable, the bundled catalog still keeps the app usable offline.

Web Serial flashing only accepts `image` files with `flash_address`. The browser fetches those static assets, verifies `sha256`, and then writes the address/data pairs through the Web Serial ROM loader.

`mains-aegis-devd` flashing reads firmware files from the daemon host filesystem. The Web UI therefore enables devd flashing only for bundled artifacts staged under `web/public/firmware/`; GitHub Release-only artifacts remain available to Web Serial when they include flash images.
