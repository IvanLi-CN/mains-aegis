# 前面板显示链路重构提速 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: active
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成（5/5）
- Created: 2026-03-15
- Last: 2026-03-15

- Directory: `docs/specs/front-panel-refresh-pipeline/assets/`

None

- [x] M1: 新增 `display_pipeline`，提供 double-buffer / dirty-row / band merge 纯逻辑。
- [x] M2: `FrontPanel` 运行时绘制切换到 framebuffer painter，不再在图元粒度直写 SPI。
- [x] M3: `PanelIo` 接入 `SPI2 + GDMA`，以 full-width dirty bands 刷新 GC9307。
- [x] M4: 主固件与 `test-fw` 切换到 `PSRAM + DMA_CH1` 显示主路径，默认运行时 `40MHz`，并保留 `20MHz` 中间档。
- [x] M5: PR / review-loop / 联调结论同步回规格与索引。

## References

- `./SPEC.md`
- `./HISTORY.md`
