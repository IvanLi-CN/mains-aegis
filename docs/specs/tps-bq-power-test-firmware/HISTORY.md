# 独立 TPS/BQ 电源测试固件（feature 驱动屏显测试套件） 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-21: 新建规格，冻结独立 `tps-test-fw` 的范围、默认 profile、UI 与验证门槛。
- 2026-03-21: 实现 `tps-test-fw`、固定 profile 运行时、独立屏显页与构建验证。
- 2026-03-27: 将 `tps-test-fw` 收敛为 feature 驱动测试套件，补齐 `5V/12V/15V/19V`、`1.5A/3.5A`、`FPWM/PFM`、`A/B/Both`、`charge off/min/1A` 选择面，并写入样机调试笔记。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
