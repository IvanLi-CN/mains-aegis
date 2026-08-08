# Firmware Catalog

Firmware Catalog is the shared firmware artifact contract for Mains Aegis Web Direct mode, `mains-aegis-devd`, local development builds, and GitHub Releases.

## Canonical schema

- Schema: `schemas/firmware-catalog.schema.json`
- Web bundled fallback: `web/public/firmware/firmware-catalog.json`
- Local generator: `tools/firmware-artifact/build-catalog-entry.py`

Each catalog has `schema_version=1` and an `artifacts` array. Each artifact describes one ESP32-S3 firmware build:

The `fault_recovery` object is authoritative for update recovery. Current single-image artifacts declare `mcu_watchdog=true`, `boot_health=true`, and `rollback_capable=false` with blocker `missing_rollback_bootloader_otadata_ota_slots`. Tooling must not offer candidate activation or describe manual reflashing as rollback until the catalog ships a rollback-enabled bootloader, `otadata`, and two OTA application slots as one verified bundle.

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

Use the repository `Justfile` as the canonical local entrypoint. It enters the
`firmware/` directory before building so Cargo reads
`firmware/.cargo/config.toml` and targets `xtensa-esp32s3-none-elf`.

Build only the release ELF:

```bash
just firmware-build
```

Build the release ELF, Web Serial image, Firmware Catalog, and Web bundled
fallback:

```bash
just firmware-release
```

The release flow writes:

- `mains-aegis-firmware.manifest.json` for the normal release, or a short semantic variant such as `mains-aegis-firmware-19v.manifest.json`
- `firmware-catalog.json`
- `SHA256SUMS`
- any referenced firmware files, including the browser-flashable `image` payload when `web_serial` is enabled

To embed one or more local builds into the Web app static assets:

```bash
bun run firmware:embed-web
```

This stages `web/public/firmware/firmware-catalog.json`, per-artifact
manifests, `SHA256SUMS`, and artifact files under
`web/public/firmware/<artifact_id>/`. The browser and `mains-aegis-devd`
static hosting then consume the same bundled fallback catalog during local
development and production preview. Bundled artifacts that advertise the
`web_serial` feature must include at least one `image` file with
`flash_address`; `bun run firmware:embed-web` now fails fast if that browser
flash payload is missing. CI validates the committed bundled fallback with
`tools/firmware-artifact/validate-web-bundle.py`, which checks manifest/catalog
consistency, staged file hashes/sizes, `SHA256SUMS`, and the required Web
Serial image presence without rebuilding a new current-HEAD artifact identity.
The firmware workflow then stages a separate generated bundle in a temporary
directory and validates that output independently, so CI covers both the
committed fallback tree and the PR's newly built artifact set.

If a bundled artifact and a GitHub Release artifact share the same `artifact_id`,
the Web App keeps the bundled copy and treats the release copy as a duplicate.
Because `artifact_id` includes `build_id`, dirty local builds and clean release
builds from the same commit do not mask each other.

## Matching and defmt policy

`mains-aegis-devd` compares connected device firmware identity with the selected artifact. Only exact `build_id`, build profile, and feature-set matches mark monitor output as `verified`; `git_sha` is provenance only because different profiles/features can share the same commit. All other cases remain `unverified` and must not be treated as a trusted defmt decode.

## GitHub Release flow

The firmware workflow builds release variants, generates manifests with this same schema, uploads them as workflow artifacts, and publishes the generated catalog plus artifact files to GitHub Releases on `push` to `main`. The Web App consumes catalogs rather than hard-coded artifact URLs.

The release job publishes a release tagged with the current commit SHA and uploads the full artifact bundle produced by `tools/firmware-artifact/build-catalog-entry.py`. The normal bundle contains `mains-aegis-firmware`, `mains-aegis-firmware.bin`, `mains-aegis-firmware.manifest.json`, `firmware-catalog.json`, and `SHA256SUMS`. The stable owner-facing file names do not replace `artifact_id` or `build_id`; those fields remain the unique machine identity inside the manifest. The Web App resolves the latest release through the GitHub Releases API and reads the `firmware-catalog.json` asset from that release.

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

## Development auto-discovery

During Vite development, the Web dev server serves a dynamic
`/firmware/firmware-catalog.json` instead of only the static file under
`web/public/firmware/`. The dev catalog is rebuilt on request from:

- bundled manifests under `web/public/firmware/`;
- generated manifests under `firmware/target/mains-aegis-artifacts/`;
- firmware identities and verified selected artifacts currently observed by
  `mains-aegis-devd`.

Generated artifact files are included only when the referenced files still
exist and their recorded `sha256` and `size` match. This prevents stale
manifests in `firmware/target/mains-aegis-artifacts/` from falsely matching a
newly overwritten `mains-aegis-firmware` file.

When a dev-only catalog entry needs `mains-aegis-devd` to read files from the
host filesystem, the dev server adds `devd_manifest_path`. The browser still
receives `/firmware/<artifact_id>/<file>` URLs, while devd receives a local
manifest with absolute file paths for dry-run, flash operations, and defmt
decode. If the artifact files are unavailable, the dev server may still expose
an observed fileless entry from the connected device identity so connection and
telemetry are not blocked by firmware catalog mismatch.
