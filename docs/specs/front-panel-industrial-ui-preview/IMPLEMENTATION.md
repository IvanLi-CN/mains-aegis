# 前面板工业仪表风 UI + 1:1 预览 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: superseded
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 重新设计（mcu-self-check-live-panel）
- Created: 2026-02-26
- Last: 2026-03-01

- Directory: `docs/specs/front-panel-industrial-ui-preview/assets/`
- In-spec references:
  - `assets/dashboard-b-off-mode.png`
  - `assets/dashboard-b-standby-mode.png`
  - `assets/dashboard-b-supplement-mode.png`
  - `assets/dashboard-b-backup-mode.png`
  - `assets/dashboard-home-focus-output.png`
  - `assets/dashboard-home-focus-battery-flow.png`
  - `assets/dashboard-menu-dashboard.png`
  - `assets/dashboard-menu-beeper.png`
  - `assets/dashboard-audio-action-focus.png`
  - `assets/dashboard-audio-system-focus.png`
  - `assets/dashboard-audio-system-off.png`
  - `assets/dashboard-menu-transition-mid.png`
  - `assets/dashboard-menu-transition-end.png`
  - `assets/dashboard-menu-beeper-review-set.png`
  - `assets/self-check-c-standby-idle.png`
  - `assets/self-check-c-standby-right.png`
  - `assets/self-check-c-assist-up.png`
  - `assets/self-check-c-backup-touch.png`
  - `assets/self-check-c-bq40-offline-idle.png`
  - `assets/self-check-c-bq40-offline-activate-dialog.png`
  - `assets/self-check-c-bq40-activating.png`
  - `assets/self-check-c-bq40-activation-succeeded.png`
  - `assets/self-check-c-bq40-activation-failed.png`

None

- [x] M1: 建立 `docs/specs` 根目录与新规格文档
- [x] M2: 新增共享 scene renderer 并接入固件
- [x] M3: 集成 A/B 双字体策略（标签 A、数值 B 等宽）
- [x] M4: 新增主机预览工具并输出 raw + PNG
- [x] M5: 批量导出 21 张状态预览图
- [x] M6: 更新 README 与旧计划口径漂移
- [x] M7: Dashboard 视觉冻结（主人评审通过，按 Variant B 锁定）

## References

- `./SPEC.md`
- `./HISTORY.md`
