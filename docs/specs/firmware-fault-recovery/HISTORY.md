# History

## 2026-08-06

- Established system-level MCU liveness, boot-loop safe-mode, and update rollback contracts.
- Selected TIMG0 MWDT because `esp_rtos::start` consumes TIMG0 timer0 while the watchdog remains independently owned.
- Selected RTC slow-domain double-buffered state for reset-loop accounting to avoid EEPROM wear.
- Recorded the existing single-image release layout as an explicit rollback architecture blocker rather than treating manual reflashing as rollback.
