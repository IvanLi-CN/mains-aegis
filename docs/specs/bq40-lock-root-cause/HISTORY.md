# BQ40 `LOCK` 根因锁定与修复 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-04-13: 新建 spec，冻结 `LOCK` 根因锁定与修复的证据门、分流规则与快车道收口条件。
- 2026-04-13: 完成 `0x55` 原始块读与 BQ40/BQ25792 低频诊断观测补强，并通过主机侧验证。
- 2026-04-13: 依据 live failure evidence 命中 `termination` 分流门；实施 `BQ40 Current at EoC -> BQ25792 ITERM` 对齐修复，并完成 clean build 上板验证。
- 2026-04-13: 依据后续 live evidence 补充确认 `HT -> IN -> XCHG` 是新的前置 blocker，并把 live DF 主板基线的起充高温门槛上调到 `40°C`、回差上调到 `2°C`。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
