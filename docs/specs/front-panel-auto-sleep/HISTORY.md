# 变更历史（front-panel-auto-sleep）

- 2026-04-27: 创建自动熄屏规格，采用测试版 `30s / 35s / 40s` 阈值；正式默认记录为 `180s / 240s / 245s`，等待硬件确认后恢复。
- 2026-07-01: 将自检页无法进入 dashboard 的阻塞状态纳入 `attention_hold`，避免 BMS 恢复或硬件未就绪期间屏幕进入 sleep 后呈现黑屏。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
