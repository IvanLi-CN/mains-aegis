# TPS55288 双路输出控制 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-01-23
- Last: 2026-01-26

- [x] M1: 落地 `TPS55288` 最小驱动封装（I2C 读写 + 关键寄存器配置）并在启动时应用默认 profile
- [x] M2: 初始化 `INA3221` 并输出 `500ms` 周期遥测日志（OUT-A/OUT-B：`vset/vbus/current`）
- [x] M3: 落地 fault/告警的最小观测与日志口径（`I2C1_INT(GPIO33)` + 状态读取/解析）
- [x] M4: 固化上板验证步骤与测量口径到 `firmware/README.md`

## References

- `./SPEC.md`
- `./HISTORY.md`
