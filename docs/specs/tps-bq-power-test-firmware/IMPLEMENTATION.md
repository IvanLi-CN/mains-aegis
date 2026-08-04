# 独立 TPS/BQ 电源测试固件（feature 驱动屏显测试套件） 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-03-21
- Last: 2026-03-27

- Directory: `docs/specs/tps-bq-power-test-firmware/assets/`
- In-spec references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

None

- [x] M1: 新建 `tps-test-fw` feature/bin 与独立规格索引。
- [x] M2: 完成最小板级 bring-up 与固定 profile 运行时。
- [x] M3: 完成 charger/TPS/INA/TMP 轮询、故障锁存与基础保护。
- [x] M4: 完成专用 `TPS TEST` 单页 UI 与 front-panel 渲染入口。
- [x] M5: 更新 `firmware/README.md` 并完成三组 `cargo +esp check`。

## References

- `./SPEC.md`
- `./HISTORY.md`
