# USB-C PD/PPS Sink 首阶段实现 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-04-07
- Last: 2026-04-23

- Directory: `docs/specs/usb-c-pd-sink-pps/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

None。

- [x] M1: 新建 spec、登记索引，并冻结 feature / 安全边界 / 验收口径
- [x] M2: 将 `FrontPanel` 改为共享 I2C2 的泛型设备，并在主固件与两个测试固件完成接线迁移
- [x] M3: 新增 `usb_pd` 模块，完成 feature 驱动 capability 生成、固定 PDO / PPS 纯逻辑与 FUSB302 薄驱动骨架
- [x] M4: 将 PD sink manager 接入主循环与 `PowerManager` / `BQ25792` 运行时，补齐 `IINDPM/VINDPM` 与 unsafe-source 保护
- [x] M5: 完成测试、feature 编译矩阵、spec sync、提交/推送/PR 与 review-loop 收口

## References

- `./SPEC.md`
- `./HISTORY.md`
