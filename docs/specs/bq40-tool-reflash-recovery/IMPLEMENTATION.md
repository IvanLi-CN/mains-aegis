# BQ40 工具链 reflash / recovery 收敛 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-03-06
- Last: 2026-03-11

- None。

- None

- [x] M1: `run.sh` / `build.sh` 的 `--force-min-charge` live 参数合同与透传路径收敛完成。
- [x] M2: 工具固件强制唤醒参数恢复为 `16.8V / 200mA / 500mA`，并能在无电池外部供电台架上留下可核对日志。
- [x] M3: `report_parser.py` 与相关日志阶段语义收紧完成，`flash_done` 仅在真实 ROM flash reflash 完成时置位。
- [x] M4: `tools/bq40-comm-tool` 文档、操作手册与离线 verify 口径同步完成。

## References

- `./SPEC.md`
- `./HISTORY.md`
