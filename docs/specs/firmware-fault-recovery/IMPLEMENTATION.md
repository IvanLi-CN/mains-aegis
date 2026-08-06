# Implementation

## Current coverage

- TIMG0 MWDT provides 8-second system liveness recovery and is fed only after a completed critical-loop slice.
- RTC slow persistent double-buffered records provide versioning, CRC, reset classification, boot-loop counting, stable-boot confirmation, and safe-mode state.
- Repeated abnormal early boots hold controllable outputs and charger policy off and render a dedicated front-panel recovery surface.
- `status` and `mcu.runtime` diagnostics expose reset, boot health, safe mode, candidate state, and rollback capability/blocker.
- Candidate confirmation/failed-boot transitions are host-tested, but activation remains prohibited until the release pipeline ships a rollback-enabled bootloader and dual-slot partition bundle.
- The compile-time `hil-watchdog-stall` profile provides deterministic real-device watchdog and repeated-abnormal-boot injection. It is excluded from normal artifacts and automatically stops injecting once safe mode is active.
- The compile-time `hil-clear-boot-health` cleanup profile clears retained HIL safe-mode state before restoring the normal release image; it is not a production recovery or rollback mechanism.
- The devd compact status includes reset cause, boot phase, abnormal count, safe-mode state, candidate state, and rollback capability. Its host test parses the rendered body as JSON so a trailing field delimiter cannot regress into a devd `native_cdc_timeout`.

## Architecture blocker

The current artifact is one application image flashed at `0x10000`. It contains no partition table, OTA data partition, alternate application slot, or project-built ESP-IDF bootloader with `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE`. The `esp-bootloader-esp-idf` application crate can manipulate OTA metadata, but cannot add rollback semantics to the bootloader already installed on a device. End-to-end failed-boot rollback therefore cannot truthfully be enabled on this baseline.
