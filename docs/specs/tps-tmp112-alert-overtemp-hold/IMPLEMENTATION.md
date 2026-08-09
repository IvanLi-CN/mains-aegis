# TMP112A 过温告警输出：Comparator 模式保持输出 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: complete；固件已写入并回读两颗 TMP112A 的 comparator、阈值与去抖配置，并保留 `THERM_KILL_N` 可观测性。GPIO40 仍在启动时释放；运行时 TPS retryable I2C 耗尽可作为独立于 TMP112 的 MCU 开漏停机源，诊断会与外部低电平区分。

## Migrated Implementation Record

- Status: 已实现
- Created: 2026-01-27
- Last: 2026-01-27

None

- [x] M1: `TMP112A` 配置与阈值写入/回读 API（两地址 `0x48/0x49`；按 `./contracts/config.md`）
- [x] M2: 启动阶段应用配置 + fail-safe 落地（配置失败则不使能 TPS 输出；日志可定位）
- [x] M3: 上板验证步骤与“过温来源提示（日志）”落地到文档（`firmware/README.md`）

## References

- `./SPEC.md`
- `./HISTORY.md`
