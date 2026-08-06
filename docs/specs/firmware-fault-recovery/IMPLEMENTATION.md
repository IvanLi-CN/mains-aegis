# Implementation

## Current coverage

- TIMG0 MWDT provides 8-second system liveness recovery and is fed only after a completed critical-loop slice.
- RTC slow persistent double-buffered records provide versioning, CRC, reset classification, boot-loop counting, stable-boot confirmation, and safe-mode state.
- Repeated abnormal early boots hold controllable outputs and charger policy off and render a dedicated front-panel recovery surface.
- Safe mode requests no outputs during self-test and best-effort disables both TPS channels at the earliest I2C-safe point before self-test; PowerManager applies the same clamp again after construction.
- `status` and `mcu.runtime` diagnostics expose reset, boot health, safe mode, candidate state, and rollback capability/blocker.
- Candidate confirmation/failed-boot transitions are host-tested, but activation remains prohibited until the release pipeline ships a rollback-enabled bootloader and dual-slot partition bundle.
- The compile-time `hil-watchdog-stall` profile provides deterministic real-device watchdog and repeated-abnormal-boot injection. It is excluded from normal artifacts and automatically stops injecting once safe mode is active.
- The compile-time `hil-clear-boot-health` cleanup profile clears retained HIL safe-mode state before restoring the normal release image; it is not a production recovery or rollback mechanism.
- Cleanup preserves `candidate_state=unsupported_layout`; clearing retained HIL state cannot claim candidate confirmation on the single-image layout.
- The devd compact status includes reset cause, boot phase, abnormal count, safe-mode state, candidate state, and rollback capability. Its host test parses the rendered body as JSON so a trailing field delimiter cannot regress into a devd `native_cdc_timeout`.
- Alternating-slot recovery is tested against every possible torn-write prefix, proving selection falls back to the previous CRC-valid record.

## Hardware validation

- Device `mains-aegis-198840` on the owner-specified `/dev/cu.usbmodem21141401` was flashed only through the bound devd path.
- Normal release `f8ace5f5-clean-9e88121a12918e22` returned a fresh native-CDC status with `reset_cause=power_on`, `abnormal_boots=0`, `safe_mode=false`, and live input/output measurements.
- The matching `hil-watchdog-stall` image produced three consecutive runtime-watchdog resets and then returned `reset_cause=watchdog`, `abnormal_boots=3`, `phase=safe_mode`, `output.active=none`, both output enables false, and `charger.allow_charge=false`.
- The matching `hil-clear-boot-health` image cleared retained test state. The normal release was restored, its exact build id and feature set were verified, fresh status again reported `abnormal_boots=0` and `safe_mode=false`, and the devd session was disconnected.

## Architecture blocker

The current artifact is one application image flashed at `0x10000`. It contains no partition table, OTA data partition, alternate application slot, or project-built ESP-IDF bootloader with `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE`. The `esp-bootloader-esp-idf` application crate can manipulate OTA metadata, but cannot add rollback semantics to the bootloader already installed on a device. End-to-end failed-boot rollback therefore cannot truthfully be enabled on this baseline.
