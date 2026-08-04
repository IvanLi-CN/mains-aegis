# BQ40 `LOCK` 根因锁定与修复 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: active
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 部分完成（3/6）
- Created: 2026-04-13
- Last: 2026-04-13

- Directory: `docs/specs/bq40-lock-root-cause/assets/`
- In-spec references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

None

- [x] M1: 创建 `LOCK` 根因 spec 并冻结证据模型、三分流规则与修复边界。
- [x] M2: 补齐 `ChargingStatus(0x55)` 原始块读与 lifetime/termination 观测字段，并完成本地验证。
- [ ] M3: 完成一次从 `<90%` 解锁基线开始的 live pack 闭环抓取，产出单份时间线证据包。
- [x] M4: 按证据命中唯一修复分支并完成实现。
- [ ] M5: 完成修复后的 live pack 闭环复验，证明不再重入 `OC/LOCK`。
- [ ] M6: PR 收敛、合并与最终上板确认完成。

## References

- `./SPEC.md`
- `./HISTORY.md`
