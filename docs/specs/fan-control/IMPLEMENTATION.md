# 风扇温控与故障保护 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: active
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 部分完成（4/5）
- Created: 2026-03-13
- Last: 2026-04-05

## Current Coverage

- 温控实现使用 `37C` 停转、`40C` 目标温度与 `500ms` 渐进 PWM 调节；`FanLevel` 仅对连续 PWM 百分比分组显示。
- tach PPR 由 `fan-tach-1-ppr` / `fan-tach-2-ppr` 构建 feature 选择并纳入 `FW_FEATURES` 固件身份；未指定时默认 `2 PPR`，双选由编译期门禁拒绝。
- PPR 参与 RPM 换算与采样窗口，不参与 tach 超时故障判定或温控闭环。

## Migrated Delivery Record

## 里程碑（Milestones）

- [x] M1: 新增风扇 spec 与索引。
- [x] M2: 接入 GPIO/PWM/tach 中断初始化。
- [x] M3: 完成风扇状态机与 `PowerManager` 集成。
- [x] M4: 补充测试、README 与日志契约。
- [ ] M5: 验证、PR 与 review-loop 收敛。


## References

- `./SPEC.md`
- `./HISTORY.md`
