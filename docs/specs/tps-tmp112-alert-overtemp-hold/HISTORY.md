# TMP112A 过温告警输出：Comparator 模式保持输出 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 固件已实现两颗 TMP112A 的 comparator 配置、阈值写入与回读，并在启动路径保留 fail-safe 与 `THERM_KILL_N` 诊断；该主题因此按完成态归档。
- `THERM_KILL_N` 同时承载 TMP112 外部硬停机与 MCU TPS I2C 失联硬抑制；两者共享电平保护效果，但 runtime 记录 MCU 持有状态，release 后仍低时明确为外部或未知来源。

## Decision Trace

- 2026-01-27: 初始化计划骨架与契约入口

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
