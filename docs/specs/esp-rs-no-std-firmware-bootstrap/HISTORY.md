# 初始化 ESP32-S3（esp-rs / esp-hal）no_std 固件工程 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-01-22: 初始化计划与接口契约骨架
- 2026-01-22: 落地 `firmware/` 最小工程骨架与 bring-up 文档；默认 `defmt`（espflash 解码）并集成 `mcu-agentd` 配置
- 2026-01-22: 根据工作流约束，将 `mcu-agentd.toml` 固定到仓库根目录，并同步更新接口契约与文档口径
- 2026-01-22: `mcu-agentd` 串口缓存补充 MAC 绑定格式（`firmware/.esp32-port`）；将固件输出标识统一为 `esp:*` 并为 `Instant::now()` 增加必要的 timer/watchdog 初始化（需重刷验证）

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
