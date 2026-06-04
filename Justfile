set dotenv-load
set shell := ["bash", "-uc"]

host_manifest := "tools/mains-aegis-host/Cargo.toml"
artifact_out := "firmware/target/mains-aegis-artifacts"
firmware_elf := "firmware/target/xtensa-esp32s3-none-elf/release/esp-firmware"

# List available development commands.
default:
    @just --list

# Run TypeScript checks for the Web UI.
web-check:
    bun run web:check

# Start the Web dev server. Override with WEB_PORT=5173.
web-dev:
    cd web && WEB_PORT=${WEB_PORT:-5173} bun run dev

# Build the Web UI.
web-build:
    bun run web:build

# Start the local devd HTTP bridge. Override with MAINS_AEGIS_DEVD_BIND=127.0.0.1:30080.
devd-bridge:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis-devd -- bridge-http --bind ${MAINS_AEGIS_DEVD_BIND:-127.0.0.1:30080} --allow-dev-cors

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

# Build ESP firmware release ELF.
firmware-build:
    cd firmware && cargo +esp build --release

# Generate a devd/Web firmware artifact manifest for the current release ELF.
firmware-artifact:
    python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --out {{ artifact_out }} --firmware-dir firmware

# Copy the generated firmware catalog into the Web public assets.
firmware-embed-web:
    bun run firmware:embed-web

# Build firmware and generate a matching firmware artifact manifest.
firmware-release: firmware-build firmware-artifact firmware-embed-web

# Select an artifact manifest for a bound devd device.
artifact-select device manifest:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} artifact select --manifest-path {{ manifest }}

# Flash dry-run for an already-bound devd device.
flash-dry-run device:
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --dry-run

# Build, select, and dry-run flash for an already-bound devd device.
flash-current-dry-run device:
    just firmware-build
    manifest=$(python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --out {{ artifact_out }} --firmware-dir firmware)
    bun run firmware:embed-web
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} artifact select --manifest-path "$manifest"
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --dry-run

# Build, select, dry-run, and real-flash an already-bound devd device. Requires confirm=flash.
flash-current-real device confirm:
    [[ "{{ confirm }}" == "flash" ]] || { echo "Refusing real flash: pass confirm=flash"; exit 2; }
    just firmware-build
    manifest=$(python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --out {{ artifact_out }} --firmware-dir firmware)
    bun run firmware:embed-web
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} artifact select --manifest-path "$manifest"
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --dry-run
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- device {{ device }} flash --real

# Run the standard local validation set.
check: web-check host-test firmware-host-test firmware-check
    git diff --check
