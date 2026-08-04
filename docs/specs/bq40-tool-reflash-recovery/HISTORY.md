# BQ40 工具链 reflash / recovery 收敛 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-06: 初始化规格，冻结工具路径边界、bench 前提、`if-rom` 验收口径与里程碑。
- 2026-03-06: 已完成工具侧 `--force-min-charge` / `flash_done` 语义修复，并新增 `--probe-mode mac-only`、missing reprobe、以及按地址细化的 `bms_diag_word` 诊断；最新实板证据表明 `0x0B` 只剩裸读 `0xFF` 伪应答、`0x16` 完全 NACK，仍属阻断态。
- 2026-03-06: 新增 boot 后 `0/800/1600 ms` staged wake probe，并在 `if-rom` 路径上复测；结果表明即使在早期唤醒窗口内，`0x0B` 依旧命令字节 NACK、`0x16` 依旧地址 NACK，ROM 恢复仍未触发。
- 2026-03-07: 在 `probe_rom_exit` 失败时追加 `0x0F00` / `0x0033`（含 PEC）盲打 ROM 入口诊断，并为 `monitor` 增加首轮 reset 失败自动回退；结果显示 ROM 入口写法在 `0x0B` 上全部 data-NACK、在 `0x16` 上全部 address-NACK，仍无法进入可见 ROM。
- 2026-03-09: 在工具固件日志中补充 `CellVoltage1..4()` 诊断；无电池偏置样本显示 `CellVoltage1≈27~51 mV`、`CellVoltage2..4=0 mV`，与 `Voltage()` 的几十 mV 浮动一致，可作为悬空偏置签名。结合更换芯片后的对照结果，本任务的软件收口口径调整为“工具链有效并可识别原始样本疑似硬损坏”，而不是要求软件恢复已损坏芯片。
- 2026-03-11: 追加真机闭环记录：`reports/20260311_112932/summary.json` 先确认当前样本已可见 `rom_events.detected=true`；随后 `reports/20260311_114111/summary.json` 显示 `flash_attempted=true`、`flash_image_done=true`、`flash_done=false`，并在 monitor 中持续出现 `stage=rom_post_flash_resume_observe rsoc=0x9002` / `stage=probe_rom_post_flash_still_rom`；最后 `reports/20260311_114419/summary.json` 与 `reports/20260311_114513/summary.json` 证明 post-recover canonical diagnose 与 offline verify 仍然没有有效样本，因此本轮 bench 结论仍是“镜像写入已发生，但退出 ROM/恢复应用态通信未被证实”。
- 2026-03-11: 在 Codex 桌面环境补记一条操作经验：若 `mcu-managerd start` 无法维持 IPC，可改用前台 `mcu-managerd run`，再执行 `mcu-agentd --non-interactive start` 后继续 live bench。
- 2026-03-11: 针对 `post_flash_still_rom` 再补一次 recover 回归后，monitor 新增 `probe_rom_post_flash_reexit_*` 证据，确认再次发送 ROM-exit（`0x08`）后 `RSOC` 可从 `0x9002` 回到 `0x0000`；因此当前主阻断已从“退出 ROM”收窄为“FW 虽已起来，但 `ManufacturingStatus` MAC 回包异常且电芯读数仍是几十 mV 悬空签名”。
- 2026-03-11: 工具报告语义继续细化：`summary.json` 新增 `rom_events.fw_seen`、`rom_events.runtime_invalid`、`rom_events.runtime_status_unconfirmed`，用于区分“已退出 ROM 但运行态无效/状态块不可判定”和“仍停留在 ROM”。
- 2026-03-11: 按最终收口口径将本规格标记为已完成：`tools/bq40-comm-tool` 已能独立完成 ROM 检测 / 镜像写入 / ROM 退出 / post-flash 无效运行态的分层诊断，剩余未恢复正常通信的部分归因于样片最终硬件运行态，而不再视为工具链主路径未收敛。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
