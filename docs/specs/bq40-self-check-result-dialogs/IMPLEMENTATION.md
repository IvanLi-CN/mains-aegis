# BQ40 自检异常态与结果弹窗 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: active
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 部分完成（4/5）
- Created: 2026-03-11
- Last: 2026-04-03

- Directory: `docs/specs/bq40-self-check-result-dialogs/assets/`
- Result dialog assets:
  - `self-check-c-bq40-result-success.png`
  - `self-check-c-bq40-result-no-battery.png`
  - `self-check-c-bq40-result-rom-mode.png`
  - `self-check-c-bq40-result-abnormal.png`
  - `self-check-c-bq40-result-not-detected.png`
  - `self-check-c-bq40-offline-activate-dialog.png`
  - `self-check-c-bq40-activating.png`
  - `self-check-c-bq40-discharge-blocked.png`
  - `self-check-c-bq40-discharge-recovery-dialog.png`
  - `self-check-c-bq40-discharge-recovering.png`

| Asset | Plan source (path) | Used by (runtime/test/docs) | Promote method (copy/derive/export) | Target (project path) | References to update | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| Result dialog PNG set | `docs/specs/bq40-self-check-result-dialogs/assets/*.png` | docs | copy | `firmware/ui/assets/*.png` | `firmware/ui/*.md`, `firmware/README.md` | PR 展示与项目文档共用同一批冻结图 |

- [x] M1: 新增 `BQ40Z50` 三层卡片状态与结果持久化枚举
- [x] M2: 补齐问题详情/恢复弹窗 renderer 与预览场景
- [x] M3: 激活运行态改为 BQ40-only 分类，完全不可访问时固定落 `NOT DETECTED`
- [x] M4: 文档与规格资产同步完成
- [x] M4.5: 放电授权恢复显式处理 `EMSHUT` 并执行 communication-exit 触发
- [ ] M5: 构建、预览验证与快车道 PR 收敛完成

## References

- `./SPEC.md`
- `./HISTORY.md`
