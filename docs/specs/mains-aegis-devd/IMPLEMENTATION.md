# Mains Aegis Device Daemon 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成（v1 devd foundation）
- Created: 2026-05-02
- Last: 2026-07-20

## Migrated Delivery Record

## 实现状态

- `tools/mains-aegis-host`: v1 daemon/API/mock validation foundation，并提供 CLI、IPC 与显式 HTTP service。
- `web/`: hosted Connect 只显示 devd discovery；devd 列出的 USB 设备通过 lease/usb-http bridge 进入 Web，LAN 设备则在 Web 中直接落为硬件 HTTP record。
- `tools/mains-aegis-host`: 提供设备级 `diag-snapshot` 只读诊断 API，转发固件 USB CDC `get_diag_snapshot` 并在 session 中缓存结果。
- `tools/mains-aegis-host`: flash API 与 `flash completed` 事件已暴露 backend `status/stdout/stderr`，用于现场确认底层 `espflash` 执行结果。
- `tools/mains-aegis-host`: flash backend 有可配置超时，默认避免底层烧录进程在 HTTP 客户端超时或断开后无界悬挂。
- `tools/mains-aegis-host`: flash backend 使用 `watchdog-reset` after-operation，避免当前 ESP32-S3 USB CDC 样机 post-flash 被 DTR/RTS line reset 拉进 ROM download。
- `tools/mains-aegis-host`: native serial reset 使用 in-process DTR/RTS app-boot 复位，并保持 boot 释放线为实测 app-boot 电平；monitor attach 不隐式复位也不主动改写 DTR/RTS，且 monitor 已运行时 `/reset` 复用 monitor 线程持有的串口，避免外部 reset 进程和重枚举前 monitor fd 争抢同一串口。
- `firmware/src/net_contract.rs`: `diag-snapshot.charger` 已暴露 `vac2_adc_mv`，用于定位 BQ25792 AC2/DC IN 实际采样。
- `tools/mains-aegis-host`: 提供 host power control surface；低功耗运行、suspend、shutdown 默认 dry-run，真实动作受启动参数保护。
- `tools/mains-aegis-host`: native serial monitor 自动在 UPS BACKUP 进入/退出边沿触发 host profile 切换，并通过 USB CDC `set_host_power_profile` 同步当前 host profile 给固件 runtime overlay；LAN/mock transport 不参与该自动策略。
- `schemas/firmware-catalog.schema.json`: v1 catalog schema。
- `tools/firmware-artifact/build-catalog-entry.py`: local manifest/catalog generator。
- 固件 catalog 生成器对外固定输出 `mains-aegis-firmware`、`mains-aegis-firmware.bin` 与 `mains-aegis-firmware.manifest.json`；19V 等显式变体使用语义后缀，详细构建身份继续保存在 manifest 中。
- `web/src/api/*`: devd mode client contracts。
- `firmware/src/net_contract.rs`: firmware identity/status/power diagnostic JSON contract。
- `firmware/src/net_contract.rs`、`firmware/src/usb_cdc_protocol.rs` 与 `firmware/src/output/mod.rs`: status JSON 新增 `host.power_profile`，USB CDC 新增 `set_host_power_profile`，前面板 BACKUP status 页把 fresh profile 显示为 `SAVER/BAL/PERF`，否则显示 `POL --`。
- `firmware/src/net_contract.rs`: `diag-snapshot.bms` 已暴露 `op_status_raw_len`、`op_status_raw_bytes`、`emshut`、`pres`、`xdsg` 与 EMSHUT 退出配置字段，供 host 直接核对 BQ40 `OperationStatus()` raw payload 与恢复门禁。
- `firmware/src/net_contract.rs`: `diag-snapshot.bms` 已暴露 BQ40 AFE FET status/control/latch、logical `op_*` FET flags、SafetyAlert 派生位、discharge path contradiction 字段；`diag-snapshot.charger` 已暴露 BQ25792 `ctrl2`、`ctrl5`、`sfet_present` 与 `sdrv_ctrl`。
- `firmware/src/output/mod.rs`、`firmware/src/net.rs` 与 `firmware/src/usb_cdc_protocol.rs`: 提供受限 BMS 放电授权恢复链路，覆盖 USB CDC `recover_bms_discharge_authorization` 与设备本体 LAN HTTP `POST /api/v1/recovery/bms-discharge-authorization`，并让前面板自检恢复操作复用同一个固件恢复事务。
- `tools/mains-aegis-host`: 提供 devd HTTP `POST /api/v1/devices/{id}/recovery/bms-discharge-authorization`，native serial / LAN transport 返回固件原始裁决结果并刷新 status/diag cache。


## References

- `./SPEC.md`
- `./HISTORY.md`
