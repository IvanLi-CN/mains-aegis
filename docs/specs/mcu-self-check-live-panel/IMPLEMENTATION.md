# MCU 自检页实时化与常驻显示 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: superseded
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 重新设计（dashboard-live-after-self-check）
- Created: 2026-03-01
- Last: 2026-03-15

## Migrated Delivery Record

## 里程碑（Milestones）

- [x] M1: 自检快照模型与进度回调落地。
- [x] M2: `Variant C` 渲染切换到真实数据分支。
- [x] M3: 启动顺序调整为“先显示自检页，再执行自检”。
- [x] M4: 自检完成后常驻显示 + 运行期实时刷新。
- [x] M5: 文档与构建验证同步完成。


## References

- `./SPEC.md`
- `./HISTORY.md`
