# TPS 热点温度采样：TMP112A 读数与日志口径 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-01-24
- Last: 2026-01-27

- [x] M1: 落地 `TMP112A` 最小驱动封装（I2C 读温度寄存器 + 解码为 `temp_c_x16`）
- [x] M2: 固化遥测字段追加（`telemetry ...` 行追加 `tmp_addr/temp_c_x16/therm_kill_n`；错误占位与限频）
- [x] M3: 固化上板验证步骤到 `firmware/README.md`

## References

- `./SPEC.md`
- `./HISTORY.md`
