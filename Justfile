set dotenv-load
set shell := ["bash", "-uc"]

host_manifest := "tools/mains-aegis-host/Cargo.toml"
devd_ipc := ".tmp/devd.sock"
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

# Build both host-tool binaries so CLI auto-start can find its sibling devd.
host-tools-build:
    cargo build --manifest-path {{ host_manifest }} --bins

# Start the local devd HTTP service in API-only development mode. Override with MAINS_AEGIS_DEVD_BIND=127.0.0.1:30080.
devd-http: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} daemon http --bind ${MAINS_AEGIS_DEVD_BIND:-127.0.0.1:30080} --allow-dev-cors

# Start the local devd IPC daemon in the foreground for development/debugging.
devd-serve: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} daemon serve --idle-timeout-secs 0

# Run the host CLI, for example: just cli devices list
cli *args: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} {{ args }}

# Run the Power Path Validation CLI, for example:
# just power-validation run --dry-run --load-cli /path/to/loadlynx
power-validation *args: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} power-validation {{ args }}

# Generate a dry-run Power Path Validation suite plan without touching hardware.
power-validation-plan *args: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} power-validation run --dry-run {{ args }}

# Run the read-only diag-snapshot HIL gate. Example:
# just hil-diag-snapshot --devd-url http://127.0.0.1:30080 --device-id <device>
hil-diag-snapshot *args:
    python3 tools/hil/diag_snapshot_readonly.py {{ args }}

# List currently known devd devices.
devices-list: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} devices list

# Run owner-visible USB candidate scan through devd.
devices-scan-usb: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} devices scan --no-lan

# Connect an already-known devd device.
device-connect device: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} connect

# Read identity for an already-known devd device.
device-identity device: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} identity

# Run host tool tests.
host-test:
    cargo test --manifest-path {{ host_manifest }}

# Run firmware host-side unit tests.
firmware-host-test:
    cargo test --manifest-path firmware/host-unit-tests/Cargo.toml

# Check ESP firmware.
firmware-check:
    cd firmware && cargo +esp check

# Build ESP firmware release ELF for the default Web/devd feature set.
firmware-build:
    cd firmware && cargo +esp build --release --bin esp-firmware --features net_http,web_serial

# Build ESP firmware for Power Path Validation telemetry.
firmware-build-hil:
    # Keep USB CDC reserved for IPC frames; warnings can starve validation sampling.
    cd firmware && DEFMT_LOG=error cargo +esp build --release --bin esp-firmware --features net_http,web_serial

# Build the explicit 19V Power Path Validation firmware variant.
firmware-build-hil-19v:
    # Keep USB CDC reserved for IPC frames; warnings can starve validation sampling.
    cd firmware && DEFMT_LOG=error cargo +esp build --release --bin esp-firmware --features net_http,web_serial,main-vout-19v

# Build the deterministic watchdog-stall HIL firmware. This profile never belongs in release artifacts.
firmware-build-watchdog-hil:
    cd firmware && DEFMT_LOG=error cargo +esp build --release --bin esp-firmware --features net_http,web_serial,hil-watchdog-stall

# Build the one-shot retained boot-health cleanup image used after watchdog HIL validation.
firmware-build-boot-health-cleanup-hil:
    cd firmware && DEFMT_LOG=error cargo +esp build --release --bin esp-firmware --features net_http,web_serial,hil-clear-boot-health

# Build the Web Serial flash image from the current release ELF.
firmware-web-image: firmware-build
    python3 -m esptool --chip esp32s3 elf2image --flash-mode dio --flash-freq 80m --flash-size 4MB --output {{ firmware_image }} {{ firmware_elf }}

# Build the Web Serial flash image for Power Path Validation telemetry.
firmware-web-image-hil: firmware-build-hil
    python3 -m esptool --chip esp32s3 elf2image --flash-mode dio --flash-freq 80m --flash-size 4MB --output {{ firmware_image }} {{ firmware_elf }}

# Build a Web Serial flash image for the explicit 19V HIL variant.
firmware-web-image-hil-19v: firmware-build-hil-19v
    python3 -m esptool --chip esp32s3 elf2image --flash-mode dio --flash-freq 80m --flash-size 4MB --output {{ firmware_image }} {{ firmware_elf }}

# Generate a devd/Web firmware artifact manifest for the current release ELF and Web Serial image.
firmware-artifact: firmware-web-image
    python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial --profile release

# Generate a devd artifact manifest for the explicit 19V HIL firmware variant.
firmware-artifact-hil-19v: firmware-web-image-hil-19v
    python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial,main-vout-19v --profile release

# Copy the generated firmware catalog into the Web public assets.
firmware-embed-web:
    bun run firmware:embed-web

# Build firmware and generate a matching firmware artifact manifest.
firmware-release: firmware-artifact firmware-embed-web

# Select an artifact manifest for a bound devd device.
artifact-select device manifest: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} artifact select --manifest-path {{ manifest }}

# Flash dry-run for an already-bound devd device.
flash-dry-run device: host-tools-build
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} flash --dry-run

# Build, select, and dry-run flash for an already-bound devd device.
flash-current-dry-run device: host-tools-build
    just firmware-web-image
    manifest=$(python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial --profile release) && \
    bun run firmware:embed-web && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} artifact select --manifest-path "$manifest" && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} flash --dry-run

# Build, select, dry-run, and real-flash an already-bound devd device. Requires confirm=flash.
flash-current-real device confirm: host-tools-build
    [[ "{{ confirm }}" == "flash" ]] || { echo "Refusing real flash: pass confirm=flash"; exit 2; }
    just firmware-web-image
    manifest=$(python3 tools/firmware-artifact/build-catalog-entry.py --elf {{ firmware_elf }} --image 0x10000:{{ firmware_image }} --out {{ artifact_out }} --firmware-dir firmware --features net_http,web_serial --profile release) && \
    bun run firmware:embed-web && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} artifact select --manifest-path "$manifest" && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} flash --dry-run && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} flash --real

# Build, select, dry-run, and real-flash the explicit 19V HIL artifact. Requires confirm=flash.
flash-current-real-hil-19v device confirm: host-tools-build
    [[ "{{ confirm }}" == "flash" ]] || { echo "Refusing real flash: pass confirm=flash"; exit 2; }
    just firmware-artifact-hil-19v
    manifest=$(ls -t {{ artifact_out }}/*main-vout-19v*.manifest.json | head -n 1) && \
    test -n "$manifest" && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} artifact select --manifest-path "$manifest" && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} flash --dry-run && \
    cargo run --manifest-path {{ host_manifest }} --bin mains-aegis -- --ipc {{ devd_ipc }} device {{ device }} flash --real

# Run the standard local validation set.
check: web-check host-test firmware-host-test firmware-check
    git diff --check
