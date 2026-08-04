# PCB netlist sync (2026-03-19) 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-19: 初始化并完成本规格；主板网表同步到 2026-03-19 导出版本，前面板导出确认零差异，同时修正主板 README 与 TPS55288 合同中的旧输出拓扑描述。
- 2026-03-19: merge-proof review fix，澄清 `ISP_TPSA/ISP_TPSB -> R68/R83 -> VOUT_TPS` 的输出路径，避免把每路 TPS 输出侧网络与共享节点混写。
- 2026-03-19: merge-proof review fix，TPS55288 合同表改为同时记录 `Output-side net` 与 `Shared output node`，保留每路通道区分并明确共享输出节点。
- 2026-03-19: merge-proof review fix，修正 INA3221 CH1/CH2 文档映射，明确 `IN-1/IN-2` 均采样共享节点 `VOUT_TPS`，与主板网表中的 `R105..R108/U22` 一致。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
