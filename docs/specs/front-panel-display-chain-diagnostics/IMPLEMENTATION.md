# 前面板显示链路长按诊断与重初始化 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-04-03
- Last: 2026-04-04

- [x] M1: 建立规格并登记到 `docs/specs/README.md`
- [x] M2: 在主固件里新增 `CENTER` 长按状态机与单次按压闸门
- [x] M3: 新增长按诊断采样日志与共享显示链路重初始化 helper
- [x] M4: README 同步长按诊断入口、日志契约与运行时行为
- [x] M5: 真机验证 + fast-track PR 收敛到 merge-ready

## References

- `./SPEC.md`
- `./HISTORY.md`
