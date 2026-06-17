set dotenv-load
set shell := ["bash", "-uc"]

host_manifest := "tools/mains-aegis-host/Cargo.toml"
artifact_out := "firmware/target/mains-aegis-artifacts"
firmware_elf := "firmware/target/xtensa-esp32s3-none-elf/release/esp-firmware"
firmware_image := "firmware/target/xtensa-esp32s3-none-elf/release/esp-firmware.bin"

# List available development commands.
default:
    @just --list

# Run TypeScript checks for the Web UI.
web-check:
    bun run web:check

# Sync the generated firmware catalog into Web assets when one exists.
firmware-sync-web:
    if [[ -f {{ artifact_out }}/firmware-catalog.json ]]; then bun run firmware:embed-web; else echo "No generated firmware catalog at {{ artifact_out }}/firmware-catalog.json; skipping Web firmware sync."; fi

# Start the Web dev server after syncing any generated firmware catalog. Override with WEB_PORT=5173.
web-dev: firmware-sync-web
    cd web && WEB_PORT=${WEB_PORT:-5173} bun run dev

# Build the Web UI.
web-build:
    bun run web:build

# Start the local devd HTTP service in API-only development mode. Override with MAINS_AEGIS_DEVD_BIND=127.0.0.1:30080.
devd-http:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis-devd -- serve-http --bind ${MAINS_AEGIS_DEVD_BIND:-127.0.0.1:30080} --allow-dev-cors

# Start the local devd IPC daemon.
devd-serve:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis-devd -- serve

# Run the host CLI, for example: just cli devices list
cli *args:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- {{ args }}

# List currently known devd devices.
devices-list:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- devices list

# Run owner-visible USB candidate scan through devd.
devices-scan-usb:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- devices scan --no-lan

# Connect an already-known devd device.
device-connect device:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} connect

# Read identity for an already-known devd device.
device-identity device:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} identity

# Run host tool tests.
host-test:
    cargo test --manifest-path {{ host_manifest }}

# Run firmware host-side unit tests.
firmware-host-test:
    cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol

# Check ESP firmware.
firmware-check:
    cd firmware && cargo +esp check

# Build ESP firmware release ELF for the default Web/devd feature set.
firmware-build:
    cd firmware && cargo +esp build --release --bin esp-firmware --features net_http,web_serial

# Build the Web Serial flash image from the current release ELF.
firmware-web-image: firmware-build
    python3 -m esptool --chip esp32s3 elf2image --flash-mode dio --flash-freq 80m --flash-size 4MB --output {{ firmware_image }} {{ firmware_elf }}

# Generate a devd/Web firmware artifact manifest for the current release ELF and Web Serial image.
firmware-artifact: firmware-web-image
    python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial --profile release

# Copy the generated firmware catalog into the Web public assets.
firmware-embed-web:
    bun run firmware:embed-web

# Build firmware and generate a matching firmware artifact manifest.
firmware-release: firmware-artifact firmware-embed-web

# Select an artifact manifest for a bound devd device.
artifact-select device manifest:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} artifact select --manifest-path {{ manifest }}

# Flash dry-run for an already-bound devd device.
flash-dry-run device:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --dry-run

# Build, select, and dry-run flash for an already-bound devd device.
flash-current-dry-run device:
    just firmware-web-image
    manifest=$(python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial --profile release)
    bun run firmware:embed-web
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} artifact select --manifest-path "$manifest"
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --dry-run

# Build, select, dry-run, and real-flash an already-bound devd device. Requires confirm=flash.
flash-current-real device confirm:
    [[ "{{ confirm }}" == "flash" ]] || { echo "Refusing real flash: pass confirm=flash"; exit 2; }
    just firmware-web-image
    manifest=$(python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial --profile release)
    bun run firmware:embed-web
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} artifact select --manifest-path "$manifest"
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --dry-run
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --real

# Run the standard local validation set.
check: web-check host-test firmware-host-test firmware-check
    git diff --check
